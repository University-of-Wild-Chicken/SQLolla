"""Adversarial PostgreSQL planner/statement errors and lazy-expression outcome checks."""
import json

from cases import fixture, unary_table
from corpus_runtime import checked, describe
from libpq_client import PgConnection
from pg_runtime import TemporaryPostgres
from rejections import execute
from run import CLI, RESULTS, digest, write_json


def cases() -> list[dict]:
    """Keep planning-time constants distinct from row-dependent errors and pruned branches."""
    examples = [("constant_empty", "SELECT 1 / 0 FROM r", []),
                ("variable_empty", "SELECT x / 0 FROM r", []),
                ("case_constant", "SELECT CASE WHEN x=0 THEN 1/0 ELSE 0 END FROM r", [1]),
                ("case_pruned", "SELECT CASE WHEN FALSE THEN 1/0 ELSE 0 END FROM r", [1]),
                ("coalesce_constant", "SELECT COALESCE(x,1/0) FROM r", [1]),
                ("case_variable", "SELECT CASE WHEN x=0 THEN 1/x ELSE 0 END FROM r", [1]),
                ("overflow_constant", "SELECT CASE WHEN x=0 THEN 2147483647+1 ELSE 0 END FROM r", [1]),
                ("and_pruned", "SELECT FALSE AND 1/0=0 FROM r", [1]),
                ("or_constant", "SELECT x=1 OR 1/0=0 FROM r", [1]),
                ("null_division", "SELECT x / NULL FROM r", [1]),
                ("false_filter", "SELECT 1/0 FROM r WHERE FALSE", [1]),
                ("negative_limit", "SELECT x FROM r LIMIT -1", [1]),
                ("negative_offset", "SELECT x FROM r OFFSET -1", [1])]
    return [fixture("expression_outcome/" + label, sql, {"r": unary_table("int32", tuple(rows))}, [], "expression_outcome")
            for label, sql, rows in examples]


def postgres(connection, case: dict) -> dict:
    """Measure EXPLAIN without ANALYZE separately so error-phase claims have actual evidence."""
    checked(connection, "CREATE TEMP TABLE r(x integer)")
    rows = case["oracle"]["tables"]["r"]["rows"]
    if rows: checked(connection, "INSERT INTO r VALUES " + ",".join("(" + str(row[0]) + ")" for row in rows))
    planned = connection.query("EXPLAIN " + case["request"]["sql"])
    result = describe(connection, connection.query(case["request"]["sql"]))
    checked(connection, "DROP TABLE r")
    phase = "planning" if planned["status"] == "error" else "statement"
    return {"result": result, "explain": planned, "observed_error_phase": phase if result["status"] == "error" else None}


def matches(expected: dict, actual: dict) -> bool:
    """Require actual SQL outcomes; coverage/engine refusals never substitute for expected errors."""
    if actual.get("status") != "ok" or not actual.get("complete"): return False
    reference = expected["result"]
    for outcome in actual["outcomes"]:
        value = outcome["outcome"]
        if reference["status"] == "error":
            if outcome["status"] == "sql_error" and value["sqlstate"] == reference["sqlstate"]:
                if value["phase"] in ("unspecified", expected["observed_error_phase"]): return True
        elif outcome["status"] == "succeeded":
            rows = [[None if cell["type"] == "null" else str(cell["value"]).lower() if cell["type"] == "bool" else str(cell["value"]) for cell in row] for row in value["rows"]]
            rows = [["t" if value == "true" else "f" if value == "false" else value for value in row] for row in rows]
            if rows == reference["rows"]: return True
    return False


def main() -> None:
    """Retain failing adversarial evidence until the evaluator closes every semantic gap."""
    fixtures = cases()
    with TemporaryPostgres() as database:
        with PgConnection(database) as connection:
            expected = [postgres(connection, case) for case in fixtures]
        version = database.execute("SELECT version();").strip()
    responses = execute([case["request"] for case in fixtures])
    if len(responses) != len(fixtures): raise ValueError("Incomplete expression-outcome responses")
    results = [{"id": case["request"]["id"], "sql": case["request"]["sql"], "status": "passed" if matches(oracle, response) else "failed",
                "postgres": oracle, "response": response} for case, oracle, response in zip(fixtures, expected, responses)]
    failures = [row["id"] for row in results if row["status"] == "failed"]
    report = {"status": "failed" if failures else "passed", "cases": len(results), "failures": failures,
              "version": version, "cli_sha256": digest(CLI.read_bytes()), "results": results}
    write_json(RESULTS / "expression-outcomes.json", report)
    print(json.dumps({key: value for key, value in report.items() if key != "results"}))
    if failures: raise SystemExit(1)


if __name__ == "__main__":
    main()

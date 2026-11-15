"""Actual PostgreSQL CTE snapshot, joined mutation, conflict and explicit DEFAULT observations."""
import json

from corpus_runtime import checked, describe
from corpus_semantics import observation_matches, schema_matches
from libpq_client import PgConnection
from oracle_contract import index_responses, top_level_ordered
from pg_runtime import TemporaryPostgres
from rejections import execute
from run import CLI, RESULTS, digest, write_json


def examples() -> list[dict]:
    """Keep same-snapshot reads distinct from explicit RETURNING visibility and target identity."""
    sqls = ["WITH u AS (UPDATE t SET v=9 RETURNING *) SELECT * FROM t",
            "WITH u AS (UPDATE t SET v=9 RETURNING *) SELECT * FROM u",
            "WITH u AS (UPDATE t SET v=9 RETURNING *) SELECT * FROM t UNION ALL SELECT * FROM u",
            "WITH u AS (UPDATE t SET v=9) SELECT 1",
            "WITH u AS (SELECT 1/0) SELECT * FROM t",
            "WITH u AS (UPDATE t SET v=9 RETURNING *) INSERT INTO s SELECT * FROM u RETURNING *",
            "WITH u AS (UPDATE t SET v=9 RETURNING *) INSERT INTO s VALUES(2,NULL) RETURNING *",
            "UPDATE t SET v=s.v FROM s WHERE t.k=s.k RETURNING t.*,s.v AS source_v",
            "DELETE FROM t USING s WHERE t.k=s.k RETURNING t.*",
            "WITH d AS (DELETE FROM t USING s WHERE t.k=s.k RETURNING t.*) SELECT * FROM t UNION ALL SELECT * FROM d",
            "INSERT INTO t VALUES(2,DEFAULT) RETURNING *",
            "INSERT INTO t VALUES(2,NULL) RETURNING *",
            "INSERT INTO t VALUES(1,9) ON CONFLICT(k) DO UPDATE SET v=excluded.v RETURNING *",
            "INSERT INTO t VALUES(1,9),(1,2) ON CONFLICT(k) DO NOTHING RETURNING *",
            "INSERT INTO t VALUES(1,9),(1,2) ON CONFLICT(k) DO UPDATE SET v=excluded.v WHERE false RETURNING *",
            "INSERT INTO t VALUES(1,9),(1,2) ON CONFLICT(k) DO UPDATE SET v=excluded.v RETURNING *",
            "INSERT INTO t VALUES(1,-1) ON CONFLICT(k) DO NOTHING RETURNING *",
            "INSERT INTO t SELECT 2,7 WHERE false ON CONFLICT(v) DO NOTHING",
            "INSERT INTO t VALUES(2,7) ON CONFLICT ON CONSTRAINT absent DO NOTHING"]
    return [{"sql": sql, "t": rows, "s": [[1, 4]]} for sql in sqls for rows in ([], [[1, 0]])]


def catalog() -> dict:
    """Wire exactly the two physical integer schemas used by PostgreSQL setup."""
    columns = [{"name": name, "ty": "int32", "nullable": False} for name in ("k", "v")]
    return {"search_path": ["public"], "tables": {name: {"schema": "public", "name": name,
            "columns": columns} for name in ("t", "s")}}


def metadata() -> dict:
    """Complete declarations include primary key, CHECK and literal default behavior."""
    default = {"kind": "literal", "value": {"type": "int32", "value": 7}}
    return {"t": {"complete": True, "defaults": {"1": default},
            "primary_key": {"name": "t_pkey", "columns": [0]},
            "checks": [{"name": "t_v_check", "sql": "v >= 0", "parser": None}]}, "s": {"complete": True}}


def request(case: dict, index: int) -> dict:
    """Supply exact initial row bags and explicit execution constraints without result lookup."""
    database = {name: [[{"type": "int32", "value": cell} for cell in row] for row in case[name]] for name in ("t", "s")}
    return {"id": "cte/" + str(index), "sql": case["sql"], "catalog": catalog(), "database": database,
            "parameters": {}, "mode": "compare", "execution": {"max_rows": 256, "max_outcomes": 256,
            "settings": {}, "table_metadata": metadata()}}


def initialize(connection, case: dict) -> None:
    """Fixture DDL is independent of the Rust binder and records actual named constraints."""
    checked(connection, "DROP TABLE IF EXISTS t,s")
    checked(connection, "CREATE TABLE t(k integer PRIMARY KEY,v integer NOT NULL DEFAULT 7 CONSTRAINT t_v_check CHECK(v>=0))")
    checked(connection, "CREATE TABLE s(k integer NOT NULL,v integer NOT NULL)")
    for name in ("t", "s"):
        if case[name]: checked(connection, "INSERT INTO " + name + " VALUES " + ",".join("(" + ",".join(map(str, row)) + ")" for row in case[name]))


def postgres(connection, case: dict) -> dict:
    """Observe EXPLAIN phase, exact command diagnostics and table state after atomic SQL failure."""
    initialize(connection, case)
    explained = connection.query("EXPLAIN " + case["sql"])
    checked(connection, "BEGIN")
    checked(connection, "SAVEPOINT statement_boundary")
    result = describe(connection, connection.query(case["sql"]))
    if result["status"] == "error": checked(connection, "ROLLBACK TO SAVEPOINT statement_boundary")
    state = {name: describe(connection, checked(connection, "SELECT * FROM " + name)) for name in ("t", "s")}
    checked(connection, "ROLLBACK")
    return {"result": result, "after": {"database": state}, "explain": explained,
            "observed_phase": "planning" if explained["status"] == "error" else "statement"}


def compare(submitted: dict, oracle: dict, response: dict) -> dict:
    """Every admitted outcome must match the deterministic PG observation including full poststate."""
    errors = []
    if response.get("status") != "ok" or not response.get("complete") or response.get("validation") != "ALLOY_TABLE_ROUNDTRIP":
        errors.append("incomplete_execution")
    else:
        values = response["outcomes"]
        expected = oracle["result"]
        for outcome in values:
            if outcome["outcome"]["effects"] or not observation_matches(outcome, expected, oracle["after"], submitted["catalog"], {}, top_level_ordered(submitted["sql"])):
                errors.append("postgres_outcome_mismatch")
            if outcome["status"] == "sql_error" and outcome["outcome"]["phase"] not in ("unspecified", oracle["observed_phase"]): errors.append("phase_mismatch")
        if not values: errors.append("empty_outcomes")
        if expected["status"] == "ok" and not schema_matches(response["schema"], expected["columns"], {}): errors.append("schema_mismatch")
    return {"id": submitted["id"], "sql": submitted["sql"], "sql_sha256": digest(submitted["sql"].encode()),
            "status": "failed" if errors else "passed", "errors": sorted(set(errors)), "postgres": oracle, "response": response}


def main() -> None:
    """Retain one canonical oracle artifact; PostgreSQL is independent of the Rust finite-table encoder."""
    cases = examples()
    submitted = [request(case, index) for index, case in enumerate(cases)]
    responses = index_responses(submitted, execute(submitted, timeout=240))
    with TemporaryPostgres() as database:
        with PgConnection(database) as connection:
            results = [compare(wire, postgres(connection, case), responses[wire["id"]]) for case, wire in zip(cases, submitted)]
        version, directory = database.execute("SELECT version();").strip(), database.directory
    failures = [result["id"] for result in results if result["status"] == "failed"]
    report = {"status": "failed" if failures else "passed", "cases": len(results), "failures": failures, "results": results,
              "postgres_version": version, "cluster_removed": not directory.exists(), "cli_sha256": digest(CLI.read_bytes()),
              "claim": "Actual PG observations compared with every Rust outcome; Alloy checks the evaluator-backed finite table.",
              "unobserved": ["physical mutation row locks", "concurrent schedules", "transaction-wide constraint timing"]}
    write_json(RESULTS / "cte.json", report)
    print(json.dumps({key: value for key, value in report.items() if key != "results"}))
    if failures: raise SystemExit(1)


if __name__ == "__main__":
    main()

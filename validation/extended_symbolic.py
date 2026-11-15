"""Check complete finite outcomes, SAT verdicts and witnesses against PostgreSQL permutations."""
from copy import deepcopy
from itertools import combinations_with_replacement, permutations
import json

from cases import fixture, unary_table, typed_cell
from corpus_runtime import checked, describe
from corpus_semantics import observation_matches, schema_matches
from libpq_client import PgConnection
from oracle_contract import index_responses, top_level_ordered
from pg_runtime import TemporaryPostgres
from rejections import execute
from run import CLI, RESULTS, digest, write_json


def cases() -> list[dict]:
    """Use independent arithmetic errors, writes, ordered limits and unordered alternatives."""
    definitions = [("division", "SELECT 1/x FROM r", [0, 1], {"kind": "no_sql_error"}),
                   ("overflow_write", "UPDATE r SET x=x+1 RETURNING x", [0, 2147483647], {"kind": "no_sql_error"}),
                   ("ordered_safe", "SELECT x FROM r ORDER BY x LIMIT 1", [None, 0, 1], {"kind": "max_rows", "max": 1}),
                   ("ordered_leak", "SELECT x FROM r ORDER BY x LIMIT 1", [None, 0, 1], {"kind": "no_value", "column": 0, "value": {"type": "int32", "value": 1}}),
                   ("unordered_leak", "SELECT x FROM r LIMIT 1", [None, 0, 1], {"kind": "no_value", "column": 0, "value": {"type": "int32", "value": 1}})]
    result = []
    for name, sql, domain, policy in definitions:
        request = fixture("extended_symbolic/" + name, sql, {"r": unary_table("int32", ())}, ["int32"], "symbolic")["request"]
        request.update(mode="analyze", database={}, bounds={"public.r": 2}, property=policy,
                       domain=[typed_cell(value, "int32") for value in domain],
                       execution={"max_rows": 256, "max_outcomes": 256, "settings": {}, "table_metadata": {"public.r": {"complete": True}}})
        result.append({"request": request, "domain": domain})
    return result


def observe(connection, sql: str, rows: tuple) -> dict:
    """Retain outputs and poststate before rollback, recovering statement errors via savepoint."""
    checked(connection, "BEGIN")
    checked(connection, "CREATE TEMP TABLE r(x integer) ON COMMIT DROP")
    if rows: checked(connection, "INSERT INTO r VALUES " + ",".join("(" + ("NULL" if n is None else str(n)) + ")" for n in rows))
    checked(connection, "SAVEPOINT observed")
    result = connection.query(sql)
    if result["status"] == "error": checked(connection, "ROLLBACK TO observed")
    result = describe(connection, result)
    result["post_rows"] = sorted(checked(connection, "SELECT x FROM r")["rows"], key=repr)
    checked(connection, "ROLLBACK")
    return result


def violates(property: dict, observation: dict) -> bool:
    """Evaluate the small int32 policy directly on independent PostgreSQL observations."""
    if property["kind"] == "no_sql_error": return observation["status"] == "error"
    rows = observation.get("rows", observation.get("exposed_rows", []))
    if property["kind"] == "max_rows": return len(rows) > property["max"]
    forbidden = str(property["value"]["value"])
    return any(row[property["column"]] == forbidden for row in rows)


def oracle(connection, case: dict) -> dict:
    """Enumerate every bounded bag and every physical permutation of that bag."""
    observations, executions = [], 0
    for size in range(3):
        for rows in combinations_with_replacement(case["domain"], size):
            allowed = []
            for order in sorted(set(permutations(rows)), key=repr):
                result = observe(connection, case["request"]["sql"], order)
                executions += 1
                if result not in allowed: allowed.append(result)
            observations.append({"input": rows, "allowed": allowed,
                "violates": any(violates(case["request"]["property"], result) for result in allowed)})
    return {"has_counterexample": any(row["violates"] for row in observations),
            "observations": observations, "postgres_executions": executions}


def concrete_requests(case: dict, expected: dict) -> list:
    """Submit every independent oracle input through concrete complete-outcome evaluation."""
    requests = []
    for index, observation in enumerate(expected["observations"]):
        request = deepcopy(case["request"])
        for key in ("bounds", "domain", "property"): del request[key]
        request.update(id=request["id"] + "/input/" + str(index), mode="compare",
                       database={"public.r": [[typed_cell(value, "int32")] for value in observation["input"]]})
        requests.append(request)
    return requests


def matches(case: dict, outcome: dict, expected: dict) -> bool:
    """Check errors, typed rows, ordering, counts, full table state and absence of effects."""
    after = {"database": {"r": {"rows": expected["post_rows"]}}}
    return not outcome["outcome"]["effects"] and observation_matches(outcome, expected, after,
        case["request"]["catalog"], {}, top_level_ordered(case["request"]["sql"]))


def validate_concrete(case: dict, observation: dict, response: dict) -> dict:
    """Require both inclusions, rejecting any extra engine outcome or omitted PG alternative."""
    if response.get("status") != "ok" or response.get("validation") != "ALLOY_TABLE_ROUNDTRIP" or not response.get("complete"):
        return {"status": "failed", "reason": "incomplete_concrete_response", "response": response}
    outcomes, allowed = response["outcomes"], observation["allowed"]
    errors = []
    if not outcomes or any(not any(matches(case, actual, pg) for pg in allowed) for actual in outcomes): errors.append("extra_or_missing_engine_outcome")
    if any(not any(matches(case, actual, pg) for actual in outcomes) for pg in allowed): errors.append("missing_postgres_alternative")
    if any(pg["status"] == "ok" and not schema_matches(response["schema"], pg["columns"], {}) for pg in allowed): errors.append("schema_mismatch")
    return {"status": "failed" if errors else "passed", "errors": errors, "response": response}


def witness_admitted(request: dict, witness: dict) -> bool:
    """Require the entire witness to stay inside the declared closed catalog and typed domain."""
    database = witness.get("database", {})
    domain = {json.dumps(cell, sort_keys=True) for cell in request["domain"]}
    if set(database) != set(request["catalog"]["tables"]): return False
    for name, rows in database.items():
        columns = request["catalog"]["tables"][name]["columns"]
        if len(rows) > request["bounds"][name]: return False
        for row in rows:
            if len(row) != len(columns): return False
            for cell, column in zip(row, columns):
                if json.dumps(cell, sort_keys=True) not in domain: return False
                if cell["type"] == "null" and (cell["value"] != column["ty"] or not column["nullable"]): return False
                if cell["type"] != "null" and cell["type"] != column["ty"]: return False
    return witness.get("complete") is True


def validate_witness(connection, case: dict, response: dict) -> tuple:
    """Replay an admitted counterexample and compare its complete observable transition."""
    witness = response.get("instance", {})
    if not witness_admitted(case["request"], witness): return "witness_outside_domain", None
    rows = tuple(None if row[0]["type"] == "null" else row[0]["value"] for row in witness["database"]["public.r"])
    replays = [observe(connection, case["request"]["sql"], order) for order in sorted(set(permutations(rows)), key=repr)]
    if not any(violates(case["request"]["property"], pg) and matches(case, witness["outcome"], pg) for pg in replays):
        return "postgres_witness_outcome_mismatch", replays
    return None, replays


def validate(connection, case: dict, expected: dict, response: dict, concrete: list) -> dict:
    """Combine exhaustive outcome-set equality with independent SAT existence and replay checks."""
    wanted = "COUNTEREXAMPLE" if expected["has_counterexample"] else "NO_COUNTEREXAMPLE_WITHIN_BOUNDS"
    failure, replay = None, None
    if response.get("status") != "ok" or response.get("analysis") != wanted or response.get("complete") is not True:
        failure = "verdict_mismatch"
    if any(response.get(key) != case["request"][key] for key in ("bounds", "domain", "property")): failure = "analysis_context_mismatch"
    if failure is None and wanted == "COUNTEREXAMPLE": failure, replay = validate_witness(connection, case, response)
    results = [validate_concrete(case, observation, actual) for observation, actual in zip(expected["observations"], concrete)]
    if len(results) != len(expected["observations"]) or any(value["status"] != "passed" for value in results): failure = "concrete_outcome_set_mismatch"
    return {"id": case["request"]["id"], "status": "failed" if failure else "passed", "failure": failure,
            "postgres": expected, "response": response, "witness_replay": replay, "concrete_outcomes": results}


def run(connection, fixtures: list) -> list:
    """Correlate all analysis and concrete responses by exact unique identities."""
    expected = [oracle(connection, case) for case in fixtures]
    concretes = [concrete_requests(case, pg) for case, pg in zip(fixtures, expected)]
    requests = [case["request"] for case in fixtures] + [request for group in concretes for request in group]
    responses = index_responses(requests, execute(requests, timeout=300))
    return [validate(connection, case, pg, responses[case["request"]["id"]], [responses[request["id"]] for request in group])
            for case, pg, group in zip(fixtures, expected, concretes)]


def main() -> None:
    """Retain actual finite oracle evidence without claiming independent Alloy SQL semantics."""
    with TemporaryPostgres() as database:
        with PgConnection(database) as connection: results = run(connection, cases())
        version = database.execute("SELECT version();").strip()
        directory = database.directory
    failures = [row["id"] for row in results if row["status"] == "failed"]
    report = {"status": "failed" if failures else "passed", "cases": len(results), "failures": failures,
              "postgres_inputs": sum(len(row["postgres"]["observations"]) for row in results), "version": version,
              "postgres_executions": sum(row["postgres"]["postgres_executions"] for row in results),
              "cluster_removed": not directory.exists(), "cli_sha256": digest(CLI.read_bytes()), "results": results,
              "scope": "Exact concrete-outcome equality for all enumerated int32 input bags and physical permutations; SAT existence and admitted witnesses replayed independently in PostgreSQL.",
              "policy_value_equality": "Exact typed transport identity, not SQL coercion/duplicate equality.",
              "unobserved": ["SQL error planning/statement phase is not inferred from SQLSTATE"]}
    write_json(RESULTS / "extended-symbolic.json", report)
    print(json.dumps({key: value for key, value in report.items() if key != "results"}))
    if failures: raise SystemExit(1)


if __name__ == "__main__":
    main()

"""Check symbolic SAT existence against exhaustive PostgreSQL inputs and replay witnesses."""
import json

from cases import fixture, small_bags, typed_cell, unary_table
from oracle import oracle_script, rows_to_bag, typed_bag
from pg_runtime import TemporaryPostgres
from rejections import execute
from run import CLI, RESULTS, digest, index_lines, write_json

DOMAIN = [None, 0, 1]
QUERIES = {"scan": "SELECT x FROM r", "equal_zero": "SELECT x FROM r WHERE x = 0",
           "is_null": "SELECT x FROM r WHERE x IS NULL", "nonnull": "SELECT x FROM r WHERE x IS NOT NULL",
           "not_zero": "SELECT x FROM r WHERE x <> 0", "distinct": "SELECT DISTINCT x FROM r",
           "constant": "SELECT 0 AS x FROM r", "not_equal": "SELECT x FROM r WHERE NOT (x = 0)",
           "logic": "SELECT x FROM r WHERE (x = 0 OR x = 1) AND x IS NOT NULL",
           "union_all": "SELECT x FROM r UNION ALL SELECT x FROM r", "unused_catalog": "SELECT x FROM r"}


def properties() -> dict:
    """List explicit result properties without deriving their answers from Rust."""
    rules = {f"max_{maximum}": {"kind": "max_rows", "max": maximum} for maximum in [0, 1, 2, 4]}
    for label, value in [("zero", 0), ("one", 1), ("null", None)]:
        for kind in ["no_value", "only_value"]:
            rules[f"{kind}_{label}"] = {"kind": kind, "column": 0, "value": typed_cell(value, "int32")}
    return rules


def concrete_cases() -> list[dict]:
    """Enumerate every zero-to-two-row input bag for each symbolic query."""
    return [fixture(f"symbolic-oracle/{label}/{index}", sql, input_tables(label, bag), ["int32"], "symbolic_oracle")
            for label, sql in QUERIES.items() for index, bag in enumerate(small_bags(DOMAIN))]


def input_tables(label: str, bag: tuple) -> dict:
    """Include an explicitly empty unused catalog table in the completeness regression."""
    tables = {"r": unary_table("int32", bag)}
    if label == "unused_catalog": tables["s"] = unary_table("int32", ())
    return tables


def property_violated(rule: dict, rows: list) -> bool:
    """Evaluate the declared output policy directly over actual PostgreSQL rows."""
    if rule["kind"] == "max_rows": return len(rows) > rule["max"]
    target = None if rule["value"]["type"] == "null" else rule["value"]["value"]
    if rule["kind"] == "no_value": return any(row[rule["column"]] == target for row in rows)
    return any(row[rule["column"]] != target for row in rows)


def analysis_requests() -> list[dict]:
    """Request symbolic inputs with the same exact domains and bounds as enumeration."""
    requests = []
    for query_name, sql in QUERIES.items():
        for property_name, rule in properties().items():
            request = fixture(f"symbolic/{query_name}/{property_name}", sql, input_tables(query_name, ()), ["int32"], "symbolic")["request"]
            bounds = {name: 2 if name == "public.r" else 0 for name in request["catalog"]["tables"]}
            request.update(mode="analyze", database={}, bounds=bounds,
                           domain=[typed_cell(value, "int32") for value in DOMAIN], property=rule)
            requests.append(request)
    return requests


def expected_search(request: dict, oracle: dict) -> tuple[bool, list | None]:
    """Find the first actual-PG violation in a complete finite input enumeration."""
    label = request["id"].split("/")[1]
    for index, bag in enumerate(small_bags(DOMAIN)):
        result = oracle[f"symbolic-oracle/{label}/{index}"]["rows"]
        if property_violated(request["property"], result): return True, list(bag)
    return False, None


def replay_case(request: dict, response: dict) -> dict:
    """Validate decoded witness inputs before converting them to independent PG tables."""
    database = response["instance"]["database"]
    if set(database) != set(request["catalog"]["tables"]): raise ValueError("Witness table set differs from complete declared catalog")
    if response["instance"].get("completeness_scope") != "encoded relations and complete declared catalog":
        raise ValueError("Witness completeness scope missing or ambiguous")
    tables = {}
    for stable_id, rows in database.items():
        if len(rows) > request["bounds"][stable_id] or any(len(row) != 1 for row in rows): raise ValueError("Witness outside row bounds")
        values = []
        for row in rows:
            cell = row[0]
            if cell not in request["domain"]: raise ValueError("Witness cell outside finite domain")
            values.append(None if cell["type"] == "null" else cell["value"])
        name = request["catalog"]["tables"][stable_id]["name"]
        tables[name] = unary_table("int32", tuple(values))
    return fixture(request["id"], request["sql"], tables, ["int32"], "witness_replay")


def compare_search(requests: list[dict], responses: list[dict], oracle: dict) -> tuple[list, list, list]:
    """Check SAT/non-SAT existence and retain every candidate witness for PostgreSQL replay."""
    if len(requests) != len(responses): raise ValueError("Incomplete symbolic responses")
    outcomes, failures, replays = [], [], []
    for request, response in zip(requests, responses):
        exists, example = expected_search(request, oracle)
        expected = "COUNTEREXAMPLE" if exists else "NO_COUNTEREXAMPLE_WITHIN_BOUNDS"
        outcome = {"id": request["id"], "expected": expected, "actual": response.get("analysis"), "minimal_pg_input": example}
        outcomes.append(outcome)
        if response.get("status") != "ok" or response.get("analysis") != expected:
            failures.append(dict(outcome, response=response))
        elif exists:
            replays.append(replay_case(request, response))
    return outcomes, failures, replays


def validate_replays(replays: list[dict], rows: dict, requests: list[dict], responses: list[dict]) -> list:
    """Require decoded witness outputs to match PG and violate the requested policy."""
    requested = {request["id"]: request for request in requests}
    replied = {response["id"]: response for response in responses if "id" in response}
    failures = []
    for case in replays:
        identifier = case["request"]["id"]
        pg_rows = rows[identifier]["rows"]
        decoded = replied[identifier]["instance"]["output"]
        if rows_to_bag(pg_rows, ["int32"]) != typed_bag(decoded) or not property_violated(requested[identifier]["property"], pg_rows):
            failures.append({"id": identifier, "reason": "PostgreSQL witness replay mismatch", "postgres": pg_rows, "decoded": decoded})
    return failures


def main() -> None:
    """Execute exhaustive PG search, symbolic Alloy search and actual-PG witness replay."""
    requests = analysis_requests()
    with TemporaryPostgres() as database:
        runtime = json.loads(database.execute("SELECT json_build_object('version',version(),'listen',current_setting('listen_addresses'),'encoding',current_setting('server_encoding'),'collation',(SELECT datcollate FROM pg_database WHERE datname=current_database()));"))
        oracle_text = database.execute(oracle_script(concrete_cases()))
        oracle = index_lines(oracle_text)
        responses = execute(requests)
        outcomes, failures, replays = compare_search(requests, responses, oracle)
        replay_rows = index_lines(database.execute(oracle_script(replays))) if replays else {}
        failures.extend(validate_replays(replays, replay_rows, requests, responses))
        directory = database.directory
    report = {"status": "failed" if failures else "passed", "queries": len(QUERIES), "input_bags_per_query": 10,
              "actual_pg_inputs": len(oracle), "symbolic_properties": len(requests), "witnesses_replayed_in_pg": len(replays),
              "cluster_removed": not directory.exists(), "cli_sha256": digest(CLI.read_bytes()),
              "runtime": runtime, "failures": failures, "outcomes": outcomes}
    write_json(RESULTS / "symbolic.json", report)
    write_json(RESULTS / "symbolic-responses.json", responses)
    (RESULTS / "symbolic-oracle.jsonl").write_text(oracle_text, encoding="utf-8")
    print(json.dumps({key: value for key, value in report.items() if key != "outcomes"}))
    if failures: raise SystemExit(1)


if __name__ == "__main__":
    main()

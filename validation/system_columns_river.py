"""Nonempty unchanged River bulk-insert SQL validates tuple-header RETURNING with authentic DDL."""
from copy import deepcopy
import json

from corpus_runtime import checked, describe, parameters, pool_snapshots, query_context, snapshot
from corpus_schema import context_files, load_context
from corpus_wire import MANIFEST, inspect, request
from corpus_semantics import observation_matches, schema_matches
from corpus_tuples import capture, align, observe, matches
from libpq_client import PgConnection
from oracle_contract import index_responses
from pg_runtime import TemporaryPostgres
from rejections import execute
from run import CLI, RESULTS, digest, write_json


def array(values: list[str]) -> str:
    """Encode text protocol array elements without interpolating them into SQL source."""
    return "{" + ",".join('"' + value.replace("\\", "\\\\").replace('"', '\\"') + '"' for value in values) + "}"


def fixture(context: dict, keys: list[str]) -> dict:
    """Use aligned nonempty arrays with fresh explicit IDs and real unique-index conflict keys."""
    result = deepcopy(context)
    width = len(keys)
    values = [[str(201 + index) for index in range(width)], ['{"job":1}'] * width,
        ["2000-01-01 00:00:00+00"] * width, ["proposed"] * width, ["3"] * width,
        ["{}"] * width, ["1"] * width, ["default"] * width,
        ["2000-01-01 00:00:00+00"] * width, ["available"] * width,
        ["alpha,beta"] * width, keys, ["1"] * width]
    for parameter, items in zip(result["parameters"], values): parameter["value"] = array(items)
    result["fixture"] = "Nonempty aligned synthetic arrays under pinned authentic River DDL; explicit IDs avoid unrelated sequence allocation."
    return result


def initialize(connection) -> None:
    """Commit a real nonempty conflict candidate before allocating the analyzed writer identity."""
    checked(connection, "TRUNCATE river_job")
    checked(connection, "INSERT INTO river_job(id,args,kind,max_attempts,unique_key,unique_states) VALUES(100,'{}','original',3,'\\xaa',B'00000001')")


def record(connection, case: dict, semantic: dict, context: dict, label: str) -> dict:
    """Capture writer and initial tuple identity before running exact source SQL without a savepoint."""
    initialize(connection)
    checked(connection, "BEGIN")
    xid = int(checked(connection, "SELECT pg_current_xact_id()::text")["rows"][0][0])
    before = snapshot(connection)
    tables = {"public.river_job": semantic["catalog"]["tables"]["public.river_job"]}
    headers = align(capture(connection, tables, xid), before["database"])
    clock = checked(connection, "SELECT transaction_timestamp()::text")["rows"][0][0]
    result = connection.query(context["sql"], parameters(connection, context))
    if result["status"] == "error": checked(connection, "ROLLBACK")
    result = describe(connection, result)
    after, after_headers = snapshot(connection), observe(connection, tables)
    if result["status"] == "ok": checked(connection, "ROLLBACK")
    rolled_back, rollback_headers = snapshot(connection), observe(connection, tables)
    evidence = {"context": context, "transaction_timestamp": clock, "before": before, "after": after,
        "result": result, "after_rollback": rolled_back, "tuple_observations": {
        "catalog_tables": tables, "before": headers, "after": after_headers, "after_rollback": rollback_headers}}
    if result["status"] == "ok" and int(result["affected_rows"]) != len(context["parameters"][0]["value"].strip("{}").split(",")): raise AssertionError("Expected nonempty RETURNING")
    return {"label": label, "evidence": evidence, "sql_sha256": digest(context["sql"].encode()), "source_sql_sha256": case["sql_sha256"]}


def submitted(case: dict, semantic: dict, value: dict) -> dict:
    """Use the same authentic catalog translator as the maintained industrial coverage ledger."""
    evidence = value["evidence"]
    local = deepcopy(case)
    local["postgres_execution"] = deepcopy(evidence)
    local.pop("postgres_fixtures", None)
    pool = {"cases": [local]}
    pool_snapshots(pool)
    wire = request(pool, local, {"semantic_context": semantic}, local["postgres_execution"])
    wire["id"] = "river_tuple/" + value["label"]
    wire["mode"] = "compare"
    wire["execution"]["tuples"] = evidence["tuple_observations"]["before"]["context"]
    return wire


def compare(value: dict, wire: dict, response: dict, semantic: dict) -> dict:
    """Every returned alternative must match data/diagnostics and independent physical header bags."""
    evidence, errors = value["evidence"], []
    if response.get("status") != "ok" or not response.get("complete") or response.get("validation") != "ALLOY_TABLE_ROUNDTRIP": errors.append("incomplete")
    else:
        if not response["outcomes"]: errors.append("empty_outcomes")
        for outcome in response["outcomes"]:
            if not observation_matches(outcome, evidence["result"], evidence["after"], wire["catalog"], semantic["enums"], False): errors.append("postgres_data_mismatch")
            if not matches(outcome["outcome"]["effects"], evidence["tuple_observations"]): errors.append("tuple_header_mismatch")
        if evidence["result"]["status"] == "ok" and not schema_matches(response["schema"], evidence["result"]["columns"], semantic["enums"]): errors.append("schema_mismatch")
    return dict(value, status="failed" if errors else "passed", errors=sorted(set(errors)), request=wire, response=response)


def main() -> None:
    """Exercise fresh/conflicting/mixed/repeated unique keys, never credit an empty-array execution."""
    manifest = json.loads(MANIFEST.read_text(encoding="utf-8"))
    case = next(case for case in manifest["cases"] if case["id"].endswith(":jobInsertFastMany"))
    context = query_context(case, "")
    variants = [("fresh", ["\\xbb"]), ("conflict", ["\\xaa"]), ("mixed", ["\\xaa", "\\xbb"]),
                ("repeat_existing", ["\\xaa", "\\xaa"]), ("repeat_new", ["\\xbb", "\\xbb"])]
    with TemporaryPostgres() as database:
        loaded = load_context(database, context_files(manifest, "river"), "river", {})
        semantic = inspect(database)
        with PgConnection(database) as connection:
            checked(connection, "SET timezone='UTC'")
            records = [record(connection, case, semantic, fixture(context, keys), label) for label, keys in variants]
        version, directory = database.execute("SELECT version();").strip(), database.directory
    wires = [submitted(case, semantic, value) for value in records]
    responses = index_responses(wires, execute(wires, timeout=300))
    results = [compare(value, wire, responses[wire["id"]], semantic) for value, wire in zip(records, wires)]
    failures = [value["label"] for value in results if value["status"] != "passed"]
    report = {"status": "failed" if failures else "passed", "cases": len(results), "failures": failures, "results": results,
              "cli_sha256": digest(CLI.read_bytes()), "postgres_version": version, "cluster_removed": not directory.exists(),
              "loaded_sources": loaded, "source_id": case["id"], "claim": "Nonempty authentic-schema SQL fixture observations; bounded exclusive top-level tuple profile, not arbitrary MVCC correctness."}
    write_json(RESULTS / "system_columns_river.json", report)
    print(json.dumps({key: value for key, value in report.items() if key not in {"results", "loaded_sources"}}))
    if failures: raise SystemExit(1)


if __name__ == "__main__":
    main()

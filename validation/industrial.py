"""Replay unchanged pinned industrial SQL against PostgreSQL, Rust and Alloy."""
import argparse
from collections import Counter
from copy import deepcopy
import json
from pathlib import Path
import subprocess

from cases import fixture, table
from oracle import oracle_script
from pg_runtime import ROOT, TemporaryPostgres
from process_helpers import process_options
from rejections import execute
from run import CLI, RESULTS, compare_all, digest, index_lines, write_json

PIN = "ad15c585155ff7fc2c5fc4c9746905e4959ff5eb"
CONTEXTS = Path(__file__).with_name("industrial-contexts.json")


def load_sources(dependency: Path) -> tuple[dict, dict]:
    """Require the pinned checkout and exactly the industrial bytes measured by census."""
    head = subprocess.check_output(["git", "-C", str(dependency), "rev-parse", "HEAD"], encoding="utf-8", **process_options()).strip()
    if head != PIN: raise ValueError("SQLeanParser dependency commit changed")
    census = json.loads((ROOT / "parser/census-results.json").read_text(encoding="utf-8"))
    if census["status"] != "complete" or census["parser_commit"] != PIN: raise ValueError("Industrial census is missing or stale")
    records = {}
    for relative, evidence in census["corpus_files"].items():
        data = (dependency / relative).read_bytes()
        if digest(data) != evidence["sha256"]: raise ValueError("Industrial corpus bytes differ from census")
        for line, raw in enumerate(data.decode("utf-8").splitlines(), 1):
            record = json.loads(raw)
            records[record["id"]] = dict(record, corpus_file=relative, corpus_line=line)
    return records, census


def make_case(record: dict, profile: dict, mode: str) -> dict:
    """Use synthetic declared catalogs and bound parameters without changing SQL text."""
    tables = {}
    for name, definition in profile["tables"].items():
        rows = [] if mode == "empty" else [definition["matching"], definition["matching"], definition["nonmatching"]]
        tables[name] = table(definition["columns"], deepcopy(rows))
    case = fixture("industrial/" + record["id"] + "/" + mode, record["sql"], tables, profile["output_types"], "industrial")
    parameters = deepcopy(profile["parameters"])
    if mode == "typed_null_parameter":
        parameters[0] = {"type": "null", "value": parameters[0]["type"]}
    case["request"]["parameters"] = {str(index): value for index, value in enumerate(parameters, 1)}
    case["industrial"] = {"source_id": record["id"], "mode": mode, "context_kind": profile["context_kind"]}
    if case["request"]["sql"].encode("utf-8") != record["sql"].encode("utf-8"): raise ValueError("SQL was changed")
    return case


def selected_cases(records: dict, census: dict, configuration: dict) -> list[dict]:
    """Select only exported source records and verify their exact SQL fingerprints."""
    observations = {item["id"]: item for item in census["observations"]}
    cases = []
    for profile in configuration["profiles"]:
        record, observation = records[profile["id"]], observations[profile["id"]]
        if observation["outcome"] != "exported" or digest(record["sql"].encode("utf-8")) != observation["sql_sha256"]:
            raise ValueError("Selected industrial SQL lacks matching parser-export evidence")
        cases.extend(make_case(record, profile, mode) for mode in configuration["fixture_modes"])
    return cases


def provenance(record: dict) -> dict:
    """Retain the upstream origin and inherited extraction transformations explicitly."""
    return {"id": record["id"], "source_url": record["source_url"], "source_file": record["source_file"],
            "commit": record.get("commit", record.get("revision")), "corpus_file": record["corpus_file"],
            "corpus_line": record["corpus_line"], "sql_sha256": digest(record["sql"].encode("utf-8")),
            "license": record.get("license", "Apache-2.0 per BenchBase source manifest"),
            "inherited_extraction_transformations": record.get("transformations", []),
            "sql_unchanged_from_corpus": True, "catalog_kind": "synthetic_test_catalog_not_upstream_DDL"}


def unreplayed(census: dict, selected: set, selected_hashes: set) -> list[dict]:
    """Account for every exported but unreplayed record without inflating coverage."""
    result = []
    for item in census["observations"]:
        if item["outcome"] != "exported" or item["id"] in selected: continue
        reason = "catalog/type context not configured for this replay suite"
        if item["sql_sha256"] in selected_hashes: reason = "identical SQL hash replayed under another source ID; this record not separately executed"
        if item["population"] == "river": reason = "context/type blocked: runtime schema template and elected_at/expires_at timestamp context are outside the certified M1 test catalog"
        if item["id"].endswith(":listTablesQuery"): reason = "system-catalog context blocked: authentic information_schema relation not loaded as a business test fixture"
        result.append({"id": item["id"], "source_url": item["source_url"], "sql_sha256": item["sql_sha256"], "reason": reason})
    return result


def case_results(cases: list[dict], oracle: dict, responses: dict) -> list[dict]:
    """Retain reproducible parameter and exact row evidence for every industrial fixture."""
    results = []
    for case in cases:
        request, details = case["request"], case["industrial"]
        response = responses[request["id"]]
        results.append(dict(details, id=request["id"], parameters=request["parameters"],
                            input_sha256=digest(json.dumps(request["database"], sort_keys=True).encode()),
                            postgres_rows=oracle[request["id"]]["rows"], rust_bag=response.get("rust_bag"),
                            alloy_bag=response.get("alloy_bag"), alloy_status=response.get("alloy_status"),
                            status=response.get("status"), error=response.get("message")))
    return results


def build_report(records: dict, census: dict, configuration: dict, cases: list, oracle: dict, responses: dict, runtime: dict) -> dict:
    """Separate synthetic-context replay evidence from the full corpus coverage census."""
    selected = {profile["id"] for profile in configuration["profiles"]}
    sources = [provenance(records[identifier]) for identifier in sorted(selected)]
    hashes = {source["sql_sha256"] for source in sources}
    failures = compare_all(cases, oracle, responses)
    return {"status": "failed" if failures else "passed", "parser_commit": PIN,
            "cli_sha256": digest(CLI.read_bytes()), "runner_sha256": digest(Path(__file__).read_bytes()),
            "contexts_sha256": digest(CONTEXTS.read_bytes()), "context_claim": configuration["context_claim"],
            "baseline": {"records": census["records"], "distinct_sql_texts": census["distinct_sql_texts"], "outcomes": census["outcomes"]},
            "selected_source_records": len(selected), "distinct_sql_queries_replayed": len(hashes),
            "fixtures_per_query": len(configuration["fixture_modes"]), "comparisons": len(cases), "skipped_selected_cases": 0,
            "selected_populations": dict(Counter(identifier.split(":")[0] for identifier in selected)),
            "runtime": runtime, "parameters": "PostgreSQL typed PREPARE/EXECUTE; no SQL parameter substitution",
            "sources": sources, "unreplayed_exported_records": unreplayed(census, selected, hashes),
            "results": case_results(cases, oracle, responses), "failures": failures,
            "limitations": ["No production catalog, keys, constraints, collation, authorization or transaction behavior is certified.",
                            "BenchBase SQL retains its documented corpus JDBC-to-PostgreSQL conversion; no further SQL rewrite occurs here.",
                            "Duplicate and NULL fixtures intentionally stress the declared synthetic nullable schemas."]}


def main() -> None:
    """Execute actual PostgreSQL and both engines, then persist one industrial report."""
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--dependency", type=Path, default=ROOT.parent / "dependencies/SQLeanParser")
    records, census = load_sources(parser.parse_args().dependency)
    configuration = json.loads(CONTEXTS.read_text(encoding="utf-8"))
    cases = selected_cases(records, census, configuration)
    with TemporaryPostgres() as database:
        runtime = {"version": database.execute("SELECT version();").strip(), "listen": "127.0.0.1", "encoding": "UTF8", "collation": "C"}
        oracle = index_lines(database.execute(oracle_script(cases)))
        directory = database.directory
    runtime["cluster_removed"] = not directory.exists()
    replies = execute([case["request"] for case in cases])
    if len(replies) != len(cases): raise ValueError("Incomplete industrial CLI responses")
    responses = {response["id"]: response for response in replies}
    report = build_report(records, census, configuration, cases, oracle, responses, runtime)
    write_json(RESULTS / "industrial.json", report)
    print(json.dumps({key: report[key] for key in ["status", "distinct_sql_queries_replayed", "comparisons", "selected_populations", "failures"]}))
    if report["failures"]: raise SystemExit(1)


if __name__ == "__main__":
    main()

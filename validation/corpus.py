"""Maintain one per-source corpus coverage ledger; no stage substitutes for runtime."""
from collections import Counter
import json
from pathlib import Path

from corpus_features import inventory
from corpus_probe import probe_records
from industrial import load_sources
from pg_runtime import ROOT, TemporaryPostgres
from run import digest, write_json

MANIFEST = Path(__file__).with_name("corpus-manifest.json")


def preserve_progress(report: dict, previous: dict) -> dict:
    """Retain later context/execution evidence only when its exact SQL hash is unchanged."""
    indexed = {row["id"]: row for row in previous.get("cases", [])}
    for row in report["cases"]:
        prior = indexed.get(row["id"], {})
        if prior.get("sql_sha256") != row["sql_sha256"]: continue
        for key in ["postgres_authoritative_prepare", "context", "execution", "effects", "postgres_execution", "postgres_fixtures", "semantic_execution", "semantic_fixtures"]:
            if key in prior: row[key] = prior[key]
    for key in ["authoritative_contexts", "authoritative_probe_runtime", "observation_relations", "observation_states", "postgres_execution_scope", "semantic_coverage_target", "seed_fixtures"]:
        if key in previous: report[key] = previous[key]
    for key in ["authoritative_prepare", "authoritative_sqlstates", "postgres_execution_sqlstates", "semantic_execution", "postgres_nonempty_sqlstates", "semantic_fixtures"]:
        if key in previous.get("counts", {}): report["counts"][key] = previous["counts"][key]
    return report


def source_record(record: dict, observation: dict, features: dict, probe: dict, replay: dict) -> dict:
    """Keep stage-specific evidence and unmet obligations for exactly one source ID."""
    previous = [case for case in replay.get("results", []) if case["source_id"] == record["id"]]
    executed = len(previous) == 3 and all(case["status"] == "ok" for case in previous)
    return {"id": record["id"], "population": record["id"].split(":")[0], "sql": record["sql"],
            "sql_sha256": digest(record["sql"].encode()), "source_url": record["source_url"],
            "source_commit": record.get("commit", record.get("revision")), "source_file": record["source_file"],
            "corpus_file": record["corpus_file"], "corpus_line": record["corpus_line"],
            "template_markers": record.get("template_markers", []), "transformations": record.get("transformations", []),
            "inventory": features, "parser": {"status": observation["outcome"], "diagnostic": observation.get("diagnostic")},
            "postgres_prepare_probe": probe,
            "context": {"status": "synthetic_m1_contract" if executed else "not_configured", "authoritative_ddl": []},
            "execution": {"status": "matched_pg_rust_alloy" if executed else "not_executed", "fixtures": len(previous),
                          "evidence": "validation/results/industrial.json" if executed else None,
                          "binary_sha256": replay.get("cli_sha256") if executed else None},
            "effects": {"status": "read_only_rows_observed" if executed else "not_observed"}}


def build_manifest(records: dict, census: dict, features: dict, probes: dict, runtime: dict) -> dict:
    """Separate baseline replay counts, grammar probes and pending semantic work."""
    replay = json.loads((ROOT / "validation/results/industrial.json").read_text(encoding="utf-8"))
    observations = {row["id"]: row for row in census["observations"]}
    rows = [source_record(record, observations[key], features[key], probes[key], replay) for key, record in records.items()]
    previous = json.loads(MANIFEST.read_text(encoding="utf-8")) if MANIFEST.exists() else {}
    report = {"schema_version": "sqlolla.validation.corpus/1", "status": "incomplete_semantic_coverage",
            "claim": "Per-record stage ledger; preparation, parser acceptance and expected rejection are not semantic execution passes.",
            "parser_commit": census["parser_commit"], "corpus_files": census["corpus_files"], "records": len(rows),
            "distinct_sql_texts": len({row["sql_sha256"] for row in rows}), "runtime": runtime,
            "counts": {"parser": dict(Counter(row["parser"]["status"] for row in rows)),
                       "postgres_probe": dict(Counter(row["postgres_prepare_probe"]["status"] for row in rows)),
                       "postgres_sqlstates": dict(Counter(row["postgres_prepare_probe"]["sqlstate"] for row in rows)),
                       "execution": dict(Counter(row["execution"]["status"] for row in rows)),
                       "features": dict(Counter(feature for row in rows for feature in row["inventory"]["features"]))},
            "context_sources": previous.get("context_sources", []), "cases": rows}
    return preserve_progress(report, previous)


def main() -> None:
    """Probe all pinned records in disposable PostgreSQL and save their exact scope."""
    records, census = load_sources(ROOT.parent / "dependencies/SQLeanParser")
    features = {key: inventory(record["sql"]) for key, record in records.items()}
    with TemporaryPostgres() as database:
        runtime = {"version": database.execute("SELECT version();").strip(), "schema": "empty public plus PostgreSQL system catalogs",
                   "business_dml_executed": False, "ddl_rollback": "PL/pgSQL exception subtransaction"}
        probes, directory = probe_records(database, records, features), database.directory
    runtime["cluster_removed"] = not directory.exists()
    report = build_manifest(records, census, features, probes, runtime)
    write_json(MANIFEST, report)
    print(json.dumps(report["counts"]))


if __name__ == "__main__":
    main()

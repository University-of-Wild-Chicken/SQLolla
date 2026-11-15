"""Load pinned application schemas and record actual PostgreSQL type resolution."""
from collections import Counter
import json

from corpus_features import inventory
from corpus_probe import probe_records
from industrial import load_sources
from pg_runtime import ROOT, TemporaryPostgres
from run import digest, write_json

MANIFEST = ROOT / "validation/corpus-manifest.json"
CATALOG_SQL = """SELECT json_build_object('table',c.relname,'column',a.attname,
 'position',a.attnum,'type',format_type(a.atttypid,a.atttypmod),'nullable',NOT a.attnotnull,
 'default',pg_get_expr(d.adbin,d.adrelid)) FROM pg_attribute a
 JOIN pg_class c ON c.oid=a.attrelid JOIN pg_namespace n ON n.oid=c.relnamespace
 LEFT JOIN pg_attrdef d ON d.adrelid=a.attrelid AND d.adnum=a.attnum
 WHERE n.nspname='public' AND c.relkind IN ('r','v') AND a.attnum>0 AND NOT a.attisdropped
 ORDER BY c.relname,a.attnum;"""


def group(identifier: str) -> str:
    """Separate benchmark schemas whose authentic table names overlap."""
    parts = identifier.split(":")
    return ":".join(parts[:2]) if parts[0] == "benchbase" else parts[0]


def context_files(manifest: dict, key: str) -> list[dict]:
    """Select verified authoritative DDL in its upstream migration order."""
    population = key.split(":")[0]
    sources = [item for item in manifest["context_sources"] if item["population"] == population and item["kind"] == "authoritative_upstream_ddl"]
    if population == "benchbase": sources = [item for item in sources if "/" + key.split(":")[1] + "/" in item["upstream_path"]]
    return sorted(sources, key=lambda item: item["upstream_path"])


def load_context(database, files: list[dict], key: str, records: dict) -> list[dict]:
    """Load fixture DDL, documenting River's explicit empty-schema template expansion."""
    database.execute("DROP SCHEMA public CASCADE; CREATE SCHEMA public;")
    loaded = []
    for source in files:
        raw = (ROOT / source["path"]).read_bytes()
        if digest(raw) != source["sha256"]: raise ValueError("Authoritative DDL bytes changed")
        sql = raw.decode("utf-8")
        if key == "river": sql = sql.replace("/* TEMPLATE: schema */", "")
        database.execute(sql)
        loaded.append({"path": source["path"], "source_sha256": source["sha256"],
                       "loaded_sha256": digest(sql.encode()), "transformation": "River DDL schema-template empty expansion" if key == "river" else "none"})
    if key == "temporal":
        for record in records.values():
            if record["id"].split(":")[-1] in {"createSchemaVersionTableQuery", "createSchemaUpdateHistoryTableQuery"}:
                database.execute(record["sql"])
                loaded.append({"source_id": record["id"], "loaded_sha256": digest(record["sql"].encode()), "transformation": "none"})
    return loaded


def inspect_group(database, key: str, records: dict, manifest: dict) -> dict:
    """Capture typed preparation results plus the authentic empty application catalog."""
    loaded = load_context(database, context_files(manifest, key), key, records)
    catalog = [json.loads(line) for line in database.execute(CATALOG_SQL).splitlines()]
    features = {identifier: inventory(record["sql"]) for identifier, record in records.items()}
    probes = probe_records(database, records, features)
    return {"group": key, "status": "loaded_and_probed", "loaded_sources": loaded, "catalog": catalog,
            "session": {"search_path": "public", "timezone": "UTC", "collation": "C", "template_query_rewrite": False},
            "probes": probes}


def apply_results(manifest: dict, contexts: list[dict], runtime: dict) -> None:
    """Attach authoritative type context while preserving the original M1 replay evidence."""
    indexed = {context["group"]: context for context in contexts}
    for case in manifest["cases"]:
        context = indexed[group(case["id"])]
        case["postgres_authoritative_prepare"] = context["probes"][case["id"]]
        case["context"]["authoritative_status"] = "loaded_empty_application_schema"
        case["context"]["authoritative_group"] = context["group"]
        case["context"]["authoritative_ddl"] = context["loaded_sources"]
    manifest["authoritative_contexts"] = [{key: value for key, value in context.items() if key != "probes"} for context in contexts]
    manifest["authoritative_probe_runtime"] = runtime
    manifest["counts"]["authoritative_prepare"] = dict(Counter(case["postgres_authoritative_prepare"]["status"] for case in manifest["cases"]))
    manifest["counts"]["authoritative_sqlstates"] = dict(Counter(case["postgres_authoritative_prepare"]["sqlstate"] for case in manifest["cases"]))


def main() -> None:
    """Resolve all unchanged SQL in fresh authoritative contexts without executing DML."""
    manifest = json.loads(MANIFEST.read_text(encoding="utf-8"))
    records, _ = load_sources(ROOT.parent / "dependencies/SQLeanParser")
    contexts = []
    with TemporaryPostgres() as database:
        runtime = {"version": database.execute("SELECT version();").strip(), "business_dml_executed": False}
        for key in sorted({group(identifier) for identifier in records}):
            selected = {identifier: record for identifier, record in records.items() if group(identifier) == key}
            contexts.append(inspect_group(database, key, selected, manifest))
            print(key, dict(Counter(probe["status"] for probe in contexts[-1]["probes"].values())), flush=True)
        directory = database.directory
    runtime["cluster_removed"] = not directory.exists()
    apply_results(manifest, contexts, runtime)
    write_json(MANIFEST, manifest)


if __name__ == "__main__":
    main()

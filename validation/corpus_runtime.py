"""Execute every industrial record in explicit PostgreSQL contexts; never infer core coverage."""
from collections import Counter
import argparse
import json
import re

from corpus_features import inventory
from corpus_schema import context_files, group, load_context
from corpus_seed import seed
import corpus_locks
import corpus_ddl
import corpus_sequences
import corpus_session_effects
import corpus_backend_effects
import corpus_tuples
import system_context
from industrial import load_sources
from libpq_client import PgConnection
from oracle import literal, quote_identifier
from pg_runtime import ROOT, TemporaryPostgres
from run import digest, write_json

MANIFEST = ROOT / "validation/corpus-manifest.json"


def parameter_value(ty: str) -> str:
    """Choose a declared synthetic value without coercing authoritative SQL types."""
    if ty.endswith("[]"): return "{}"
    if ty in {"integer", "bigint", "smallint", "numeric", "double precision"}: return "1"
    if ty == "boolean": return "true"
    if ty == "date": return "1996-01-01"
    if ty.startswith("timestamp"): return "2000-01-01 00:00:00+00"
    if ty == "bytea": return "\\x01"
    if ty == "jsonb": return "{}"
    if ty == "river_job_state": return "available"
    return "seed"


def query_context(case: dict, creator: str) -> dict:
    """Declare the five source-backed driver contexts and preserve both SQL hashes."""
    identifier, sql = case["id"], case["sql"]
    types = case["postgres_authoritative_prepare"].get("inferred_parameter_types", []) or []
    prerequisite, transformations = [], []
    view = re.sub(r"\$[12]\b", "'1996-01-01'", creator)
    if identifier == "benchbase:tpch:Q1:query_stmt": types = ["text"]
    if identifier == "benchbase:tpch:Q15:createview_stmt":
        sql, types = view, []
        transformations.append("Both date placeholders instantiated to 1996-01-01, matching Q15.java Statement substitution")
    if identifier in {"benchbase:tpch:Q15:query_stmt", "benchbase:tpch:Q15:dropview_stmt"}: prerequisite = [view]
    if identifier.endswith(":jobInsertFastMany"):
        sql = sql.replace("/* TEMPLATE: schema */", "")
        types = ["bigint[]", "jsonb[]", "timestamp with time zone[]", "text[]", "smallint[]", "jsonb[]", "smallint[]", "text[]", "timestamp with time zone[]", "text[]", "text[]", "bytea[]", "integer[]"]
        transformations.append("River sqlctemplate schema replacement is empty/public, including inside regclass string; duplicate-expression fallback retained")
    parameters = [{"type": ty, "value": parameter_value(ty)} for ty in types]
    if identifier == "benchbase:tpch:Q1:query_stmt": parameters[0]["value"] = "90"
    if identifier.endswith(":terminateDatabaseConnectionsQuery"): parameters[0]["value"] = "sqlolla_no_matching_database"
    return {"sql": sql, "original_sql_sha256": case["sql_sha256"] if "sql_sha256" in case else digest(case["sql"].encode()),
            "instantiated_sql_sha256": digest(sql.encode()), "transformations": transformations,
            "prerequisite_sql": prerequisite, "parameters": parameters, "fixture": "authoritative schema with upstream seed data; synthetic nonnull scalar parameters and empty arrays"}


def checked(connection, sql: str) -> dict:
    """Fail fixture setup visibly rather than treating setup errors as query behavior."""
    result = connection.query(sql)
    if result["status"] != "ok": raise ValueError({"setup_sql": sql, "result": result})
    return result


def parameters(connection, context: dict) -> list[dict]:
    """Resolve each explicit PostgreSQL parameter type to its actual local OID."""
    result = []
    for parameter in context["parameters"]:
        oid = checked(connection, "SELECT " + literal(parameter["type"]) + "::regtype::oid")["rows"][0][0]
        result.append(dict(parameter, oid=int(oid)))
    return result


def describe(connection, result: dict) -> dict:
    """Attach stable PostgreSQL type names so process-local OIDs are not portable evidence."""
    for column in result.get("columns", []):
        oid = column["type_oid"]
        if oid not in connection.type_names:
            connection.type_names[oid] = checked(connection, f"SELECT format_type({oid},NULL)")["rows"][0][0]
        column["type"] = connection.type_names[oid]
    canonical_regclass(connection, result)
    return result


def canonical_regclass(connection, result: dict) -> None:
    """Preserve original display while observing the numeric identity used by typed regclass transport."""
    indices = [index for index, column in enumerate(result.get("columns", [])) if column["type"] == "regclass"]
    if not indices or "rows" not in result: return
    result["postgres_text_rows"] = [row.copy() for row in result["rows"]]
    for row in result["rows"]:
        for index in indices:
            if row[index] is not None:
                row[index] = checked(connection, "SELECT " + literal(row[index]) + "::regclass::oid")["rows"][0][0]
    result["regclass_transport"] = "Canonical unsigned OID identity; original PostgreSQL display retained in postgres_text_rows."


def snapshot(connection) -> dict:
    """Record every public relation and sequence, including nontransactional sequence state."""
    relations = checked(connection, "SELECT c.relname,c.relkind FROM pg_class c JOIN pg_namespace n ON n.oid=c.relnamespace WHERE n.nspname='public' AND c.relkind IN ('r','S') ORDER BY c.relname")["rows"]
    tables, sequences = {}, {}
    for name, kind in relations:
        result = describe(connection, checked(connection, "SELECT * FROM " + quote_identifier(name)))
        result["rows"].sort(key=lambda row: json.dumps(row, ensure_ascii=False))
        if kind == "S": sequences[name] = result
        else: tables[name] = {"kind": kind, "columns": result["columns"], "rows": result["rows"]}
    locks = checked(connection, "SELECT classid::text,objid::text,objsubid::text,mode,granted::text FROM pg_locks WHERE pid=pg_backend_pid() AND locktype='advisory' ORDER BY classid,objid,objsubid,mode")["rows"]
    return {"database": tables, "sequences": sequences, "advisory_locks": locks,
            "sequence_context": corpus_sequences.capture(connection, sequences),
            "relation_lookup": corpus_sequences.relation_lookup(connection),
            "catalog": corpus_ddl.capture(connection, {})}


def restore_sequences(connection, state: dict) -> None:
    """Reset owned fixture sequences after observing that rollback does not reset nextval."""
    for name, record in state["sequences"].items():
        values = dict(zip([column["name"] for column in record["columns"]], record["rows"][0]))
        checked(connection, "SELECT setval(" + literal(name) + "," + values["last_value"] + "," + ("true" if values["is_called"] == "t" else "false") + ")")


def execute_case(database, connection, case: dict, context: dict, observer, tuple_tables: dict) -> dict:
    """Run one source query and observe rows, SQLSTATE, relation effects and rollback."""
    initial = snapshot(connection)
    checked(connection, "BEGIN")
    for sql in context["prerequisite_sql"]: checked(connection, sql)
    before = snapshot(connection)
    session = corpus_session_effects.prepare(database, connection, observer, case, context, before)
    observation = observe_statement(connection, observer, case, context, before, tuple_tables)
    result, after = observation["result"], observation["after"]
    rolled_back = rollback_observations(connection, observer, session, initial, before, observation)
    if observation["tuple_observations"] is not None:
        observation["tuple_observations"]["after_rollback"] = corpus_tuples.observe(connection, tuple_tables)
    return dict(observation, status="postgres_observed_core_not_compared", context=context,
            before=before, after_rollback=rolled_back, session_effects=session,
            query_sql_unchanged=context["original_sql_sha256"] == context["instantiated_sql_sha256"],
            rollback_restored_database=initial["database"] == rolled_back["database"],
            sequence_state_changed=initial["sequences"] != rolled_back["sequences"],
            unobserved_effects=[effect for effect in case["inventory"]["effect_obligations"] if effect != "pg_advisory_xact_lock"])


def observe_statement(connection, observer, case: dict, context: dict, before: dict, tuple_tables: dict) -> dict:
    """Capture statement inputs before execution and independently observe each requested effect family."""
    physical = corpus_locks.identities(connection, before) if corpus_locks.applies(case) else None
    typed = parameters(connection, context)
    clock = checked(connection, "SELECT transaction_timestamp()::text")["rows"][0][0]
    system = system_context.before(connection, case, context["sql"])
    backends = corpus_backend_effects.prepare(connection, case, context, system)
    tuples = prepare_tuples(connection, tuple_tables, before)
    if tuples is None: checked(connection, "SAVEPOINT observed_statement")
    result = connection.query(context["sql"], typed)
    rolled_back = result["status"] == "error" and tuples is not None
    if result["status"] == "error": checked(connection, "ROLLBACK" if rolled_back else "ROLLBACK TO SAVEPOINT observed_statement")
    result = describe(connection, result)
    after = snapshot(connection)
    system_context.after(connection, system, context["sql"])
    corpus_backend_effects.finish(backends, system, result)
    if tuples is not None: tuples["after"] = corpus_tuples.observe(connection, tuple_tables)
    locks = corpus_locks.observe(connection, observer, physical) if physical is not None else None
    return {"transaction_timestamp": clock, "result": result, "after": after,
            "row_locks": locks, "system_context": system, "backend_effects": backends,
            "tuple_observations": tuples, "statement_transaction_aborted": rolled_back,
            "statement_envelope": "top_level_no_savepoints" if tuples is not None else "statement_savepoint"}


def rollback_observations(connection, observer, session, initial: dict, before: dict, observation: dict) -> dict:
    """Observe rollback independently before restoring task-owned nontransactional sequence state."""
    corpus_session_effects.before_end(observer, session)
    if not observation["statement_transaction_aborted"]: checked(connection, "ROLLBACK")
    rolled_back = snapshot(connection)
    corpus_session_effects.finish(observer, session, before, observation["after"], rolled_back, observation["result"])
    restore_sequences(connection, initial)
    return rolled_back


def prepare_tuples(connection, tables: dict, before: dict) -> dict | None:
    """Allocate a top-level writer before the query and align independently captured input headers."""
    if not tables: return None
    xid = int(checked(connection, "SELECT pg_current_xact_id()::text")["rows"][0][0])
    captured = corpus_tuples.align(corpus_tuples.capture(connection, tables, xid), before["database"])
    return {"catalog_tables": tables, "before": captured}


def pool_snapshots(manifest: dict) -> None:
    """Deduplicate complete evidence by content hash inside the one maintained ledger."""
    relations, states = manifest.get("observation_relations", {}), manifest.get("observation_states", {})
    for case in manifest["cases"]:
        observations = [case["postgres_execution"], *case.get("postgres_fixtures", {}).values()]
        for evidence in observations:
            pool_evidence(evidence, relations, states)
    manifest["observation_relations"], manifest["observation_states"] = relations, states
    prune_snapshots(manifest)


def prune_snapshots(manifest: dict) -> None:
    """Retain exactly the pooled states and relations reachable from maintained fixture evidence."""
    references = set()
    for case in manifest["cases"]:
        for evidence in [case["postgres_execution"], *case.get("postgres_fixtures", {}).values()]:
            references.update(evidence[stage + "_state_sha256"] for stage in ("before", "after", "after_rollback"))
    states = {key: manifest["observation_states"][key] for key in references}
    relations = {key for state in states.values() for key in state["database"].values()}
    manifest["observation_states"] = states
    manifest["observation_relations"] = {key: manifest["observation_relations"][key] for key in relations}


def pool_evidence(evidence: dict, relations: dict, states: dict) -> None:
    """Pool newly observed complete states while leaving prior evidence references intact."""
    for key in ["before", "after", "after_rollback"]:
            if key not in evidence: continue
            state = evidence.pop(key)
            for name, relation in state["database"].items():
                fingerprint = digest(json.dumps(relation, sort_keys=True, ensure_ascii=False).encode())
                relations[fingerprint] = relation
                state["database"][name] = fingerprint
            fingerprint = digest(json.dumps(state, sort_keys=True, ensure_ascii=False).encode())
            states[fingerprint] = state
            evidence[key + "_state_sha256"] = fingerprint


def run_group(database, key: str, cases: list, records: dict, manifest: dict, creator: str, fixture: str) -> None:
    """Execute a complete source population with strict fixture/session isolation."""
    load_context(database, context_files(manifest, key), key, records)
    with PgConnection(database) as connection, PgConnection(database) as observer:
        for statement in ["SET timezone='UTC'", "SET statement_timeout='2s'", "SET lock_timeout='500ms'", "SET standard_conforming_strings=on"]: checked(connection, statement)
        if fixture == "nonempty": manifest.setdefault("seed_fixtures", {})[key] = seed(connection)
        for case, context, tuples in ordered_cases(cases, manifest, key, creator, fixture):
            observation = execute_case(database, connection, case, context, observer, tuples)
            invalidate_semantics(manifest, case)
            if fixture == "nonempty": case.setdefault("postgres_fixtures", {})[fixture] = observation
            else: case["postgres_execution"] = observation
    observations = [case["postgres_execution"] if fixture == "baseline" else case["postgres_fixtures"][fixture] for case in cases]
    print(key, dict(Counter(observation["result"]["sqlstate"] for observation in observations)), flush=True)


def ordered_cases(cases: list, manifest: dict, key: str, creator: str, fixture: str) -> list:
    """Observe pristine tuple contexts before unrelated rollback/probe locks can leave raw header remnants."""
    group_context = next(value for value in manifest["authoritative_contexts"] if value["group"] == key)
    tables = group_context["semantic_context"]["catalog"]["tables"]
    result = []
    for case in cases:
        context = fixture_context(case, creator, fixture)
        result.append((case, context, corpus_tuples.requested_tables(context["sql"], tables)))
    return sorted(result, key=lambda item: not bool(item[2]))


def fixture_context(case: dict, creator: str, fixture: str) -> dict:
    """Declare positive notification inputs without deriving any expected effect from the core."""
    context = query_context(case, creator)
    if fixture == "nonempty":
        context["fixture"] = "authoritative schema, synthetic FK-ordered nonempty rows, source literal defaults, nonnull scalar parameters, empty arrays unless explicitly declared"
        if case["id"].endswith(":pGNotifyMany"):
            context["parameters"][2]["value"] = "{one,two,one}"
            context["fixture"] += "; notification payloads one,two,one exercise commit duplicate folding"
    return context


def invalidate_semantics(manifest: dict, case: dict, reason: str = "stale_oracle_replaced") -> None:
    """Changed oracle evidence invalidates all derived comparisons and aggregate coverage claims."""
    case.pop("semantic_fixtures", None)
    case["semantic_execution"] = {"status": reason, "fixtures": 0}
    manifest["counts"]["semantic_execution"] = dict(Counter(
        item.get("semantic_execution", {}).get("status", "not_compared") for item in manifest["cases"]))
    manifest["counts"].pop("semantic_fixtures", None)
    manifest.pop("semantic_coverage_target", None)


def main() -> None:
    """Update one complete PostgreSQL execution ledger without crediting unsupported core paths."""
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--fixture", choices=["baseline", "nonempty"], default="baseline")
    parser.add_argument("--only-system", action="store_true", help="Refresh declared system contexts without replacing unrelated fixture evidence")
    args = parser.parse_args()
    fixture = args.fixture
    manifest = json.loads(MANIFEST.read_text(encoding="utf-8"))
    records, _ = load_sources(ROOT.parent / "dependencies/SQLeanParser")
    creator = records["benchbase:tpch:Q15:createview_stmt"]["sql"]
    with TemporaryPostgres() as database:
        for key in sorted({group(identifier) for identifier in records}):
            cases = [case for case in manifest["cases"] if group(case["id"]) == key]
            if args.only_system: cases = [case for case in cases if system_context.applies(case)]
            if not cases: continue
            selected = {identifier: record for identifier, record in records.items() if group(identifier) == key}
            run_group(database, key, cases, selected, manifest, creator, fixture)
        directory = database.directory
    manifest["counts"]["postgres_execution_sqlstates"] = dict(Counter(case["postgres_execution"]["result"]["sqlstate"] for case in manifest["cases"]))
    manifest["postgres_execution_scope"] = {"cases": len(manifest["cases"]), "cluster_removed": not directory.exists(),
        "fixture_count_per_case": 1 + min(len(case.get("postgres_fixtures", {})) for case in manifest["cases"]),
        "claim": "Actual PostgreSQL execution only; Rust outcome membership and SAT selection remain separate."}
    if fixture == "nonempty": manifest["counts"]["postgres_nonempty_sqlstates"] = dict(Counter(case["postgres_fixtures"][fixture]["result"]["sqlstate"] for case in manifest["cases"]))
    pool_snapshots(manifest)
    write_json(MANIFEST, manifest)


if __name__ == "__main__":
    main()

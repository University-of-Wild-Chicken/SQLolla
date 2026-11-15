"""Translate independently inspected PostgreSQL catalog facts into typed test requests."""
import json
import re
from copy import deepcopy
import corpus_ddl
import corpus_functions
import corpus_sequences
import corpus_session_effects
import corpus_backend_effects

from corpus_schema import CATALOG_SQL, context_files, group, load_context
from industrial import load_sources
from pg_runtime import ROOT, TemporaryPostgres
from run import digest, write_json

MANIFEST = ROOT / "validation/corpus-manifest.json"
ENUM_SQL = """SELECT json_build_object('name',t.typname,'labels',json_agg(e.enumlabel ORDER BY e.enumsortorder))
 FROM pg_type t JOIN pg_enum e ON e.enumtypid=t.oid JOIN pg_namespace n ON n.oid=t.typnamespace
 WHERE n.nspname='public' GROUP BY t.typname ORDER BY t.typname;"""
TABLE_SQL = """SELECT json_build_object('name',c.relname,'row_security',c.relrowsecurity,
 'has_triggers',EXISTS(SELECT 1 FROM pg_trigger t WHERE t.tgrelid=c.oid AND NOT t.tgisinternal),
 'has_inheritance_or_partitioning',c.relkind='p' OR c.relispartition OR EXISTS(SELECT 1 FROM pg_inherits i WHERE i.inhrelid=c.oid OR i.inhparent=c.oid))
 FROM pg_class c JOIN pg_namespace n ON n.oid=c.relnamespace WHERE n.nspname='public' AND c.relkind IN ('r','p') ORDER BY c.relname;"""
CONSTRAINT_SQL = """SELECT json_build_object('table',c.relname,'name',k.conname,'kind',k.contype,
 'columns',k.conkey,'referenced_table',r.relname,'referenced_columns',k.confkey,
 'deferrable',k.condeferrable,'initially_deferred',k.condeferred,
 'on_update',k.confupdtype,'on_delete',k.confdeltype,'match',k.confmatchtype,
 'sql',pg_get_expr(k.conbin,k.conrelid),'nulls_not_distinct',COALESCE(i.indnullsnotdistinct,false),
 'insert_trigger_name',(SELECT min(t.tgname) FROM pg_trigger t WHERE t.tgconstraint=k.oid AND t.tgrelid=k.conrelid AND (t.tgtype&4)=4))
 FROM pg_constraint k JOIN pg_class c ON c.oid=k.conrelid JOIN pg_namespace n ON n.oid=c.relnamespace
 LEFT JOIN pg_class r ON r.oid=k.confrelid LEFT JOIN pg_index i ON i.indexrelid=k.conindid
 WHERE n.nspname='public' ORDER BY c.relname,COALESCE((SELECT min(t.tgname) FROM pg_trigger t
 WHERE t.tgconstraint=k.oid AND t.tgrelid=k.conrelid AND (t.tgtype&4)=4),k.oid::text);"""
INDEX_SQL = """SELECT json_build_object('table',c.relname,'name',ic.relname,
 'columns',ARRAY(SELECT k FROM unnest(i.indkey) WITH ORDINALITY a(k,p) WHERE p<=i.indnkeyatts ORDER BY p),
 'predicate',pg_get_expr(i.indpred,i.indrelid),'definition',pg_get_indexdef(i.indexrelid),
 'nulls_not_distinct',i.indnullsnotdistinct,'valid',i.indisvalid AND i.indisready AND i.indislive,
 'immediate',i.indimmediate,'btree',am.amname='btree','expressions',i.indexprs IS NOT NULL,
 'default_operators',NOT EXISTS(SELECT 1 FROM unnest(i.indclass) o JOIN pg_opclass p ON p.oid=o WHERE NOT p.opcdefault),
 'column_collations',NOT EXISTS(SELECT 1 FROM unnest(i.indcollation) WITH ORDINALITY q(oid,p)
 JOIN unnest(i.indkey) WITH ORDINALITY k(attnum,p) USING(p) JOIN pg_attribute a ON a.attrelid=c.oid AND a.attnum=k.attnum
 WHERE q.p<=i.indnkeyatts AND q.oid<>a.attcollation))
 FROM pg_index i JOIN pg_class c ON c.oid=i.indrelid JOIN pg_namespace n ON n.oid=c.relnamespace
 JOIN pg_class ic ON ic.oid=i.indexrelid JOIN pg_am am ON am.oid=ic.relam
 WHERE n.nspname='public' AND i.indisunique AND NOT EXISTS
 (SELECT 1 FROM pg_constraint k WHERE k.conindid=i.indexrelid AND k.contype IN ('p','u')) ORDER BY c.relname,ic.relname;"""
INDEX_ORDER_SQL = """SELECT json_build_object('table',c.relname,
 'names',json_agg(COALESCE(k.conname,ic.relname) ORDER BY i.indexrelid))
 FROM pg_index i JOIN pg_class c ON c.oid=i.indrelid JOIN pg_namespace n ON n.oid=c.relnamespace
 JOIN pg_class ic ON ic.oid=i.indexrelid LEFT JOIN pg_constraint k ON k.conindid=i.indexrelid AND k.contype IN ('p','u')
 WHERE n.nspname='public' AND i.indisunique GROUP BY c.relname ORDER BY c.relname;"""


def sql_type(name: str, enums: dict) -> object:
    """Keep PostgreSQL families, nominal enums and typmods instead of coercing to int32/text."""
    if name.endswith("[]"): return {"array": sql_type(name[:-2], enums)}
    simple = {"integer": "int32", "bigint": "int64", "smallint": "int16", "boolean": "bool", "text": "text",
              "double precision": "float64", "bytea": "bytea", "date": "date", "timestamp without time zone": "timestamp",
              "timestamp with time zone": "timestamptz", "interval": "interval", "jsonb": "jsonb", "json": "json",
              "name": "name", "character": "bpchar", "oid": "oid", "xid": "xid", "regclass": "regclass", '"char"': "internal_char", "void": "void"}
    if name in simple: return simple[name]
    if name in enums: return {"enum": {"name": "public." + name, "labels": enums[name]}}
    match = re.fullmatch(r"numeric(?:\((\d+)(?:,(-?\d+))?\))?", name)
    if match: return {"numeric": {"precision": int(match[1]) if match[1] else None, "scale": int(match[2] or 0) if match[1] else None}}
    match = re.fullmatch(r"character varying(?:\((\d+)\))?", name)
    if match: return {"varchar": {"length": int(match[1]) if match[1] else None}}
    match = re.fullmatch(r"character(?:\((\d+)\))?", name)
    if match: return {"char": {"length": int(match[1] or 1)}}
    match = re.fullmatch(r"bit(?:\((\d+)\))?", name)
    if match: return {"bit": {"length": int(match[1] or 1)}}
    raise ValueError("Unmodeled authoritative PostgreSQL type: " + name)


def cell(value: str | None, ty: object) -> dict:
    """Retain native scalar identity and exact text transport for richer values."""
    if value is None: return {"type": "null", "value": ty}
    if ty in ("int32", "int64"): return {"type": ty, "value": int(value)}
    if ty == "bool":
        if value not in ("t", "true", "f", "false"): raise ValueError("Invalid PostgreSQL boolean transport")
        return {"type": "bool", "value": value in ("t", "true")}
    if ty == "text": return {"type": "text", "value": value}
    return {"type": "scalar", "value": {"ty": ty, "value": value}}


def initial_metadata(table: dict) -> dict:
    """Start from observed table flags; explicit constraint/index inspection completes admission."""
    return {"complete": True, "defaults": {}, "primary_key": None,
            "unique": [], "checks": [], "foreign_keys": [], "has_triggers": table["has_triggers"],
            "row_security": table["row_security"], "has_inheritance_or_partitioning": table["has_inheritance_or_partitioning"]}


def unique_indexes(metadata: dict, observations: list[dict]) -> None:
    """Admit only observed default-column btree indexes; retain partial predicate SQL for parsing."""
    for index in observations:
        target = metadata["public." + index["table"]]
        supported = all(index[name] for name in ("valid", "immediate", "btree", "default_operators", "column_collations"))
        supported &= not index["expressions"] and all(position > 0 for position in index["columns"])
        if not supported:
            target["complete"] = False
            continue
        key = {"name": index["name"], "columns": [position - 1 for position in index["columns"]],
               "nulls_not_distinct": index["nulls_not_distinct"]}
        predicate = None if index["predicate"] is None else {"name": index["name"], "sql": index["predicate"], "parser": None}
        target.setdefault("unique_indexes", []).append({"key": key, "predicate": predicate})


def unique_order(database, metadata: dict) -> None:
    """Preserve PostgreSQL's ascending index-OID enforcement order across all unique key kinds."""
    for observed in read_json(database, INDEX_ORDER_SQL):
        metadata["public." + observed["table"]]["unique_enforcement_order"] = observed["names"]


def constraint(metadata: dict, item: dict) -> None:
    """Preserve enforcement timing, named keys and actions; unsupported constraints block writes."""
    target = metadata["public." + item["table"]]
    key = {"name": item["name"], "columns": [i - 1 for i in item["columns"] or []],
           "deferrable": item["deferrable"], "initially_deferred": item["initially_deferred"]}
    if item["kind"] in ("p", "u"):
        key["nulls_not_distinct"] = item["nulls_not_distinct"]
        if item["kind"] == "p": target["primary_key"] = key
        else: target["unique"].append(key)
    elif item["kind"] == "c": target["checks"].append({"name": item["name"], "sql": item["sql"], "parser": None})
    elif item["kind"] == "f":
        actions = {"a": "no_action", "r": "restrict", "c": "cascade", "n": "set_null", "d": "set_default"}
        key.update(referenced_table="public." + item["referenced_table"], referenced_columns=[i - 1 for i in item["referenced_columns"]],
                   on_update=actions[item["on_update"]], on_delete=actions[item["on_delete"]])
        target["foreign_keys"].append(key)
        if item["match"] != "s": target["complete"] = False
    else: target["complete"] = False


def inspect(database) -> dict:
    """Generate typed metadata only from actual pinned-schema PostgreSQL catalog observations."""
    enums = {item["name"]: item["labels"] for item in read_json(database, ENUM_SQL)}
    metadata = {"public." + item["name"]: initial_metadata(item) for item in read_json(database, TABLE_SQL)}
    tables = {}
    for column in read_json(database, CATALOG_SQL):
        name, index = "public." + column["table"], column["position"] - 1
        target = tables.setdefault(name, {"schema": "public", "name": column["table"], "columns": []})
        target["columns"].append({"name": column["column"], "ty": sql_type(column["type"], enums), "nullable": column["nullable"]})
        if column["default"] is not None:
            metadata[name]["defaults"][str(index)] = {"kind": "expression", "sql": column["default"], "parser": None}
    constraints = read_json(database, CONSTRAINT_SQL)
    for item in constraints: constraint(metadata, item)
    indexes = read_json(database, INDEX_SQL)
    unique_indexes(metadata, indexes)
    unique_order(database, metadata)
    functions = read_json(database, corpus_functions.FUNCTION_SQL)
    catalog = {"search_path": ["public"], "tables": tables}
    definitions = corpus_functions.declared(functions, enums, sql_type)
    if definitions is not None: catalog["functions"] = definitions
    return {"catalog": catalog, "table_metadata": metadata, "function_observations": functions,
            "enums": enums, "constraint_observations": constraints, "index_observations": indexes,
            "source": "PostgreSQL catalog over verified upstream DDL"}


def read_json(database, sql: str) -> list[dict]:
    """Require complete line-delimited JSON from the owned oracle cluster."""
    return [json.loads(line) for line in database.execute(sql).splitlines()]


def state(manifest: dict, evidence: dict, stage: str) -> dict:
    """Resolve content-addressed fixture states without dropping unused tables or sequences."""
    snapshot = verified_payload(manifest["observation_states"], evidence[stage + "_state_sha256"])
    return dict(snapshot, database={name: verified_payload(manifest["observation_relations"], key) for name, key in snapshot["database"].items()})


def verified_payload(pool: dict, key: str) -> dict:
    """Reject edited content-addressed evidence instead of trusting its dictionary key."""
    value = pool[key]
    if digest(json.dumps(value, sort_keys=True, ensure_ascii=False).encode()) != key:
        raise ValueError("Oracle snapshot content hash mismatch")
    return value


def typed_rows(rows: list, types: list) -> list:
    """Require exact row width before converting lossless PostgreSQL cell transport."""
    if any(not isinstance(row, list) or len(row) != len(types) for row in rows):
        raise ValueError("Oracle row width differs from declared schema")
    return [[cell(value, ty) for value, ty in zip(row, types)] for row in rows]


def request(manifest: dict, case: dict, context: dict, evidence: dict | None = None) -> dict:
    """Build a full authentic context; unresolved view or system-catalog contexts remain visible."""
    evidence = evidence or case["postgres_execution"]
    if digest(evidence["context"]["sql"].encode()) != evidence["context"]["instantiated_sql_sha256"]:
        raise ValueError("Instantiated source SQL hash mismatch")
    before = state(manifest, evidence, "before")
    catalog = context["semantic_context"]["catalog"]
    if set(before["database"]) != {table["name"] for table in catalog["tables"].values()}:
        raise ValueError("Oracle relations differ from the complete semantic catalog")
    database = {}
    for name, table in catalog["tables"].items():
        relation = before["database"][table["name"]]
        if [column["name"] for column in relation["columns"]] != [column["name"] for column in table["columns"]]:
            raise ValueError("Oracle column names/order differ from the semantic catalog")
        database[name] = typed_rows(relation["rows"], [column["ty"] for column in table["columns"]])
    parameters = {str(i): cell(item["value"], sql_type(item["type"], context["semantic_context"]["enums"]))
                  for i, item in enumerate(evidence["context"]["parameters"], 1)}
    wire = {"id": case["id"], "sql": evidence["context"]["sql"], "catalog": catalog, "database": database, "parameters": parameters,
            "execution": {"max_outcomes": 256, "max_rows": 256, "settings": {
                "timezone": cell("UTC", "text"), "collation": cell("C", "text"),
                "transaction_timestamp": cell(evidence["transaction_timestamp"], "timestamptz")},
                "table_metadata": context["semantic_context"]["table_metadata"],
                "locks": (evidence.get("row_locks") or {}).get("initial"),
                "backend_signals": corpus_backend_effects.execution_context(evidence.get("backend_effects")),
                "tuples": ((evidence.get("tuple_observations") or {}).get("before") or {}).get("context"),
                "session_effects": corpus_session_effects.execution_context(evidence.get("session_effects"))}}
    if "catalog" in before: corpus_ddl.merge_request(wire, before["catalog"])
    return system_request(sequence_request(wire, before), evidence)


def sequence_request(wire: dict, before: dict) -> dict:
    """Bind sequence defaults using exact same-session relation identities and retained sequence state."""
    observed = before.get("relation_lookup")
    if observed is None: return wire
    lookup = corpus_sequences.typed_lookup(observed)
    wire = deepcopy(wire)
    wire["catalog"]["relation_lookup"] = lookup
    wire["catalog"]["search_path"] = lookup["search_path"]
    if wire["catalog"].get("functions") is not None:
        wire["catalog"]["functions"]["search_path"] = lookup["search_path"]
    wire["execution"]["catalog_lookup"] = lookup
    wire["execution"]["sequences"] = before["sequence_context"]
    return wire


def system_request(wire: dict, evidence: dict) -> dict:
    """Add full observed system projections and session settings without mutating shared metadata."""
    system = evidence.get("system_context")
    if system is None: return wire
    wire = deepcopy(wire)
    settings = system["settings"]
    if len(settings["rows"]) != 1: raise ValueError("Expected one complete session observation")
    values = typed_rows(settings["rows"], [sql_type(column["type"], {}) for column in settings["columns"]])[0]
    wire["execution"]["settings"].update({column["name"]: value for column, value in zip(settings["columns"], values)})
    wire["catalog"]["search_path"] = system["effective_search_path"]
    if wire["catalog"].get("functions") is not None:
        wire["catalog"]["functions"]["search_path"] = system["effective_search_path"]
    wire["execution"]["max_rows"] = 4096
    wire["execution"]["catalog_lookup"] = lookup_context(system)
    if wire["catalog"].get("relation_lookup") is not None:
        wire["catalog"]["relation_lookup"] = wire["execution"]["catalog_lookup"]
    for key, result in system["relations_before"].items():
        if key in wire["catalog"]["tables"]: raise ValueError("System projection collides with existing relation")
        schema, name = key.rsplit(".", 1)
        columns = [{"name": column["name"], "ty": sql_type(column["type"], {}), "nullable": True} for column in result["columns"]]
        wire["catalog"]["tables"][key] = {"schema": schema, "name": name, "columns": columns}
        wire["database"][key] = typed_rows(result["rows"], [column["ty"] for column in columns])
        wire["execution"]["table_metadata"][key] = {"complete": False}
    return wire


def lookup_context(system: dict) -> dict:
    """Supply object identities and effective path instead of precomputed function answers."""
    settings = system["settings"]
    names = [column["name"] for column in settings["columns"]]
    if len(names) != len(set(names)): raise ValueError("Duplicate session observation key")
    current_database = settings["rows"][0][names.index("current_database")]
    rows = system["relation_inventory"]["rows"]
    if any(len(row) != 3 for row in rows): raise ValueError("Incomplete relation identity row")
    return {"complete": True, "current_database": current_database, "search_path": system["effective_search_path"],
            "schema_usage": system["schema_usage"],
            "relations": [{"schema": schema, "name": name, "oid": int(oid)} for schema, name, oid in rows]}


def main() -> None:
    """Maintain authoritative transport context beside existing corpus provenance and outcomes."""
    manifest = json.loads(MANIFEST.read_text(encoding="utf-8"))
    records, _ = load_sources(ROOT.parent / "dependencies/SQLeanParser")
    with TemporaryPostgres() as database:
        for context in manifest["authoritative_contexts"]:
            key = context["group"]
            selected = {name: row for name, row in records.items() if group(name) == key}
            load_context(database, context_files(manifest, key), key, selected)
            observed = inspect(database)
            if context.get("semantic_context") != observed:
                invalidate_context(manifest, key)
            context["semantic_context"] = observed
            print(key, "typed catalog and constraints inspected", flush=True)
    write_json(MANIFEST, manifest)


def invalidate_context(manifest: dict, key: str) -> None:
    """Changed binding metadata invalidates derived comparisons before publishing a new catalog."""
    from corpus_runtime import invalidate_semantics
    for case in manifest["cases"]:
        if group(case["id"]) == key:
            invalidate_semantics(manifest, case, "stale_semantic_context_replaced")


if __name__ == "__main__":
    main()

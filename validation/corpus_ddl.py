"""Capture actual PostgreSQL namespaces and stored view definitions without materializing views."""
from copy import deepcopy
import json

NAMESPACE_SQL = """SELECT n.nspname,c.relname,c.relkind FROM pg_namespace n
 LEFT JOIN pg_class c ON c.relnamespace=n.oid ORDER BY n.nspname,c.relname"""
VIEW_SQL = """SELECT n.nspname,c.relname,pg_get_viewdef(c.oid,true),
 ARRAY(SELECT a.attname FROM pg_attribute a WHERE a.attrelid=c.oid AND a.attnum>0
 AND NOT a.attisdropped ORDER BY a.attnum)::text
 FROM pg_class c JOIN pg_namespace n ON n.oid=c.relnamespace
 WHERE c.relkind='v' AND n.nspname !~ '^pg_' AND n.nspname<>'information_schema'
 ORDER BY n.nspname,c.relname"""
DEPENDENCY_SQL = """SELECT DISTINCT n.nspname,c.relname,vn.nspname,v.relname,
 pg_describe_object(d.classid,d.objid,d.objsubid) FROM pg_class c
 JOIN pg_namespace n ON n.oid=c.relnamespace JOIN pg_depend d ON
 ((d.refclassid='pg_class'::regclass AND d.refobjid=c.oid)
 OR (d.refclassid='pg_type'::regclass AND d.refobjid=c.reltype))
 LEFT JOIN pg_rewrite r ON d.classid='pg_rewrite'::regclass AND r.oid=d.objid
 LEFT JOIN pg_class v ON r.ev_class=v.oid AND v.relkind='v'
 LEFT JOIN pg_namespace vn ON vn.oid=v.relnamespace
 WHERE d.deptype='n' AND n.nspname !~ '^pg_' AND n.nspname<>'information_schema'
 AND (v.oid IS NULL OR v.oid<>c.oid) ORDER BY 1,2,3,4,5"""


def rows(connection, sql: str) -> list:
    """Treat catalog acquisition errors as fixture failures, never SQLolla outcomes."""
    result = connection.query(sql)
    if result["status"] != "ok": raise ValueError({"catalog_sql": sql, "result": result})
    return result["rows"]


def capture(connection, context: dict) -> dict:
    """Capture all relation namespace collisions and non-system view dependency evidence."""
    namespace = {"complete": True, "schemas": {}, "views": {}, "dependents": {}}
    for schema, name, kind in rows(connection, NAMESPACE_SQL):
        names = namespace["schemas"].setdefault(schema, [])
        if schema.startswith("pg_") or schema == "information_schema": continue
        if name is not None: names.append(name)
        if kind == "v": namespace["views"].setdefault(schema, []).append(name)
    for schema, name, view_schema, view_name, description in rows(connection, DEPENDENCY_SQL):
        token = "view:" + json.dumps([view_schema, view_name], separators=(",", ":"), ensure_ascii=False) if view_name else "object:" + description
        namespace["dependents"].setdefault(schema, {}).setdefault(name, []).append(token)
    for names in namespace["dependents"].values():
        for name, tokens in names.items(): names[name] = sorted(set(tokens))
    search_path = [row[0] for row in rows(connection, "SELECT unnest(current_schemas(true))")]
    views = capture_views(connection, search_path)
    return {"namespace": namespace, "views": views, "effective_search_path": search_path,
            "creation_schema": {"name": rows(connection, "SELECT current_schema()::text")[0][0]},
            "namespace_scope": "Complete non-system relation namespace; system-schema existence only; internal storage objects and OIDs excluded."}


def capture_views(connection, search_path: list) -> dict:
    """Retain PostgreSQL's deparsed definition and exact ordered labels for pinned reparsing."""
    views = {}
    for schema, name, sql, _ in rows(connection, VIEW_SQL):
        escaped_schema, escaped_name = schema.replace("'", "''"), name.replace("'", "''")
        query = "SELECT a.attname FROM pg_attribute a JOIN pg_class c ON c.oid=a.attrelid JOIN pg_namespace n ON n.oid=c.relnamespace WHERE n.nspname='" + escaped_schema + "' AND c.relname='" + escaped_name + "' AND a.attnum>0 AND NOT a.attisdropped ORDER BY a.attnum"
        columns = [row[0] for row in rows(connection, query)]
        views[schema + "." + name] = {"schema": schema, "name": name, "columns": columns,
                                    "source_sql": sql, "search_path": search_path,
                                    "resolved_relations": view_relations(connection, schema, name)}
    return views


def merge_request(wire: dict, evidence: dict) -> None:
    """Add copied namespace/view inputs and keep view rows out of the physical database."""
    catalog = wire["catalog"] = deepcopy(wire["catalog"])
    catalog["namespace"], catalog["views"] = deepcopy(evidence["namespace"]), deepcopy(evidence["views"])
    if "effective_search_path" in evidence:
        catalog["search_path"] = deepcopy(evidence["effective_search_path"])
        if catalog.get("functions") is not None:
            catalog["functions"]["search_path"] = deepcopy(catalog["search_path"])
    if "creation_schema" in evidence: catalog["creation_schema"] = deepcopy(evidence["creation_schema"])
    identities = {(view["schema"], view["name"]) for view in evidence["views"].values()}
    for key, table in list(catalog["tables"].items()):
        if (table["schema"], table["name"]) in identities:
            del catalog["tables"][key]
            wire["database"].pop(key, None)
            wire.get("execution", {}).get("table_metadata", {}).pop(key, None)


def view_relations(connection, schema: str, name: str) -> list:
    """Retain direct PostgreSQL dependency identities so later search-path shadowing cannot rebind a view."""
    schema, name = schema.replace("'", "''"), name.replace("'", "''")
    query = """SELECT DISTINCT tn.nspname,t.relname FROM pg_class v
 JOIN pg_namespace vn ON vn.oid=v.relnamespace JOIN pg_rewrite r ON r.ev_class=v.oid
 JOIN pg_depend d ON d.classid='pg_rewrite'::regclass AND d.objid=r.oid
 JOIN pg_class t ON d.refclassid='pg_class'::regclass AND t.oid=d.refobjid
 JOIN pg_namespace tn ON tn.oid=t.relnamespace WHERE d.deptype='n' AND t.oid<>v.oid
 AND vn.nspname='%s' AND v.relname='%s' ORDER BY 1,2""" % (schema, name)
    return rows(connection, query)


def matches(actual: dict, before: dict, after: dict, submitted: dict, sql: str) -> bool:
    """Match view catalog changes to independent PG execution and exact submitted source provenance."""
    if canonical(actual.get("namespace")) != canonical(after["namespace"]): return False
    if actual.get("search_path") != submitted["search_path"]: return False
    if actual.get("creation_schema") != submitted.get("creation_schema"): return False
    if normalized_tables(actual.get("tables", {})) != normalized_tables(submitted["tables"]): return False
    if actual.get("table_semantics", {}) != submitted.get("table_semantics", {}): return False
    if normalized_functions(actual.get("functions")) != normalized_functions(submitted.get("functions")): return False
    if not lookup_matches(actual, before, after, submitted): return False
    views = actual.get("views", {})
    if set(views) != set(after["views"]): return False
    for key, view in views.items():
        if key in before["views"]:
            if view != submitted.get("views", {}).get(key): return False
        else:
            expected = dict(after["views"][key], source_sql=sql)
            if view != expected: return False
    return True


def lookup_matches(actual: dict, before: dict, after: dict, submitted: dict) -> bool:
    """Namespace changes explicitly invalidate old OIDs; independently observed no-ops preserve them."""
    changed = canonical(before["namespace"]) != canonical(after["namespace"])
    expected = None if changed else submitted.get("relation_lookup")
    return actual.get("relation_lookup") == expected


def normalized_tables(tables: dict) -> dict:
    """Absent source annotations and explicit null annotations describe the same physical column."""
    return {key: dict(table, columns=[dict(column, qualifier=column.get("qualifier"), schema=column.get("schema"))
                                    for column in table["columns"]]) for key, table in tables.items()}


def normalized_functions(functions: dict | None) -> dict | None:
    """Normalize only optional Column source annotations, preserving bodies, signatures and paths exactly."""
    if functions is None: return None
    definitions = [dict(definition, parameters=[dict(parameter,
        qualifier=parameter.get("qualifier"), schema=parameter.get("schema"))
        for parameter in definition["parameters"]]) for definition in functions["definitions"]]
    return dict(functions, definitions=definitions)


def canonical(value: object) -> object:
    """Namespace arrays are mathematical sets; dictionary order is not an observable catalog effect."""
    if isinstance(value, dict): return {key: canonical(item) for key, item in value.items()}
    if isinstance(value, list): return sorted(canonical(item) for item in value)
    return value

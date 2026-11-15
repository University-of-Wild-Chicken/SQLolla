"""Observe complete role-visible rows of explicitly declared system catalog column projections."""
import re

PROJECTIONS = {"pg_catalog.pg_class": ["oid", "relname", "relnamespace", "relkind"],
               "pg_catalog.pg_namespace": ["oid", "nspname"],
               "information_schema.schemata": ["schema_name"],
               "information_schema.columns": ["table_name", "table_schema", "column_name"],
               "information_schema.tables": ["table_name", "table_schema"],
               "pg_catalog.pg_stat_activity": ["pid", "datname"]}
SESSION_SQL = """SELECT version() AS version, current_schema() AS current_schema,
 current_database() AS current_database, current_user AS current_user,
 pg_backend_pid() AS pg_backend_pid, current_setting('server_version_num') AS \"setting:server_version_num\",
 current_setting('yb_enable_listen_notify',true) AS \"setting:yb_enable_listen_notify\""""


def applies(case: dict) -> bool:
    """Select context-bearing source statements without using expected query outputs."""
    return "system_catalog" in case["inventory"]["features"] or any(name.lower() in {
        "version", "current_setting", "current_schema", "current_database", "pg_backend_pid", "to_regclass"}
        for name in case["inventory"]["functions"])


def checked(connection, sql: str) -> dict:
    """Context acquisition failures are setup failures, never candidate SQL outcomes."""
    result = connection.query(sql)
    if result["status"] != "ok": raise ValueError({"context_sql": sql, "result": result})
    for column in result.get("columns", []):
        oid = column["type_oid"]
        if oid not in connection.type_names:
            name = connection.query(f"SELECT format_type({oid},NULL)")
            if name["status"] != "ok": raise ValueError(name)
            connection.type_names[oid] = name["rows"][0][0]
        column["type"] = connection.type_names[oid]
    return result


def relations(connection, sql: str) -> dict:
    """Project named columns while retaining every visible row; no query predicate is pushed down."""
    result = {}
    for relation, columns in PROJECTIONS.items():
        name = relation.rsplit(".", 1)[1]
        if not re.search(r"\b" + re.escape(name) + r"\b", sql, re.I): continue
        query = "SELECT " + ",".join(columns) + " FROM " + relation
        result[relation] = checked(connection, query)
    return result


def before(connection, case: dict, sql: str) -> dict | None:
    """Bind session values and complete relation-name inventory to the same oracle connection."""
    if not applies(case): return None
    settings = checked(connection, SESSION_SQL)
    inventory = checked(connection, "SELECT n.nspname,c.relname,c.oid FROM pg_catalog.pg_class c JOIN pg_catalog.pg_namespace n ON n.oid=c.relnamespace ORDER BY n.nspname,c.relname")
    search_path = checked(connection, "SELECT unnest(current_schemas(true))")
    permissions = checked(connection, "SELECT nspname,has_schema_privilege(oid,'USAGE') FROM pg_namespace ORDER BY nspname")
    return {"settings": settings, "relations_before": relations(connection, sql),
            "relation_inventory": inventory, "effective_search_path": [row[0] for row in search_path["rows"]],
            "schema_usage": {name: allowed == "t" for name, allowed in permissions["rows"]},
            "projection_contract": "Every role-visible row of the named column projections; omitted columns are outside this explicit catalog abstraction.",
            "source": "Actual PostgreSQL16 system catalogs on the statement connection; no expected-result lookup."}


def after(connection, observation: dict | None, sql: str) -> None:
    """Preserve independently observed catalog poststate, including backend presence changes."""
    if observation is not None:
        observation["relations_after"] = relations(connection, sql)

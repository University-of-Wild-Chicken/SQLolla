"""Seed authentic schemas with declared synthetic rows in foreign-key dependency order."""
import json
import re

from oracle import literal, quote_identifier

COLUMNS = """SELECT json_build_object('table',c.relname,'name',a.attname,'oid',a.atttypid,
 'type',format_type(a.atttypid,a.atttypmod),'nullable',NOT a.attnotnull,
 'default',pg_get_expr(d.adbin,d.adrelid),'enum',t.typtype='e')
 FROM pg_attribute a JOIN pg_class c ON c.oid=a.attrelid
 JOIN pg_namespace n ON n.oid=c.relnamespace JOIN pg_type t ON t.oid=a.atttypid
 LEFT JOIN pg_attrdef d ON d.adrelid=a.attrelid AND d.adnum=a.attnum
 WHERE n.nspname='public' AND c.relkind='r' AND a.attnum>0 AND NOT a.attisdropped
 AND a.attgenerated='' ORDER BY c.relname,a.attnum"""
DEPENDENCIES = """SELECT child.relname,parent.relname FROM pg_constraint k
 JOIN pg_class child ON child.oid=k.conrelid JOIN pg_class parent ON parent.oid=k.confrelid
 JOIN pg_namespace n ON n.oid=child.relnamespace
 WHERE k.contype='f' AND n.nspname='public' AND child.oid<>parent.oid"""


def checked(connection, sql: str, parameters=None) -> dict:
    """Treat fixture errors as harness failures, never as expected source-query errors."""
    result = connection.query(sql, parameters)
    if result["status"] != "ok": raise ValueError({"fixture_sql": sql, "result": result})
    return result


def ordered_tables(connection) -> list[tuple[str, list]]:
    """Topologically order real application tables from PostgreSQL foreign-key metadata."""
    tables = {}
    for row in checked(connection, COLUMNS)["rows"]:
        column = json.loads(row[0])
        tables.setdefault(column["table"], []).append(column)
    dependencies = {name: set() for name in tables}
    for child, parent in checked(connection, DEPENDENCIES)["rows"]: dependencies[child].add(parent)
    result, visited = [], set()
    while len(visited) < len(tables):
        ready = sorted(name for name in tables if name not in visited and dependencies[name] <= visited)
        if not ready: raise ValueError("Seed context has cyclic foreign keys requiring an explicit fixture contract")
        for name in ready: result.append((name, tables[name])); visited.add(name)
    return result


def scalar_value(connection, column: dict) -> str | None:
    """Choose synthetic values within authentic scalar widths and declared nullability."""
    ty, name = column["type"], column["name"]
    if column["nullable"]: return None
    default = constant_default(column.get("default"))
    if default is not None: return default
    if column["table"] == "river_leader" and name == "name": return "default"
    if column["enum"]:
        return checked(connection, f"SELECT enumlabel FROM pg_enum WHERE enumtypid={column['oid']} ORDER BY enumsortorder LIMIT 1")["rows"][0][0]
    if ty.endswith("[]"): return "{}"
    if ty in {"integer", "bigint", "smallint", "double precision", "real"}: return "1"
    if ty.startswith("numeric"):
        modifier = re.search(r"\((\d+),(\d+)\)", ty)
        return "0." + "0" * (int(modifier[2]) - 1) + "1" if modifier and int(modifier[1]) <= int(modifier[2]) else "1"
    if ty == "boolean": return "true"
    if ty == "bytea": return "\\x01"
    if ty in {"json", "jsonb"}: return "{}"
    if ty.startswith("timestamp"): return "2000-01-01 00:00:00+00"
    if ty == "date": return "1996-01-01"
    if ty.startswith("bit("): return "0" * int(ty[4:-1])
    width = re.search(r"\((\d+)\)", ty)
    return "seed"[:int(width[1])] if width else "seed"


def constant_default(expression: str | None) -> str | None:
    """Use declared literal defaults without evaluating clocks, functions or sequences."""
    if not expression: return None
    if re.fullmatch(r"[+-]?\d+(?:\.\d+)?", expression): return expression
    if expression in {"true", "false"}: return expression
    quoted = re.fullmatch(r"'((?:''|[^'])*)'(?:::[a-zA-Z_][a-zA-Z_0-9 \[\]()]*)?", expression)
    return quoted[1].replace("''", "'") if quoted else None


def insert_row(connection, name: str, columns: list[dict]) -> dict:
    """Insert one explicit typed row; upstream existing seed rows remain authoritative."""
    existing = int(checked(connection, "SELECT count(*) FROM " + quote_identifier(name))["rows"][0][0])
    if existing: return {"table": name, "status": "retained_upstream_seed_rows", "rows": existing}
    parameters = [{"oid": int(column["oid"]), "value": scalar_value(connection, column)} for column in columns]
    names = ",".join(quote_identifier(column["name"]) for column in columns)
    placeholders = ",".join("$" + str(index) for index in range(1, len(columns) + 1))
    sql = "INSERT INTO " + quote_identifier(name) + "(" + names + ") VALUES(" + placeholders + ")"
    checked(connection, sql, parameters)
    return {"table": name, "status": "inserted_synthetic_row", "rows": 1,
            "values": {column["name"]: parameter["value"] for column, parameter in zip(columns, parameters)}}


def advance_sequences(connection, tables: list) -> None:
    """Align owned serial sequences after explicit seed identifiers to avoid accidental conflicts."""
    for name, columns in tables:
        for column in columns:
            sql = "SELECT pg_get_serial_sequence(" + literal(name) + "," + literal(column["name"]) + ")"
            sequence = checked(connection, sql)["rows"][0][0]
            if sequence:
                maximum = checked(connection, "SELECT max(" + quote_identifier(column["name"]) + ") FROM " + quote_identifier(name))["rows"][0][0]
                if maximum is not None: checked(connection, "SELECT setval(" + literal(sequence) + "," + maximum + ",true)")


def seed(connection) -> dict:
    """Create a nonempty test database while validating every real FK/check/default contract."""
    tables = ordered_tables(connection)
    observations = [insert_row(connection, name, columns) for name, columns in tables]
    advance_sequences(connection, tables)
    return {"status": "seeded", "catalog_claim": "authoritative upstream DDL", "row_claim": "synthetic values, not application production data",
            "rows_per_previously_empty_table": 1, "nullable_policy": "NULL", "tables": observations}

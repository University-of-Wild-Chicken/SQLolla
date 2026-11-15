"""Build PostgreSQL-only fixtures and read exact bags from its JSON output."""
import json
from collections import Counter
from typing import Any

SQL_TYPES = {"int32": "integer", "bool": "boolean", "text": 'text COLLATE "C"'}


def quote_identifier(value: str) -> str:
    """Quote generated identifiers without allowing them to alter SQL syntax."""
    return '"' + value.replace('"', '""') + '"'


def literal(value: Any) -> str:
    """Serialize fixture values under standard_conforming_strings=on."""
    if value is None:
        return "NULL"
    if isinstance(value, bool):
        return "TRUE" if value else "FALSE"
    if isinstance(value, int):
        return str(value)
    return "'" + value.replace("'", "''") + "'"


def table_setup(name: str, table: dict) -> list[str]:
    """Create independent temporary PostgreSQL input tables with explicit types."""
    columns = ", ".join(quote_identifier(name) + " " + SQL_TYPES[ty] for name, ty in table["columns"])
    statements = [f"CREATE TEMP TABLE {quote_identifier(name)} ({columns}) ON COMMIT DROP;"]
    if table["rows"]:
        rows = ",".join("(" + ",".join(literal(value) for value in row) + ")" for row in table["rows"])
        statements.append(f"INSERT INTO {quote_identifier(name)} VALUES {rows};")
    return statements


def oracle_statement(case: dict) -> str:
    """Wrap a query only in the PostgreSQL oracle to preserve duplicate row arrays."""
    columns = [f"c{index}" for index in range(len(case["oracle"]["output_types"]))]
    row = "json_build_array(" + ",".join("q." + name for name in columns) + ")"
    query = case["request"]["sql"]
    identifier = literal(case["request"]["id"])
    statement = (f"SELECT json_build_object('id',{identifier},'rows',COALESCE(json_agg({row}),'[]'::json)) "
                 f"FROM ({query}) AS q({','.join(columns)});")
    return bind_parameters(statement, case["request"].get("parameters", {}))


def bind_parameters(statement: str, parameters: dict) -> str:
    """Bind typed numbered parameters through PREPARE without rewriting query bytes."""
    if not parameters: return statement
    indices = sorted(int(index) for index in parameters)
    if indices != list(range(1, len(indices) + 1)): raise ValueError("Oracle fixture parameters must be contiguous")
    cells = [parameters[str(index)] for index in indices]
    names = {"int32": "integer", "bool": "boolean", "text": "text"}
    types = [names[cell["value"] if cell["type"] == "null" else cell["type"]] for cell in cells]
    values = [literal(None if cell["type"] == "null" else cell["value"]) for cell in cells]
    return f"PREPARE oracle_case ({','.join(types)}) AS {statement}\nEXECUTE oracle_case ({','.join(values)});\nDEALLOCATE oracle_case;"


def oracle_script(cases: list[dict]) -> str:
    """Build independent transactions so one case's database cannot leak into another."""
    statements = ["SET standard_conforming_strings=on; SET statement_timeout='10s'; SET timezone='UTC';"]
    for case in cases:
        statements.append("BEGIN;")
        for name, table in case["oracle"]["tables"].items():
            statements.extend(table_setup(name, table))
        statements.extend([oracle_statement(case), "ROLLBACK;"])
    return "\n".join(statements)


def rows_to_bag(rows: list, output_types: list[str]) -> Counter:
    """Canonicalize exact typed PostgreSQL row occurrences without losing NULL types."""
    if any(len(row) != len(output_types) for row in rows):
        raise ValueError("PostgreSQL result width differs from fixture output schema")
    typed = [[{"type": "null", "value": ty} if value is None else {"type": ty, "value": value}
              for value, ty in zip(row, output_types)] for row in rows]
    return typed_bag(typed)


def typed_bag(rows: list) -> Counter:
    """Compare unordered bags by canonical row JSON and multiplicity."""
    return Counter(json.dumps(row, ensure_ascii=False, sort_keys=True, separators=(",", ":")) for row in rows)

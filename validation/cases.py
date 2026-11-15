"""Independent finite database enumeration and SQL fixtures for the M1 profile."""
from __future__ import annotations

from itertools import combinations_with_replacement, product
from typing import Any, Iterator

DOMAINS = {"int32": [None, 0, 1], "bool": [None, False, True], "text": [None, "", "é"]}
SET_OPERATORS = {"union": "UNION", "union_all": "UNION ALL", "intersect": "INTERSECT",
                 "intersect_all": "INTERSECT ALL", "except": "EXCEPT", "except_all": "EXCEPT ALL"}
JOIN_OPERATORS = {"inner": "INNER JOIN", "left": "LEFT JOIN", "right": "RIGHT JOIN", "full": "FULL JOIN"}


def typed_cell(value: Any, scalar_type: str) -> dict:
    """Encode an input value using the public core Cell contract."""
    return {"type": "null", "value": scalar_type} if value is None else {"type": scalar_type, "value": value}


def table(columns: list[tuple[str, str]], rows: list[list]) -> dict:
    """Describe one independently generated SQL table and its row occurrences."""
    return {"columns": columns, "rows": rows}


def unary_table(scalar_type: str, values: tuple) -> dict:
    """Represent a one-column bag without deduplicating its occurrences."""
    return table([("x", scalar_type)], [[value] for value in values])


def fixture(name: str, sql: str, tables: dict, output_types: list[str], family: str) -> dict:
    """Build the CLI request and independent PostgreSQL setup specification."""
    catalog, database = {}, {}
    for name_in_sql, source in tables.items():
        stable_id = "public." + name_in_sql
        columns = [{"name": name, "ty": ty, "nullable": True, "qualifier": None, "schema": None}
                   for name, ty in source["columns"]]
        catalog[stable_id] = {"schema": "public", "name": name_in_sql, "columns": columns}
        database[stable_id] = [[typed_cell(value, ty) for value, (_, ty) in zip(row, source["columns"])]
                               for row in source["rows"]]
    request = {"id": name, "sql": sql, "catalog": {"search_path": ["public"], "tables": catalog},
               "database": database, "parameters": {}, "mode": "compare"}
    return {"request": request, "oracle": {"tables": tables, "output_types": output_types}, "family": family}


def small_bags(domain: list, maximum_rows: int = 2) -> list[tuple]:
    """Enumerate every unordered bag up to the stated cardinality exactly once."""
    return [bag for size in range(maximum_rows + 1) for bag in combinations_with_replacement(domain, size)]


def pair_cases(scalar_type: str, left: tuple, right: tuple, suffix: str) -> Iterator[dict]:
    """Cover six set variants and five join/product variants for one database."""
    tables = {"r": unary_table(scalar_type, left), "s": unary_table(scalar_type, right)}
    for label, operator in SET_OPERATORS.items():
        sql = f"SELECT x FROM r {operator} SELECT x FROM s"
        yield fixture(f"{scalar_type}/{label}/{suffix}", sql, tables, [scalar_type], "set")
    for label, operator in JOIN_OPERATORS.items():
        sql = f"SELECT l.x AS left_x, rr.x AS right_x FROM r AS l {operator} s AS rr ON l.x = rr.x"
        yield fixture(f"{scalar_type}/join_{label}/{suffix}", sql, tables, [scalar_type] * 2, "join")
    sql = "SELECT l.x AS left_x, rr.x AS right_x FROM r AS l CROSS JOIN s AS rr"
    yield fixture(f"{scalar_type}/product/{suffix}", sql, tables, [scalar_type] * 2, "product")


def unary_cases(scalar_type: str, bag: tuple, suffix: str) -> Iterator[dict]:
    """Cover NULL selection, self-comparison, duplicate projection and DISTINCT."""
    queries = {"is_null": ("SELECT x FROM r WHERE x IS NULL", 1),
               "is_not_null": ("SELECT x FROM r WHERE x IS NOT NULL", 1),
               "self_equal": ("SELECT x FROM r WHERE x = x", 1),
               "project_aliases": ("SELECT x AS first_x, x AS second_x FROM r", 2),
               "distinct": ("SELECT DISTINCT x FROM r", 1)}
    for label, (sql, width) in queries.items():
        yield fixture(f"{scalar_type}/{label}/{suffix}", sql, {"r": unary_table(scalar_type, bag)},
                      [scalar_type] * width, "unary")


def exhaustive_cases() -> Iterator[dict]:
    """Enumerate all selected operations over three complete ten-bag domains."""
    for scalar_type, domain in DOMAINS.items():
        bags = small_bags(domain)
        for index, bag in enumerate(bags):
            yield from unary_cases(scalar_type, bag, f"b{index:02}")
        for (li, left), (ri, right) in product(enumerate(bags), repeat=2):
            yield from pair_cases(scalar_type, left, right, f"l{li:02}r{ri:02}")


def boolean_cases() -> Iterator[dict]:
    """Project every truth-table input so UNKNOWN remains observable in results."""
    rows = [list(pair) for pair in product(DOMAINS["bool"], repeat=2)]
    tables = {"truths": table([("a", "bool"), ("b", "bool")], rows)}
    expressions = {"and": "a AND b", "or": "a OR b", "not": "NOT a", "eq": "a = b",
                   "ne": "a <> b", "not_and": "NOT (a AND b)", "nested": "(a OR b) AND NOT a",
                   "is_null": "a IS NULL", "is_not_null": "a IS NOT NULL"}
    for label, expression in expressions.items():
        sql = f"SELECT a, b, {expression} AS outcome FROM truths"
        yield fixture("truth/" + label, sql, tables, ["bool"] * 3, "three_valued_logic")
        sql = f"SELECT a, b FROM truths WHERE {expression}"
        yield fixture("truth/filter_" + label, sql, tables, ["bool"] * 2, "three_valued_logic")


def integer_cases() -> Iterator[dict]:
    """Exercise each integer comparison at NULL and signed32-bit boundaries."""
    domain = [None, -2147483648, -1, 0, 1, 2147483647]
    tables = {"ints": table([("a", "int32"), ("b", "int32")], [list(pair) for pair in product(domain, repeat=2)])}
    for label, operator in {"eq": "=", "ne": "<>", "lt": "<", "le": "<=", "gt": ">", "ge": ">="}.items():
        sql = f"SELECT a, b, a {operator} b AS outcome FROM ints"
        yield fixture("integer/" + label, sql, tables, ["int32", "int32", "bool"], "integer_boundaries")


def composed_cases() -> Iterator[dict]:
    """Cover composition, derived relations, unusual text and outer-join filters."""
    tables = {"r": unary_table("int32", (None, 0, 0, 1)), "s": unary_table("int32", (None, 0, 1))}
    queries = {"derived": "SELECT d.x AS renamed FROM (SELECT x FROM r WHERE x IS NOT NULL) AS d",
               "after_left_join": "SELECT l.x FROM r AS l LEFT JOIN s AS rr ON l.x = rr.x WHERE rr.x IS NULL",
               "on_filtered_left": "SELECT l.x FROM r AS l LEFT JOIN s AS rr ON l.x = rr.x AND rr.x = 1",
               "distinct_projection": "SELECT DISTINCT x AS renamed FROM r WHERE x = x"}
    for label, sql in queries.items():
        yield fixture("composition/" + label, sql, tables, ["int32"], "composition")
    text = (None, "", "a'b", "a\\b", "line\nbreak", "é", "e\u0301", "数据😀")
    text_tables = {"r": unary_table("text", text)}
    yield fixture("text/unicode_escapes", "SELECT x FROM r", text_tables, ["text"], "text_exact")
    yield fixture("text/distinct_exact", "SELECT DISTINCT x FROM r", text_tables, ["text"], "text_exact")


def literal_cases() -> Iterator[dict]:
    """Cover VALUES and source-free literal SELECT without oracle casts in input."""
    examples = {"integer": ("VALUES (1), (NULL), (1)", "int32"),
                "boolean": ("VALUES (TRUE), (NULL), (FALSE)", "bool"),
                "text": ("VALUES ('x'), (NULL), ('x')", "text"),
                "null_context": ("VALUES (NULL), (NULL)", "text"),
                "select_null": ("SELECT NULL", "text"),
                "select_integer": ("SELECT -2147483648", "int32")}
    for label, (sql, scalar_type) in examples.items():
        yield fixture("literal/" + label, sql, {}, [scalar_type], "literal")


def context_cases() -> Iterator[dict]:
    """Check PostgreSQL unknown-literal resolution independently of the binder."""
    examples = {"null_union_int": ("SELECT NULL UNION SELECT 1", "int32"),
                "text_union_int": ("SELECT '2' UNION SELECT 1", "int32"),
                "null_union_null": ("SELECT NULL UNION SELECT NULL", "text"),
                "null_intersect": ("SELECT NULL INTERSECT SELECT NULL", "text"),
                "text_integer_eq": ("SELECT '2' = 2", "bool"),
                "text_boolean_prefix": ("SELECT 'tru' = TRUE", "bool"),
                "text_boolean_numeric": ("SELECT '1' = TRUE", "bool"),
                "text_integer_spaces": ("SELECT ' +1 ' = 1", "bool")}
    for label, (sql, scalar_type) in examples.items():
        yield fixture("context/" + label, sql, {}, [scalar_type], "unknown_literal_context")


def cte_cases() -> Iterator[dict]:
    """Exercise nonrecursive lexical visibility, alias columns and repeated inputs."""
    tables = {"r": unary_table("int32", (None, 0, 0, 1)), "s": unary_table("int32", (None, 1))}
    examples = {"filter": ("WITH q AS (SELECT x FROM r WHERE x IS NOT NULL) SELECT x FROM q", 1),
                "rename": ("WITH q(y) AS (SELECT x FROM r) SELECT y FROM q", 1),
                "sequential": ("WITH q AS (SELECT x FROM r), qq AS (SELECT x FROM q WHERE x IS NOT NULL) SELECT x FROM qq", 1),
                "reuse": ("WITH q AS (SELECT x FROM r) SELECT l.x, rr.x FROM q AS l JOIN q AS rr ON l.x = rr.x", 2),
                "shadow_table": ("WITH r AS (VALUES (7), (NULL)) SELECT column1 FROM r", 1),
                "nested_shadow": ("WITH q AS (SELECT x FROM r) SELECT d.x FROM (WITH q AS (SELECT x FROM s) SELECT x FROM q) AS d", 1),
                "outer_visibility": ("WITH q AS (SELECT x FROM r) SELECT d.x FROM (SELECT x FROM q) AS d", 1),
                "derived_columns": ("SELECT d.y FROM (SELECT x FROM r) AS d(y)", 1),
                "self_join": ("SELECT l.x, rr.x FROM r AS l FULL JOIN r AS rr ON l.x = rr.x", 2)}
    for label, (sql, width) in examples.items():
        yield fixture("cte/" + label, sql, tables, ["int32"] * width, "cte_and_binding")


def multicolumn_cases() -> Iterator[dict]:
    """Ensure duplicate equivalence considers every column and typed NULL position."""
    columns = [("x", "int32"), ("b", "bool")]
    tables = {"r": table(columns, [[0, True], [0, False], [0, None], [None, True], [0, True]]),
              "s": table(columns, [[0, True], [None, True], [1, False]])}
    for label, operator in SET_OPERATORS.items():
        yield fixture("multicolumn/" + label, f"SELECT x,b FROM r {operator} SELECT x,b FROM s",
                      tables, ["int32", "bool"], "multicolumn")
    yield fixture("multicolumn/distinct", "SELECT DISTINCT x,b FROM r", tables, ["int32", "bool"], "multicolumn")
    sql = "SELECT l.x,l.b,rr.x,rr.b FROM r AS l FULL JOIN s AS rr ON l.x = rr.x"
    yield fixture("multicolumn/full_join", sql, tables, ["int32", "bool"] * 2, "multicolumn")


def all_cases() -> list[dict]:
    """Return the complete deterministic suite, without random sampling or skips."""
    groups = [exhaustive_cases(), boolean_cases(), integer_cases(), composed_cases(), literal_cases(),
              context_cases(), cte_cases(), multicolumn_cases()]
    return [case for group in groups for case in group]

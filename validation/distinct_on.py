"""Compare DISTINCT ON representative alternatives against independently permuted PostgreSQL inputs."""
from itertools import permutations
import json

from cases import fixture, table
from corpus_runtime import describe
from grouping import compare as compare_scalar
from libpq_client import PgConnection
from oracle import oracle_script
from ordering import compare
from pg_runtime import TemporaryPostgres
from rejections import execute
from run import CLI, RESULTS, digest, index_lines, write_json


def queries() -> list[tuple]:
    """Exercise visible/hidden keys, aliases, ordinals, NULL groups, ties and pagination."""
    return [("unordered", "SELECT DISTINCT ON (x) x,label FROM r", ["int32", "text"], False),
            ("hidden", "SELECT DISTINCT ON (x) label FROM r", ["text"], False),
            ("ties", "SELECT DISTINCT ON (x) x,label FROM r ORDER BY x", ["int32", "text"], True),
            ("winner", "SELECT DISTINCT ON (x) x,label FROM r ORDER BY x,label DESC", ["int32", "text"], True),
            ("null_first", "SELECT DISTINCT ON (x) label FROM r ORDER BY x DESC NULLS FIRST,label", ["text"], True),
            ("alias", "SELECT DISTINCT ON (z) x AS z,label FROM r ORDER BY z,label", ["int32", "text"], True),
            ("ordinal", "SELECT DISTINCT ON (1) x,label FROM r ORDER BY 1,2", ["int32", "text"], True),
            ("keys", "SELECT DISTINCT ON (x,label) label FROM r ORDER BY label,x", ["text"], True),
            ("duplicate_key", "SELECT DISTINCT ON (x,x) label FROM r ORDER BY x,label", ["text"], True),
            ("expression", "SELECT DISTINCT ON (x+1) label FROM r ORDER BY x+1,label", ["text"], True),
            ("positive_constant", "SELECT DISTINCT ON (+1) label FROM r", ["text"], False),
            ("limit", "SELECT DISTINCT ON (x) label FROM r LIMIT 1", ["text"], False),
            ("offset", "SELECT DISTINCT ON (x) label FROM r ORDER BY x LIMIT 1 OFFSET 1", ["text"], True),
            ("nested", "SELECT label FROM (SELECT DISTINCT ON (x) label FROM r ORDER BY x)q", ["text"], False)]


def build_cases() -> tuple[list, list, dict]:
    """Enumerate all six physical row permutations for both non-NULL and NULL duplicate keys."""
    bases, oracle, details = [], [], {}
    inputs = [[[1, "a"], [1, "b"], [None, "c"]], [[None, "a"], [None, "b"], [2, "c"]]]
    for number, rows in enumerate(inputs):
        for label, sql, types, ordered in queries():
            identifier = "distinct_on/" + str(number) + "/" + label
            bases.append(fixture(identifier, sql, {"r": table([("x", "int32"), ("label", "text")], rows)}, types, "distinct_on"))
            details[identifier] = {"ordered": ordered, "permutations": []}
            for index, order in enumerate(permutations(rows)):
                variant = fixture(identifier + "/" + str(index), sql,
                                  {"r": table([("x", "int32"), ("label", "text")], list(order))}, types, "distinct_on")
                oracle.append(variant)
                details[identifier]["permutations"].append(variant["request"]["id"])
    return bases, oracle, details


def scalar_cases() -> list[dict]:
    """Observe actual SQLSTATE for literal-key restrictions and leading-order mismatch."""
    queries = [f"SELECT DISTINCT ON ({key}) x FROM (VALUES(1),(2))r(x)"
               for key in ("true", "'x'", "NULL", "1.0", "-1", "0", "2", "2147483648", "-2147483649")]
    queries += ["SELECT DISTINCT ON (x) x,label FROM (VALUES(1,'a'),(1,'b'))r(x,label) ORDER BY label,x",
                "SELECT DISTINCT ON (x) x FROM (VALUES(1),(1),(2))r(x) ORDER BY x",
                "SELECT DISTINCT ON (x) x FROM (SELECT 1 AS x WHERE false)r"]
    return [{"id": "distinct_on/scalar/" + str(index), "sql": sql,
             "catalog": {"search_path": ["public"], "tables": {}}, "database": {}, "parameters": {},
             "mode": "compare", "execution": {"max_rows": 256, "max_outcomes": 256, "settings": {}, "table_metadata": {}}}
            for index, sql in enumerate(queries)]


def main() -> None:
    """Require exact complete outcome sets and SQL error observations on PostgreSQL16 and Alloy."""
    bases, oracle_cases, details = build_cases()
    scalars = scalar_cases()
    scalar_responses = execute(scalars, timeout=180)
    with TemporaryPostgres() as database:
        oracle = index_lines(database.execute(oracle_script(oracle_cases), timeout=120))
        with PgConnection(database) as connection:
            results = [compare_scalar(request, describe(connection, connection.query(request["sql"])), response)
                       for request, response in zip(scalars, scalar_responses, strict=True)]
        version, directory = database.execute("SELECT version();").strip(), database.directory
    results.extend(compare(bases, details, oracle, execute([case["request"] for case in bases], timeout=180)))
    failures = [result["id"] for result in results if result["status"] != "passed"]
    report = {"status": "failed" if failures else "passed", "cases": len(results), "failures": failures,
              "postgres_executions": len(oracle) + len(scalars), "postgres_version": version,
              "cluster_removed": not directory.exists(), "cli_sha256": digest(CLI.read_bytes()), "results": results,
              "scope": "Exact alternatives over all physical permutations of two tiny bags; deterministic literal errors; not universal completeness."}
    write_json(RESULTS / "distinct_on.json", report)
    print(json.dumps({key: value for key, value in report.items() if key != "results"}))
    if failures: raise SystemExit(1)


if __name__ == "__main__":
    main()

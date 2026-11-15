"""Independent PostgreSQL target-list UNNEST expansion, NULL padding and pagination evidence."""
from itertools import permutations
import json

from cases import fixture, table
from corpus_runtime import describe
from grouping import compare as compare_scalar
from libpq_client import PgConnection
from oracle import oracle_script
from oracle_contract import index_responses
from ordering import compare
from pg_runtime import TemporaryPostgres
from rejections import execute
from run import CLI, RESULTS, digest, index_lines, write_json


def queries() -> list[str]:
    """Exercise element types, nested scalar wrappers, duplicate rows and empty expansion."""
    return ["SELECT unnest(ARRAY[1,2,2,NULL])", "SELECT unnest(NULL::int[])",
            "SELECT unnest(ARRAY[]::int[])", "SELECT unnest(ARRAY[1,2]),unnest(ARRAY['a'])",
            "SELECT unnest(NULL::int[]),unnest(ARRAY['a','b'])", "SELECT 7,unnest(ARRAY[]::int[])",
            "SELECT 7,unnest(ARRAY[1,2])", "SELECT unnest(ARRAY[1,2])+10",
            "SELECT unnest(ARRAY[1,2])::bigint", "SELECT unnest('{{1,2},{3,4}}'::int[])",
            "SELECT unnest('[0:1]={4,5}'::int[])", "SELECT unnest(ARRAY[true,false,NULL])",
            "SELECT unnest(ARRAY['a',NULL,'b'])", "SELECT unnest(ARRAY[1::smallint,2::smallint])",
            "SELECT pg_catalog.unnest(ARRAY[1,2])", "SELECT DISTINCT unnest(ARRAY[1,1,2,NULL,NULL])",
            "SELECT unnest(ARRAY[1,2,3]) LIMIT 1", "SELECT unnest(ARRAY[1,2,3]) LIMIT 1 OFFSET 1",
            "SELECT unnest(ARRAY[1,2,3]) OFFSET 2", "SELECT unnest(ARRAY[1,2,3]) LIMIT 0",
            "SELECT nullif(unnest(ARRAY[1,2]),1)", "SELECT upper(unnest(ARRAY['a','b']))",
            "SELECT *,unnest(ARRAY[x,x+1]) FROM (VALUES(3))r(x)",
            "SELECT q.x FROM (SELECT unnest(ARRAY[1,2]) AS x)q ORDER BY x",
            "WITH q AS (SELECT unnest(ARRAY[1,2]) AS x) SELECT sum(x) FROM q",
            "SELECT unnest(ARRAY[1,2]) UNION ALL SELECT 3", "SELECT unnest(3)",
            "SELECT CASE WHEN true THEN unnest(ARRAY[1,2]) END",
            "SELECT coalesce(unnest(ARRAY[1,2]),0)"]


def requests() -> list[dict]:
    """Use explicit VALUES/ARRAY syntax rather than a preferred SQLolla-selected fixture."""
    return [{"id": "set_returning/" + str(index), "sql": sql,
             "catalog": {"search_path": ["public"], "tables": {}}, "database": {}, "parameters": {},
             "mode": "compare", "execution": {"max_rows": 256, "max_outcomes": 256, "settings": {}, "table_metadata": {}}}
            for index, sql in enumerate(queries())]


def paginated_cases() -> tuple[list, list, dict]:
    """Permute source occurrences independently while preserving array element order within each."""
    rows = [[1], [2], [3]]
    bases, oracle, details = [], [], {}
    for label, suffix in (("limit", " LIMIT 3"), ("offset", " LIMIT 3 OFFSET 1"), ("all", "")):
        identifier = "set_returning/source/" + label
        sql = "SELECT x,unnest(ARRAY[x,x+10]) FROM r" + suffix
        bases.append(fixture(identifier, sql, {"r": table([("x", "int32")], rows)}, ["int32", "int32"], "set_returning"))
        details[identifier] = {"ordered": False, "permutations": []}
        for index, order in enumerate(permutations(rows)):
            variant = fixture(identifier + "/" + str(index), sql, {"r": table([("x", "int32")], list(order))},
                              ["int32", "int32"], "set_returning")
            oracle.append(variant)
            details[identifier]["permutations"].append(variant["request"]["id"])
    return bases, oracle, details


def main() -> None:
    """Require actual SQL observations and exact finite page alternatives before claiming coverage."""
    submitted = requests()
    responses = index_responses(submitted, execute(submitted, timeout=180))
    bases, oracle_cases, details = paginated_cases()
    with TemporaryPostgres() as database:
        oracle = index_lines(database.execute(oracle_script(oracle_cases), timeout=120))
        with PgConnection(database) as connection:
            results = [compare_scalar(request, describe(connection, connection.query(request["sql"])), responses[request["id"]])
                       for request in submitted]
        version, directory = database.execute("SELECT version();").strip(), database.directory
    results.extend(compare(bases, details, oracle, execute([case["request"] for case in bases], timeout=180)))
    failures = [result["id"] for result in results if result["status"] != "passed"]
    report = {"status": "failed" if failures else "passed", "cases": len(results), "failures": failures,
              "postgres_executions": len(oracle) + len(submitted), "postgres_version": version,
              "cluster_removed": not directory.exists(), "cli_sha256": digest(CLI.read_bytes()), "results": results,
              "scope": "Target-list sibling UNNEST with typed padding/storage order; complete source-page alternatives over three rows. Nested and ordered SRF projection remains a separate admission boundary."}
    write_json(RESULTS / "set_returning.json", report)
    print(json.dumps({key: value for key, value in report.items() if key != "results"}))
    if failures: raise SystemExit(1)


if __name__ == "__main__":
    main()

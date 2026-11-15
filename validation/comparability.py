"""Measure type-driven PostgreSQL rejection independently from empty-input and NULL execution."""
import json

from corpus_runtime import describe
from grouping import compare
from libpq_client import PgConnection
from oracle_contract import index_responses
from pg_runtime import TemporaryPostgres
from rejections import execute
from run import CLI, RESULTS, digest, write_json


def relational_queries() -> list[str]:
    """Empty bags must not hide a missing equality/order operator in relational planning."""
    queries = []
    for ty in ["json", "json[]", "jsonb", "jsonb[]"]:
        source = "(SELECT NULL::" + ty + " x WHERE false)t"
        queries += ["SELECT DISTINCT x FROM " + source, "SELECT x FROM " + source + " ORDER BY x",
                    "SELECT x FROM " + source + " GROUP BY x", "SELECT count(DISTINCT x) FROM " + source,
                    "SELECT DISTINCT ON (x) x FROM " + source,
                    "SELECT row_number() OVER(PARTITION BY x ORDER BY x) FROM " + source]
        queries += ["SELECT NULL::" + ty + " WHERE false " + op + " SELECT NULL::" + ty + " WHERE false"
                    for op in ["UNION", "UNION ALL", "INTERSECT ALL", "EXCEPT ALL"]]
    return queries


def scalar_queries() -> list[str]:
    """Generic array functions retain strict NULL bypass and invocation-dependent errors."""
    return ["SELECT NULL::json[] = NULL::json[]", "SELECT '{}'::json[] = '{}'::json[]",
            "SELECT NULL::json[] < NULL::json[]", "SELECT '{}'::json[] < '{}'::json[]",
            "SELECT x=x FROM (SELECT NULL::json[] x WHERE false)t",
            "SELECT min(x) FROM (SELECT NULL::json x WHERE false)t",
            "SELECT max(x) FROM (SELECT NULL::json x WHERE false)t"]


def requests() -> list[dict]:
    """Force explicit-outcome encoding so errors and every success are checked by the same oracle."""
    return [{"id": "comparability/" + str(index), "sql": sql,
             "catalog": {"tables": {}, "search_path": ["public"]}, "database": {},
             "execution": {"max_rows": 256, "max_outcomes": 256, "settings": {}, "table_metadata": {}}}
            for index, sql in enumerate(relational_queries() + scalar_queries())]


def inspect(connection, submitted: dict, response: dict) -> dict:
    """Compare full outcomes and independently observe whether EXPLAIN already fails."""
    expected = describe(connection, connection.query(submitted["sql"]))
    planned = connection.query("EXPLAIN " + submitted["sql"])
    result = compare(submitted, expected, response)
    result["explain"] = planned
    for outcome in response.get("outcomes", []):
        if outcome["status"] != "sql_error": continue
        phase = outcome["outcome"]["phase"]
        actual = "planning" if planned["status"] == "error" else "statement"
        if phase not in (actual, "unspecified"):
            result["status"] = "failed"
            result["errors"].append("error_phase_mismatch")
    return result


def boundaries(connection) -> list[dict]:
    """Preserve genuine generic-array MIN behavior without crediting current coverage refusals."""
    queries = ["SELECT min(x) FROM (SELECT NULL::json[] x WHERE false)t",
               "SELECT min(x) FROM (VALUES('{}'::json[]))t(x)",
               "SELECT min(x) FROM (VALUES('{}'::json[]),('{}'::json[]))t(x)"]
    return [{"sql": sql, "postgres": describe(connection, connection.query(sql)),
             "explain": connection.query("EXPLAIN " + sql),
             "core_coverage": "uncovered_generic_array_extremum_not_credited"} for sql in queries]


def main() -> None:
    """Retain exact PostgreSQL and decoded Alloy evidence for every admitted comparison family."""
    submitted = requests()
    responses = index_responses(submitted, execute(submitted, timeout=240))
    with TemporaryPostgres() as database:
        with PgConnection(database) as connection:
            results = [inspect(connection, item, responses[item["id"]]) for item in submitted]
            uncovered = boundaries(connection)
        version, directory = database.execute("SELECT version();").strip(), database.directory
    failures = [row["id"] for row in results if row["status"] == "failed"]
    report = {"status": "failed" if failures else "passed", "cases": len(results), "failures": failures,
              "results": results, "uncovered_boundaries": uncovered, "postgres_version": version,
              "cli_sha256": digest(CLI.read_bytes()), "cluster_removed": not directory.exists()}
    write_json(RESULTS / "comparability.json", report)
    print(json.dumps({key: value for key, value in report.items() if key not in ("results", "uncovered_boundaries")}))
    if failures: raise SystemExit(1)


if __name__ == "__main__":
    main()

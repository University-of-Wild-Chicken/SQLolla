"""Actual PostgreSQL subquery observations, with admission and explicit refusals kept separate."""
import argparse
import json

from corpus_runtime import describe
from grouping import compare
from libpq_client import PgConnection
from oracle_contract import index_responses
from pg_runtime import TemporaryPostgres
from rejections import execute
from run import CLI, RESULTS, digest, write_json


def admitted_queries() -> list[str]:
    """Probe cardinality, lazy targets, correlation, quantified truth and explicit array order."""
    return ["SELECT (SELECT 1 WHERE false)", "SELECT (SELECT 7)",
            "SELECT (SELECT x FROM (VALUES(7),(7))t(x))",
            "SELECT EXISTS(SELECT 1/0)", "SELECT EXISTS(SELECT 1/0 WHERE false)",
            "SELECT EXISTS(SELECT NULL)", "SELECT EXISTS(SELECT DISTINCT 1/0)",
            "SELECT EXISTS(SELECT 1 ORDER BY 1/0)", "SELECT EXISTS(SELECT (SELECT 1/0))",
            "SELECT 1 IN (SELECT x FROM (VALUES(NULL::int),(1))t(x))",
            "SELECT 1 IN (SELECT x FROM (VALUES(NULL::int),(2))t(x))",
            "SELECT 1=ANY(SELECT 1 WHERE false),1=ALL(SELECT 1 WHERE false)",
            "SELECT 1=ANY(SELECT NULL::int),1=ALL(SELECT NULL::int)",
            "SELECT 1<ANY(SELECT x FROM(VALUES(1),(2))t(x)),1<ALL(SELECT x FROM(VALUES(1),(2))t(x))",
            "SELECT (SELECT t.x) FROM (VALUES(1),(2))t(x)",
            "SELECT (SELECT (SELECT t.x)) FROM (VALUES(1),(2))t(x)",
            "SELECT (SELECT x FROM (VALUES(9))i(x)) FROM (VALUES(1),(2))t(x)",
            "SELECT EXISTS(SELECT 1 FROM(VALUES(1),(3))i(x) WHERE i.x=t.x) FROM(VALUES(1),(2))t(x)",
            "SELECT CASE WHEN x>0 THEN 7 ELSE (SELECT y FROM(VALUES(1),(2))i(y)) END FROM(VALUES(1),(2))t(x)",
            "SELECT coalesce(x,(SELECT y FROM(VALUES(1),(2))i(y))) FROM(VALUES(1),(2))t(x)",
            "SELECT ARRAY(SELECT 1 WHERE false)",
            "SELECT ARRAY(SELECT x FROM(VALUES(1),(2),(NULL::int))t(x) ORDER BY x DESC NULLS LAST)",
            "SELECT ARRAY(SELECT x FROM(VALUES(1),(1))t(x))",
            "SELECT (SELECT 1,2)", "SELECT (SELECT 1/0 WHERE false)",
            "SELECT 1/x IN(SELECT 1 WHERE false) FROM(VALUES(0))t(x)",
            "SELECT (x<0) AND (SELECT y>0 FROM(VALUES(1),(2))i(y)) FROM(VALUES(1))t(x)",
            "SELECT x,sum(y) FROM(VALUES(1,5),(2,1))t(x,y) GROUP BY x HAVING sum(y)>(SELECT sum(x) FROM(VALUES(1),(2))s(x))"]


def refused_queries() -> list[tuple[str, str]]:
    """Pin deliberate coverage boundaries instead of counting syntax as successful execution."""
    return [("SELECT EXISTS(SELECT sum(1/0))", "unsupported.exists_shape"),
            ("SELECT EXISTS(SELECT 1 LIMIT 0)", "unsupported.exists_shape"),
            ("SELECT EXISTS(SELECT 1 UNION SELECT 2)", "unsupported.exists_shape"),
            ("SELECT ARRAY(SELECT x FROM(VALUES(1),(2))t(x))", "unsupported.subquery_alternatives"),
            ("SELECT (SELECT x FROM(VALUES(1),(2))t(x) LIMIT 1)", "unsupported.subquery_alternatives"),
            ("SELECT (SELECT 1/x FROM(VALUES(0),(1))t(x))", "unsupported.subquery_error_order"),
            ("SELECT (SELECT sum(t.x)) FROM(VALUES(1),(2))t(x)", "unsupported.outer_aggregate_scope")]


def cases() -> list[dict]:
    """Attach stable local source IDs without modifying the SQL submitted to PostgreSQL."""
    values = [(sql, None) for sql in admitted_queries()] + refused_queries()
    return [{"id": "subquery/" + str(index), "sql": sql, "expected_refusal": refusal}
            for index, (sql, refusal) in enumerate(values)]


def request(case: dict) -> dict:
    """All synthetic data lives in SQL VALUES, leaving physical catalog and state explicitly empty."""
    return {"id": case["id"], "sql": case["sql"], "catalog": {"search_path": ["public"], "tables": {}},
            "database": {}, "parameters": {}, "mode": "compare",
            "execution": {"max_rows": 256, "max_outcomes": 256, "settings": {}, "table_metadata": {}}}


def postgres(cases: list[dict]) -> tuple[list, str, bool]:
    """Execute unchanged SQL on actual PostgreSQL16 in an isolated, subsequently removed cluster."""
    with TemporaryPostgres() as database:
        with PgConnection(database) as connection:
            observations = [describe(connection, connection.query(case["sql"])) for case in cases]
        version, directory = database.execute("SELECT version();").strip(), database.directory
    return observations, version, not directory.exists()


def refusal(case: dict, expected: dict, response: dict) -> dict:
    """A tested refusal is evidence of a boundary, never a semantic equivalence pass."""
    diagnostic = response.get("code", response.get("error", {}).get("code"))
    if diagnostic is None:
        diagnostic = response.get("diagnostic", {}).get("code")
    valid = response.get("status") != "ok" and diagnostic == case["expected_refusal"]
    return {"id": case["id"], "sql": case["sql"], "sql_sha256": digest(case["sql"].encode()),
            "status": "refusal_verified" if valid else "failed", "expected_refusal": case["expected_refusal"],
            "postgres": expected, "response": response}


def comparisons(cases: list, observations: list, responses: dict) -> list:
    """Separate successful finite semantic comparisons from deliberately unsupported inputs."""
    results = []
    for case, expected in zip(cases, observations):
        response = responses[case["id"]]
        result = refusal(case, expected, response) if case["expected_refusal"] else compare(request(case), expected, response)
        results.append(result)
    return results


def main() -> None:
    """Retain one maintained report; PG-only mode gathers evidence before a shared binary rebuild."""
    parser = argparse.ArgumentParser()
    parser.add_argument("--postgres-only", action="store_true")
    args = parser.parse_args()
    submitted = cases()
    observed, version, removed = postgres(submitted)
    if args.postgres_only:
        print(json.dumps({"version": version, "cluster_removed": removed,
                          "observations": [dict(case, postgres=value) for case, value in zip(submitted, observed)]}))
        return
    requests = [request(case) for case in submitted]
    responses = index_responses(requests, execute(requests, timeout=240))
    results = comparisons(submitted, observed, responses)
    failures = [value["id"] for value in results if value["status"] == "failed"]
    report = {"status": "failed" if failures else "passed", "cases": len(results), "failures": failures,
              "semantic_passes": sum(value["status"] == "passed" for value in results),
              "verified_refusals": sum(value["status"] == "refusal_verified" for value in results),
              "postgres_version": version, "cluster_removed": removed, "cli_sha256": digest(CLI.read_bytes()),
              "claim": "Actual PostgreSQL observations versus every returned Rust outcome; Alloy checks the evaluator-backed table.",
              "limits": ["No universal PostgreSQL completeness proof", "Refusals are not semantic passes", "SQLSTATE does not establish error phase"], "results": results}
    write_json(RESULTS / "subqueries.json", report)
    print(json.dumps({key: value for key, value in report.items() if key != "results"}))
    if failures:
        raise SystemExit(1)


if __name__ == "__main__":
    main()

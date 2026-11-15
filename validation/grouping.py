"""Independent PostgreSQL aggregate, GROUP BY and FILTER boundary observations."""
import json

from corpus_runtime import checked, describe
from corpus_semantics import catalog_unchanged, observation_matches, schema_matches
from libpq_client import PgConnection
from oracle_contract import index_responses, top_level_ordered
from pg_runtime import TemporaryPostgres
from rejections import execute
from run import CLI, RESULTS, digest, write_json


def queries() -> list[str]:
    """Exercise result-family resolution, NULL grouping, FILTER order and alias binding."""
    return ["SELECT sum(x),avg(x),min(x),max(x),count(*) FROM (VALUES(1),(2))t(x)",
            "SELECT sum(x),avg(x) FROM (VALUES(1::bigint),(2::bigint))t(x)",
            "SELECT sum(x),avg(x),min(x) FROM (VALUES(1.23::numeric(4,2)),(2.45::numeric(4,2)))t(x)",
            "SELECT min(x),max(x) FROM (VALUES('ab'::varchar(3)),('a'::varchar(3)))t(x)",
            "SELECT sum(NULL)", "SELECT min(NULL)",
            "SELECT sum(1/x) FILTER (WHERE x<>0) FROM (VALUES(0),(1))t(x)",
            "SELECT sum(1/0) FILTER (WHERE false)",
            "SELECT x,count(*) FROM (VALUES(NULL::int),(NULL::int),(1))t(x) GROUP BY x ORDER BY x",
            "SELECT count(*),sum(x),avg(x),min(x) FROM (SELECT 1 AS x WHERE false)t",
            "SELECT x,count(*) FROM (SELECT 1 AS x WHERE false)t GROUP BY x",
            "SELECT x+1 AS y,count(*) FROM (VALUES(1),(1),(2))t(x) GROUP BY y ORDER BY y",
            "SELECT x AS z,x+1 AS z FROM (VALUES(1))t(x) GROUP BY z",
            "SELECT x AS z,x AS z FROM (VALUES(1))t(x) GROUP BY z",
            "SELECT *,count(*) FROM (VALUES(1))t(x) GROUP BY 1",
            "SELECT sum(count(*)) FROM (VALUES(1))t(x)",
            "SELECT count(DISTINCT x),sum(DISTINCT x) FROM (VALUES(1),(1),(2),(NULL))t(x)",
            "SELECT x,count(*) FROM (VALUES(1),(1),(2))t(x) GROUP BY x HAVING count(*)>1"]


def requests() -> list[dict]:
    """Declare an empty physical catalog; all query inputs are explicit VALUES relations."""
    sqls = queries() + ["SELECT x FROM (VALUES(1))t(x) GROUP BY " + clause
                       for clause in ("0", "-1", "'a'", "NULL", "true", "+1", "(1)", "1::int", "(-1)", "2147483648", "-2147483649")]
    return [{"id": "grouping/" + str(index), "sql": sql, "catalog": {"search_path": ["public"], "tables": {}},
             "database": {}, "parameters": {}, "mode": "compare",
             "execution": {"max_rows": 256, "max_outcomes": 256, "settings": {}, "table_metadata": {}}}
            for index, sql in enumerate(sqls)]


def compare(request: dict, expected: dict, response: dict) -> dict:
    """Require every complete Rust alternative to equal the deterministic PG observation."""
    errors = []
    if response.get("status") != "ok" or response.get("validation") != "ALLOY_TABLE_ROUNDTRIP" or not response.get("complete"):
        errors.append("incomplete_execution")
    else:
        outcomes = response["outcomes"]
        if not outcomes or any(value["outcome"]["effects"] or (value["status"] == "succeeded" and not catalog_unchanged(value["outcome"].get("catalog"), request["catalog"])) or not observation_matches(value, expected,
            {"database": {}}, request["catalog"], {}, top_level_ordered(request["sql"])) for value in outcomes):
            errors.append("postgres_outcome_mismatch")
        if expected["status"] == "ok" and not schema_matches(response["schema"], expected["columns"], {}): errors.append("schema_mismatch")
    return {"id": request["id"], "sql": request["sql"], "sql_sha256": digest(request["sql"].encode()),
            "status": "failed" if errors else "passed", "errors": errors, "postgres": expected, "response": response}


def typmods(connection) -> dict:
    """Observe actual expression typmods through a temporary view rather than guessing from OIDs."""
    checked(connection, "CREATE TEMP VIEW aggregate_types AS SELECT min(v) AS minv,min(n) AS minn,max(n) AS maxn,sum(n) AS sumn,avg(n) AS avgn FROM (VALUES('abc'::varchar(3),1.2::numeric(4,2)))t(v,n)")
    return checked(connection, "SELECT attname,format_type(atttypid,atttypmod),atttypmod::text FROM pg_attribute WHERE attrelid='aggregate_types'::regclass AND attnum>0 ORDER BY attnum")


def main() -> None:
    """Retain one maintained aggregate oracle report, including explicit unsupported outcomes."""
    submitted = requests()
    responses = index_responses(submitted, execute(submitted, timeout=180))
    with TemporaryPostgres() as database:
        with PgConnection(database) as connection:
            results = [compare(request, describe(connection, connection.query(request["sql"])), responses[request["id"]]) for request in submitted]
            types = typmods(connection)
        version, directory = database.execute("SELECT version();").strip(), database.directory
    failures = [result["id"] for result in results if result["status"] == "failed"]
    report = {"status": "failed" if failures else "passed", "cases": len(results), "failures": failures, "results": results,
              "typmod_observations": types, "postgres_version": version, "cluster_removed": not directory.exists(),
              "cli_sha256": digest(CLI.read_bytes()), "unobserved": ["SQL error planning/statement phase is not inferred from SQLSTATE"],
              "claim": "Actual PostgreSQL observations versus every returned Rust outcome; Alloy checks the evaluator-backed table."}
    write_json(RESULTS / "grouping.json", report)
    print(json.dumps({key: value for key, value in report.items() if key not in ("results", "typmod_observations")}))
    if failures: raise SystemExit(1)


if __name__ == "__main__":
    main()

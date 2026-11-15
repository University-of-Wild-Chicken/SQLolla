"""Actual PostgreSQL ranking observations plus independent complete finite tie-set checks."""
from itertools import permutations, product
import json

from corpus_runtime import describe
from corpus_semantics import pg_rows, rows_key, schema_matches
from grouping import compare
from libpq_client import PgConnection
from oracle_contract import index_responses
from pg_runtime import TemporaryPostgres
from rejections import execute
from run import CLI, RESULTS, digest, write_json


def deterministic() -> list[str]:
    """Exercise partitions, peers, NULL placement, source scope, conditional targets and pagination."""
    return ["SELECT x,row_number()OVER(ORDER BY x),rank()OVER(ORDER BY x),dense_rank()OVER(ORDER BY x) FROM(VALUES(3),(1),(2))t(x)",
            "SELECT x,rank()OVER(ORDER BY x),dense_rank()OVER(ORDER BY x) FROM(VALUES(1),(1),(3),(NULL::int))t(x)",
            "SELECT x,rank()OVER(ORDER BY x DESC NULLS FIRST),dense_rank()OVER(ORDER BY x DESC NULLS FIRST) FROM(VALUES(1),(1),(3),(NULL::int))t(x)",
            "SELECT g,x,row_number()OVER(PARTITION BY g ORDER BY x) FROM(VALUES(NULL::int,2),(NULL::int,1),(1,3))t(g,x)",
            "SELECT x,CASE WHEN x>0 THEN row_number()OVER(ORDER BY x) ELSE NULL END FROM(VALUES(0),(1),(2))t(x)",
            "SELECT *,row_number()OVER(ORDER BY x) FROM(VALUES(2),(1))t(x)",
            "SELECT rank,rank()OVER() FROM(VALUES(1),(2))t(rank)",
            "SELECT x FROM(VALUES(2),(1))t(x) ORDER BY row_number()OVER(ORDER BY x DESC)",
            "SELECT x,row_number()OVER(ORDER BY x) AS n FROM(VALUES(2),(1))t(x) ORDER BY n DESC LIMIT 1",
            "SELECT row_number()OVER() FROM(VALUES(1))t(x) WHERE false",
            "SELECT row_number()OVER(ORDER BY 1/0) FROM(VALUES(1))t(x) WHERE false",
            "SELECT x,rank()OVER(ORDER BY 1),dense_rank()OVER(ORDER BY 1) FROM(VALUES(1),(2),(3))t(x)",
            "SELECT DISTINCT rank()OVER(ORDER BY x) FROM(VALUES(1),(1),(2))t(x)",
            "SELECT x,(SELECT row_number()OVER(ORDER BY t.x)) FROM(VALUES(2),(1))t(x)",
            "SELECT row_number()OVER() UNION ALL SELECT row_number()OVER()",
            "SELECT x,rank()OVER(),dense_rank()OVER() FROM(VALUES(1),(2),(3),(4),(5),(6),(7),(8))t(x)",
            "SELECT row_number()OVER(ORDER BY 1/x) FROM(VALUES(0))t(x) WHERE false",
            "SELECT row_number()OVER(ORDER BY 1/x) FROM(VALUES(0))t(x)"]


def tie_cases() -> list[dict]:
    """Small independent mathematical sets test completeness, not only membership of one PG tie choice."""
    cases = []
    for source in ("(1,0),(2,0)", "(2,0),(1,0)"):
        sql = "SELECT id,row_number()OVER(ORDER BY k) AS rn,row_number()OVER(ORDER BY k) AS again FROM(VALUES" + source + ")t(id,k)"
        choices = [[[identifier,number,number] for number,identifier in enumerate(order,1)] for order in permutations((1,2))]
        cases.append({"sql":sql,"allowed":choices})
    sql = "SELECT id,row_number()OVER(PARTITION BY g ORDER BY k) AS rn FROM(VALUES(1,0,0),(2,0,0),(3,1,0),(4,1,0))t(id,g,k)"
    choices = [[[identifier,number] for order in orders for number,identifier in enumerate(order,1)]
               for orders in product(permutations((1,2)),permutations((3,4)))]
    return cases + [{"sql":sql,"allowed":choices}]


def requests() -> tuple[list[dict], dict]:
    """Keep every occurrence explicit in unchanged SQL, with an empty physical database."""
    cases = [{"sql":sql} for sql in deterministic()] + tie_cases()
    submitted = [{"id":"window/"+str(index),"sql":case["sql"],"mode":"compare","catalog":{"search_path":["public"],"tables":{}},
                  "database":{},"parameters":{},"execution":{"max_rows":256,"max_outcomes":256,"settings":{},"table_metadata":{}}}
                 for index,case in enumerate(cases)]
    return submitted, {request["id"]:case["allowed"] for request,case in zip(submitted,cases) if "allowed" in case}


def independent_set(allowed: list) -> set:
    """IDs remain int32 while all PostgreSQL ranking outputs are int64."""
    return {json.dumps(rows_key([[{"type":"int32" if index==0 else "int64","value":value}
                                 for index,value in enumerate(row)] for row in rows])) for rows in allowed}


def tie_compare(request: dict, expected: dict, response: dict, allowed: list) -> dict:
    """Require equality with the full finite set and independently verify the actual PG observation is in it."""
    errors, legal = [], independent_set(allowed)
    if response.get("status") != "ok" or not response.get("complete") or response.get("validation") != "ALLOY_TABLE_ROUNDTRIP": errors.append("incomplete_execution")
    else:
        outcomes = response["outcomes"]
        if any(value["status"] != "succeeded" or value["outcome"]["effects"] or value["outcome"]["database"] or value["outcome"]["ordering"] != "bag" for value in outcomes): errors.append("unexpected_effect_or_failure")
        else:
            actual = {json.dumps(rows_key(value["outcome"]["rows"])) for value in outcomes}
            if actual != legal: errors.append("incomplete_or_invented_tie_set")
        if not schema_matches(response["schema"],expected["columns"],{}): errors.append("schema_mismatch")
    if expected["status"] != "ok" or json.dumps(rows_key(pg_rows(expected,{}))) not in legal: errors.append("postgres_not_in_legal_set")
    return {"id":request["id"],"sql":request["sql"],"status":"failed" if errors else "passed","errors":errors,
            "postgres":expected,"response":response,"independent_legal_sets":allowed}


def main() -> None:
    """Maintain one reproducible evidence report and remove the owned PostgreSQL cluster."""
    submitted, ties = requests()
    responses = index_responses(submitted,execute(submitted,timeout=180))
    with TemporaryPostgres() as database:
        with PgConnection(database) as connection:
            expected = [describe(connection,connection.query(request["sql"])) for request in submitted]
        version,directory=database.execute("SELECT version()").strip(),database.directory
    results = [tie_compare(request,observed,responses[request["id"]],ties[request["id"]]) if request["id"] in ties
               else compare(request,observed,responses[request["id"]]) for request,observed in zip(submitted,expected)]
    failures=[result["id"] for result in results if result["status"] != "passed"]
    report={"status":"failed" if failures else "passed","cases":len(results),"failures":failures,"results":results,
            "postgres_version":version,"cluster_removed":not directory.exists(),"cli_sha256":digest(CLI.read_bytes()),
            "claim":"Actual PG observations and exact independently enumerated small tie sets; finite Alloy checks evaluator-backed outcomes."}
    write_json(RESULTS / "windows.json",report)
    print(json.dumps({key:value for key,value in report.items() if key != "results"}))
    if failures: raise SystemExit(1)


if __name__ == "__main__":
    main()

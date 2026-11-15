"""Actual PostgreSQL immutable SQL function expansion with catalog-observed bodies and parameters."""
import json

from corpus_runtime import checked, describe
from corpus_wire import inspect, typed_rows
from cte import compare
from libpq_client import PgConnection
from oracle_contract import index_responses
from pg_runtime import TemporaryPostgres
from rejections import execute
from run import CLI, RESULTS, digest, write_json


def cases() -> list[tuple[str, str]]:
    """Check named/positional parameters, NULLs, return conversion and variable bit widths."""
    addition = "CREATE FUNCTION f(a int,b int) RETURNS int LANGUAGE SQL IMMUTABLE AS 'SELECT a+b'"
    positional = "CREATE FUNCTION f(int,int) RETURNS int LANGUAGE SQL IMMUTABLE AS 'SELECT $1+$2'"
    bits = "CREATE FUNCTION f(mask bit,state text) RETURNS bool LANGUAGE SQL IMMUTABLE AS $$ SELECT CASE state WHEN 'on' THEN get_bit(mask,7) ELSE 0 END=1 $$"
    result = [(addition, query) for query in ("SELECT f(1,2)", "SELECT f(NULL,2)", "SELECT f(x,y) FROM t",
              "SELECT public.f(x,y) FROM t ORDER BY x", "SELECT f(x,x) FROM t", "SELECT f(1,2),f(3,4)")]
    result += [(positional, "SELECT f(x,y) FROM t"), (positional, "SELECT f(NULL,2)")]
    result += [(bits, query) for query in ("SELECT f(m,s) FROM t", "SELECT f(m,'on') FROM t", "SELECT f(NULL::bit,'on')",
              "SELECT f(m,s) FROM (VALUES('1'::bit(1),'on'))q(m,s)",
              "SELECT f(m,s) FROM (VALUES('1'::bit(1),'off'))q(m,s)")]
    result += [("CREATE FUNCTION f(a int) RETURNS bigint LANGUAGE SQL IMMUTABLE AS 'SELECT a'", "SELECT f(x) FROM t"),
               ("CREATE FUNCTION f(a int) RETURNS int LANGUAGE SQL IMMUTABLE AS 'SELECT 1/a'", "SELECT f(0)"),
               ("CREATE FUNCTION f(a int) RETURNS int LANGUAGE SQL IMMUTABLE AS 'SELECT 7'", "SELECT f(NULL)")]
    return result + resolution_cases()


def resolution_cases() -> list:
    """Resolve builtin collisions and reject explicit-only argument conversions."""
    result = []
    identity = "CREATE FUNCTION f(a int) RETURNS int LANGUAGE SQL IMMUTABLE AS 'SELECT a'"
    result += [(identity, query) for query in ("SELECT f('1'::text)", "SELECT f(1::bigint)",
        "SELECT f('1')", "SELECT f(NULL::bigint)", "SELECT f(1::smallint)")]
    wider = "CREATE FUNCTION f(a bigint) RETURNS bigint LANGUAGE SQL IMMUTABLE AS 'SELECT a'"
    result += [(wider, "SELECT f(x) FROM t"), (wider, "SELECT f(1)")]
    shadow = "CREATE FUNCTION public.abs(a integer) RETURNS integer LANGUAGE SQL IMMUTABLE AS 'SELECT 999'"
    result += [(shadow, query) for query in ("SELECT abs(1)", "SELECT public.abs(1)", "SELECT pg_catalog.abs(1)")]
    result += [(shadow, "SELECT abs(1)", ["public", "pg_catalog"]),
               (shadow, "SELECT abs(1)", ["pg_catalog", "public"])]
    text_abs = "CREATE FUNCTION public.abs(a text) RETURNS integer LANGUAGE SQL IMMUTABLE AS 'SELECT 42'"
    result += [(text_abs, "SELECT abs('x'::text)"), (text_abs, "SELECT abs(1)")]
    integer_lower = "CREATE FUNCTION public.lower(a integer) RETURNS integer LANGUAGE SQL IMMUTABLE AS 'SELECT a'"
    result += [(integer_lower, "SELECT lower(2)"), (integer_lower, "SELECT lower('ABC'::text)")]
    result += [("CREATE FUNCTION f(a varchar) RETURNS varchar LANGUAGE SQL IMMUTABLE AS 'SELECT a'", "SELECT f('x'::name)"),
               ("CREATE FUNCTION f(a char) RETURNS char LANGUAGE SQL IMMUTABLE AS 'SELECT a'", "SELECT f('x'::name)"),
               ("CREATE FUNCTION f(a name) RETURNS name LANGUAGE SQL IMMUTABLE AS 'SELECT a'", "SELECT f('x'::text)"),
               ("CREATE FUNCTION f(a text) RETURNS text LANGUAGE SQL IMMUTABLE AS 'SELECT a'", "SELECT f('x'::name)")]
    return result


def snapshot(connection) -> dict:
    """Observe all physical rows, including unused columns and duplicate occurrences."""
    return {"database": {"public.t": describe(connection, checked(connection, "SELECT * FROM t"))}}


def observe(database, connection, index: int, case: tuple) -> tuple[dict, dict]:
    """Read definitions from pg_proc and compare the exact SQL against PG, never a function answer table."""
    definition, sql, *path = case
    search_path = path[0] if path else ["public"]
    checked(connection, "SET search_path TO public")
    checked(connection, "DROP SCHEMA public CASCADE")
    checked(connection, "CREATE SCHEMA public")
    checked(connection, "CREATE TABLE t(x int,y int,m bit(8),s text)")
    checked(connection, "INSERT INTO t VALUES(1,2,B'00000001','on'),(3,NULL,B'00000000','off')")
    checked(connection, definition)
    checked(connection, "SET search_path TO " + ",".join(search_path))
    semantics = inspect(database)
    semantics["catalog"]["search_path"] = search_path
    if semantics["catalog"].get("functions"):
        semantics["catalog"]["functions"]["search_path"] = search_path
    before = snapshot(connection)
    columns = semantics["catalog"]["tables"]["public.t"]["columns"]
    request = {"id": "sql_functions/" + str(index), "sql": sql, "catalog": semantics["catalog"],
               "database": {"public.t": typed_rows(before["database"]["public.t"]["rows"], [column["ty"] for column in columns])},
               "parameters": {}, "mode": "compare", "execution": {"max_rows": 256, "max_outcomes": 256,
               "settings": {}, "table_metadata": semantics["table_metadata"]}}
    explained = connection.query("EXPLAIN " + sql)
    checked(connection, "BEGIN")
    result = describe(connection, connection.query(sql))
    checked(connection, "ROLLBACK")
    return request, {"result": result, "after": snapshot(connection), "explain": explained,
                     "observed_phase": "planning" if explained["status"] == "error" else "statement",
                     "function_observations": semantics["function_observations"]}


def main() -> None:
    """Require actual PG values, errors, type labels and frame equality on every complete outcome."""
    with TemporaryPostgres() as database:
        with PgConnection(database) as connection:
            fixtures = [observe(database, connection, index, case) for index, case in enumerate(cases())]
        version, directory = database.execute("SELECT version();").strip(), database.directory
    submitted = [request for request, _ in fixtures]
    responses = index_responses(submitted, execute(submitted, timeout=240))
    results = [compare(request, oracle, responses[request["id"]]) for request, oracle in fixtures]
    failures = [result["id"] for result in results if result["status"] != "passed"]
    report = {"status": "failed" if failures else "passed", "cases": len(results), "failures": failures,
              "postgres_version": version, "cluster_removed": not directory.exists(),
              "cli_sha256": digest(CLI.read_bytes()), "results": results,
              "scope": "Explicit immutable non-STRICT SECURITY INVOKER scalar SQL bodies with inert typed arguments; no recursion, nested UDF, external reads or effectful body claims."}
    write_json(RESULTS / "sql_functions.json", report)
    print(json.dumps({key: value for key, value in report.items() if key != "results"}))
    if failures: raise SystemExit(1)


if __name__ == "__main__":
    main()

"""Actual PostgreSQL catalog scalar, stable session and relation-identity lookup comparisons."""
import json

from corpus_runtime import checked, describe
from corpus_wire import system_request
from grouping import compare
from libpq_client import PgConnection
from oracle import literal
from oracle_contract import index_responses
from pg_runtime import TemporaryPostgres
from rejections import execute
from run import CLI, RESULTS, digest, write_json
import system_context


def scalar_queries() -> list[str]:
    """Keep OID width/radix/cast rules and catalog byte identity independently observable."""
    values = ["0", "4294967295", "4294967296", "-1", "-2147483648", "0x10", "010", "09", "1_000", "  +12  "]
    queries = ["SELECT " + literal(value) + "::oid" for value in values]
    queries += ["SELECT (-1)::integer::oid", "SELECT (-1)::bigint::oid", "SELECT 4294967295::bigint::oid",
                "SELECT '4294967295'::oid::integer", "SELECT '4294967295'::oid::bigint",
                "SELECT '4294967295'::oid > '2147483647'::oid", "SELECT 1::smallint::oid"]
    queries += ["SELECT " + literal(value) + '::"char", ' + literal(value) + '::"char"::integer'
                for value in ["", "abc", "i", "é", "\\377", "\\001", "\\000"]]
    queries += ['SELECT 127::"char"', 'SELECT 128::"char"', 'SELECT (-128)::"char"',
                'SELECT \'\\377\'::"char" > \'a\'::"char"']
    return queries


def lookup_queries() -> list[str]:
    """Exercise actual name splitting, shadowing, missing names and numeric identity transport."""
    values = ["", "missing", "1234", "0", "-", "4294967295", "4294967296", "-1", "010", "09",
              "lookup_entity", "public.MiXeD", 'public."MiXeD"', ' PUBLIC . "MiXeD" ', "odd-name", '"odd-name"',
              '"double""quote"', "some table", "a..b", "a.b.c.d", 'postgres.public."MiXeD"',
              "other.public.foo", '"unterminated', '"a"b', "public.*"]
    queries = ["SELECT to_regclass(" + literal(value) + ")" for value in values]
    queries += ["SELECT to_regclass('lookup_entity')::oid", "SELECT to_regclass('missing') IS NULL",
                "SELECT to_regclass(NULL)", "SELECT to_regclass(true)", "SELECT to_regclass(1)",
                "SELECT CASE WHEN false THEN to_regclass('other.public.foo') IS NULL ELSE true END"]
    return queries


def definitions() -> list[tuple[str, str, str]]:
    """Associate each SQL statement with an independently executed search-path setting."""
    queries = scalar_queries() + lookup_queries() + ["SELECT version(),pg_backend_pid(),current_schema(),current_database()",
        "SELECT current_setting('server_version_num')", "SELECT current_setting('yb_enable_listen_notify',true)",
        "SELECT current_setting('yb_enable_listen_notify')", "SELECT pg_terminate_backend(NULL)",
        "SELECT pg_terminate_backend(123) WHERE false"]
    result = [(sql, "public", "") for sql in queries]
    result += [("SELECT current_schema()", "", ""), ("SELECT current_schema", "", ""),
               ("SELECT to_regclass('lookup_entity')", "second,first", ""),
               ("SELECT to_regclass('lookup_entity')", "first,second", ""),
               ("SELECT to_regclass('private_space.secret')", "public", "lookup_reader"),
               ("SELECT to_regclass('absent_space.secret')", "public", "lookup_reader"),
               ("SELECT to_regclass('secret'),current_schema()", "private_space", "lookup_reader")]
    return result


def setup(connection) -> None:
    """Create only isolated lookup identities; queries do not read or mutate their table rows."""
    for sql in ["CREATE SCHEMA first", "CREATE SCHEMA second", "CREATE TABLE public.lookup_entity(x int)",
                "CREATE TABLE first.lookup_entity(x int)", "CREATE TABLE second.lookup_entity(x int)",
                'CREATE TABLE public."MiXeD"(x int)', 'CREATE TABLE public."odd-name"(x int)',
                'CREATE TABLE public."double""quote"(x int)', "CREATE ROLE lookup_reader",
                "CREATE SCHEMA private_space", "CREATE TABLE private_space.secret(x int)"]:
        checked(connection, sql)


def observations(connection) -> tuple[list, dict]:
    """Capture complete context on the same connection before each unchanged source statement."""
    submitted, expected = [], {}
    case = {"inventory": {"features": ["system_catalog"], "functions": []}}
    for index, (sql, search_path, role) in enumerate(definitions()):
        checked(connection, "RESET ROLE")
        if role: checked(connection, "SET ROLE " + role)
        checked(connection, "SET search_path=" + (search_path if search_path else "''"))
        system = system_context.before(connection, case, sql)
        if system["relations_before"]: raise ValueError("Lookup-only suite unexpectedly reads a catalog relation")
        request = {"id": "catalog/" + str(index), "sql": sql, "catalog": {"tables": {}, "search_path": ["public"]},
                   "database": {}, "parameters": {}, "mode": "compare",
                   "execution": {"max_rows": 256, "max_outcomes": 256, "settings": {}, "table_metadata": {}}}
        request = system_request(request, {"system_context": system})
        submitted.append(request)
        expected[request["id"]] = describe(connection, connection.query(sql))
    return submitted, expected


def main() -> None:
    """Require every decoded Rust outcome to match each deterministic PostgreSQL observation."""
    with TemporaryPostgres() as database:
        with PgConnection(database) as connection:
            setup(connection)
            submitted, expected = observations(connection)
        version, directory = database.execute("SELECT version()").strip(), database.directory
    responses = index_responses(submitted, execute(submitted, timeout=300))
    results = [compare(request, expected[request["id"]], responses[request["id"]]) for request in submitted]
    failures = [value["id"] for value in results if value["status"] != "passed"]
    report = {"status": "failed" if failures else "passed", "cases": len(results), "failures": failures, "results": results,
              "requests_sha256": digest(json.dumps(submitted, sort_keys=True).encode()), "cli_sha256": digest(CLI.read_bytes()),
              "postgres_version": version, "cluster_removed": not directory.exists(),
              "claim": "Actual PostgreSQL versus every complete Rust outcome; Regclass transport is numeric identity, with original PG text retained.",
              "sources": ["https://www.postgresql.org/docs/16/datatype-oid.html", "https://www.postgresql.org/docs/16/functions-info.html",
                          "https://github.com/postgres/postgres/blob/REL_16_16/src/backend/utils/adt/regproc.c",
                          "https://github.com/postgres/postgres/blob/REL_16_16/src/backend/utils/adt/char.c"]}
    write_json(RESULTS / "catalog-context.json", report)
    print(json.dumps({key: value for key, value in report.items() if key != "results"}))
    if failures: raise SystemExit(1)


if __name__ == "__main__":
    main()

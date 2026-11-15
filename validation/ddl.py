"""Actual PostgreSQL DDL/view catalog transitions and finite Alloy outcome replay checks."""
import json

import corpus_ddl
from corpus_runtime import checked, describe
from corpus_semantics import pg_rows, rows_key, schema_matches
from corpus_sequences import relation_lookup
from corpus_wire import inspect, typed_rows
from libpq_client import PgConnection
from oracle import quote_identifier
from oracle_contract import index_responses
from pg_runtime import TemporaryPostgres
from rejections import execute
from run import CLI, RESULTS, digest, write_json


def cases() -> list[tuple[str, str]]:
    """Exercise creates, frames, skipped invalid declarations, view reads and dependency failures."""
    return [("", "CREATE TABLE t(x int PRIMARY KEY,y varchar(12) NOT NULL,UNIQUE(y))"),
            ("CREATE TABLE t(x int);INSERT INTO t VALUES(7)", "CREATE TABLE IF NOT EXISTS t(x text)"),
            ("CREATE SEQUENCE t", "CREATE TABLE IF NOT EXISTS t(x int)"),
            ("CREATE TABLE t(x int)", "CREATE TABLE t(x int)"),
            ("", "CREATE TABLE t(x int,x text)"),
            ("", "CREATE TABLE t(x int,PRIMARY KEY(x,x))"),
            ("", "CREATE VIEW v AS SELECT 1/0 AS x"),
            ("", "CREATE VIEW v(a) AS SELECT 7"),
            ("CREATE TABLE t(x int);INSERT INTO t VALUES(7),(7)", "CREATE VIEW v AS SELECT x FROM t"),
            ("CREATE TABLE t(x int);CREATE VIEW v AS SELECT x FROM t", "CREATE VIEW w AS SELECT x FROM v"),
            ("CREATE TABLE t(x int);CREATE VIEW v AS SELECT x FROM t", "DROP VIEW v"),
            ("", "DROP VIEW IF EXISTS missing"), ("", "DROP VIEW missing"),
            ("CREATE TABLE t(x int)", "DROP VIEW IF EXISTS t"),
            ("CREATE VIEW v AS SELECT 1 AS x;CREATE VIEW w AS SELECT x FROM v", "DROP VIEW v"),
            ("CREATE VIEW v AS SELECT 1 AS x;CREATE VIEW w AS SELECT x FROM v", "DROP VIEW v,w"),
            ("CREATE VIEW v AS SELECT 1 AS x", "CREATE VIEW v AS SELECT 2 AS x"),
            ("CREATE VIEW v AS SELECT 1 AS x", "SELECT * FROM v"),
            ("CREATE TABLE t(x int);INSERT INTO t VALUES(7),(7);CREATE VIEW v AS SELECT x FROM t", "SELECT sum(x) FROM v"),
            ("CREATE VIEW v AS SELECT 1/0 AS x", "SELECT * FROM v"),
            ("CREATE VIEW v AS SELECT 1 AS x", "WITH v AS(SELECT 9 AS x) SELECT * FROM public.v"),
            ("CREATE VIEW v AS SELECT 1 AS x", "CREATE VIEW w AS SELECT x FROM v WHERE x>0"),
            ("SET search_path=missing,public", "CREATE TABLE t(x int)"),
            ("SET search_path=public,pg_catalog", "CREATE VIEW v AS SELECT 1 AS x"),
            ("SET search_path=''", "CREATE TABLE t(x int)"),
            ("SET search_path=''", "CREATE VIEW public.v AS SELECT 1 AS x")]


def physical(connection, catalog: dict) -> dict:
    """Capture only physical declared bags; views are definitions, never stored table rows."""
    result = {}
    for key, table in catalog["tables"].items():
        relation = quote_identifier(table["schema"]) + "." + quote_identifier(table["name"])
        rows = checked(connection, "SELECT * FROM " + relation)["rows"]
        result[key] = typed_rows(rows, [column["ty"] for column in table["columns"]])
    return result


def prepare(database, connection, index: int, setup: str, sql: str) -> dict:
    """Observe prestate, execute unchanged SQL once, and observe the independent catalog poststate."""
    checked(connection, "DROP SCHEMA public CASCADE")
    checked(connection, "CREATE SCHEMA public")
    checked(connection, "SET search_path=public")
    for statement in setup.split(";"):
        if statement: checked(connection, statement)
    context, before = inspect(database), corpus_ddl.capture(connection, {})
    exclude_views(context["catalog"],before)
    request = {"id": "ddl/" + str(index), "sql": sql, "catalog": context["catalog"],
               "database": physical(connection, context["catalog"]), "parameters": {}, "mode": "compare",
               "execution": {"max_rows":256,"max_outcomes":256,"settings":{},"table_metadata": context["table_metadata"]}}
    corpus_ddl.merge_request(request, before)
    lookup = relation_lookup(connection)
    request["catalog"].update(search_path=lookup["search_path"], relation_lookup=lookup)
    request["execution"]["catalog_lookup"] = lookup
    observed = describe(connection, connection.query(sql))
    after, metadata = corpus_ddl.capture(connection, {}), inspect(database)
    exclude_views(metadata["catalog"],after)
    return {"request": request, "setup": setup, "postgres": observed, "before": before, "after": after,
            "post_catalog": metadata["catalog"], "post_metadata": metadata["table_metadata"],
            "post_database": physical(connection, metadata["catalog"])}


def exclude_views(catalog: dict, capture: dict) -> None:
    """Keep PostgreSQL view columns out of a physical table snapshot even on older inspector versions."""
    identities = {(view["schema"],view["name"]) for view in capture["views"].values()}
    catalog["tables"] = {key: value for key, value in catalog["tables"].items() if (value["schema"],value["name"]) not in identities}


def catalog_matches(value: dict, case: dict) -> bool:
    """Require exact user namespace, column schema and persisted key behavior for newly created tables."""
    actual, requested = value["catalog"], case["request"]["catalog"]
    if corpus_ddl.canonical(actual.get("namespace")) != corpus_ddl.canonical(case["after"]["namespace"]): return False
    if actual["search_path"] != requested["search_path"]: return False
    if stripped_tables(actual["tables"]) != stripped_tables(case["post_catalog"]["tables"]): return False
    for key, semantics in actual.get("table_semantics", {}).items():
        if semantics != case["post_metadata"].get(key): return False
    expected_tables = dict(requested, tables=actual["tables"], table_semantics=actual.get("table_semantics", {}))
    return corpus_ddl.matches(actual, case["before"], case["after"], expected_tables, case["request"]["sql"])


def stripped_tables(tables: dict) -> dict:
    """Source qualifier annotations are binding metadata, not persistent PostgreSQL table properties."""
    return {key: dict(table, columns=[{name: column[name] for name in ("name", "ty", "nullable")}
                                    for column in table["columns"]]) for key, table in tables.items()}


def observation_matches(value: dict, case: dict) -> bool:
    """Compare every outcome's SQL diagnostics or complete rows, user catalog and physical state."""
    expected, outcome = case["postgres"], value["outcome"]
    if expected["status"] == "error":
        return value["status"] == "sql_error" and all(outcome.get(key) == expected.get(key) for key in ("sqlstate", "constraint_name", "column_name"))
    if value["status"] != "succeeded" or outcome["effects"]: return False
    if outcome["command_tag"] != expected["command"] or outcome["affected"] != expected["affected_rows"]: return False
    if rows_key(outcome["rows"]) != rows_key(pg_rows(expected, {})): return False
    if set(outcome["database"]) != set(case["post_database"]): return False
    if any(rows_key(rows) != rows_key(case["post_database"][key]) for key, rows in outcome["database"].items()): return False
    return catalog_matches(outcome, case)


def compare(case: dict, response: dict) -> dict:
    """Require independent PG agreement for every returned complete finite-Alloy behavior."""
    errors = []
    if response.get("status") != "ok" or not response.get("complete") or response.get("validation") != "ALLOY_TABLE_ROUNDTRIP": errors.append("incomplete_execution")
    elif not response["outcomes"] or any(not observation_matches(value, case) for value in response["outcomes"]): errors.append("postgres_outcome_mismatch")
    elif case["postgres"]["status"] == "ok" and not schema_matches(response["schema"], case["postgres"]["columns"], {}): errors.append("schema_mismatch")
    return dict(case, status="failed" if errors else "passed", errors=errors, response=response)


def main() -> None:
    """Maintain one reproducible report and remove the owned isolated PostgreSQL cluster."""
    with TemporaryPostgres() as database:
        with PgConnection(database) as connection:
            evidence = [prepare(database, connection, i, setup, sql) for i, (setup, sql) in enumerate(cases())]
        version, directory = database.execute("SELECT version()").strip(), database.directory
    requests = [case["request"] for case in evidence]
    responses = index_responses(requests, execute(requests, timeout=240))
    results = [compare(case, responses[case["request"]["id"]]) for case in evidence]
    failures = [case["request"]["id"] for case in results if case["status"] != "passed"]
    report = {"status": "failed" if failures else "passed", "cases": len(results), "failures": failures,
              "results": results, "postgres_version": version, "cluster_removed": not directory.exists(),
              "cli_sha256": digest(CLI.read_bytes()), "scope": "Complete non-system namespace; internal storage/OIDs and arbitrary external DDL history are outside the admitted catalog projection."}
    write_json(RESULTS / "ddl.json", report)
    print(json.dumps({key: value for key, value in report.items() if key != "results"}))
    if failures: raise SystemExit(1)


if __name__ == "__main__":
    main()

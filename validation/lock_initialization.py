"""Actual PG relation-lock initialization probes separate planning footprint from row demand."""
import argparse
import json

from corpus_runtime import checked, describe
from corpus_semantics import observation_matches, schema_matches
from oracle_contract import index_responses
from rejections import execute
from run import CLI, digest
from libpq_client import PgConnection
from pg_runtime import TemporaryPostgres
from run import RESULTS, write_json


# These supported profiles are permanent regression gates, never optional coverage credit.
ADMITTED = {"unused", "unused_chain", "plain_limit_zero", "case_false", "unused_lock_free",
            "plain_false", "empty_locked_cte", "used_locked_cte"}
BOUNDARIES = {
    "unused_materialized": {"unsupported.cte_materialization"},
    "unused_not_materialized": {"unsupported.cte_materialization"},
    "where_false": {"unsupported.cte_consumer_demand"},
    "cte_limit_zero": {"unsupported.cte_consumer_demand"},
    "empty_cross": {"unsupported.cte_consumer_demand"},
    "empty_left": {"unsupported.cte_consumer_demand"},
    "empty_in": {"unsupported.cte_consumer_demand"},
    "derived_false": {"unsupported.nested_lock_demand"},
    "exists_false": {"unsupported.exists_shape"},
    "locked_outer_select": {"unsupported.lock_origin"},
}


def cases() -> list[tuple[str, str]]:
    """Distinguish unused definitions, pruned consumers, empty joins and row-short-circuiting contexts."""
    prefix = "WITH q AS (SELECT * FROM t FOR UPDATE) "
    return [("unused", prefix + "SELECT 1"),
        ("unused_materialized", "WITH q AS MATERIALIZED (SELECT * FROM t FOR UPDATE) SELECT 1"),
        ("unused_not_materialized", "WITH q AS NOT MATERIALIZED (SELECT * FROM t FOR UPDATE) SELECT 1"),
        ("where_false", prefix + "SELECT * FROM q WHERE false"),
        ("empty_cross", prefix + "SELECT * FROM e CROSS JOIN q"),
        ("empty_left", prefix + "SELECT * FROM e LEFT JOIN q ON true"),
        ("empty_in", prefix + "SELECT * FROM e WHERE k IN(SELECT k FROM q)"),
        ("unused_chain", "WITH q AS(SELECT * FROM t FOR UPDATE),r AS(SELECT * FROM q) SELECT 1"),
        ("cte_limit_zero", prefix + "SELECT * FROM q LIMIT 0"),
        ("plain_limit_zero", "SELECT * FROM t LIMIT 0 FOR UPDATE"),
        ("derived_false", "SELECT * FROM(SELECT * FROM t FOR UPDATE)q WHERE false"),
        ("exists_false", "SELECT EXISTS(SELECT * FROM t WHERE false FOR UPDATE)"),
        ("case_false", "SELECT CASE WHEN false THEN(SELECT k FROM t FOR UPDATE) ELSE 0 END"),
        ("unused_lock_free", "WITH q AS(SELECT * FROM t) SELECT 1"),
        ("plain_false", "SELECT * FROM t WHERE false"),
        ("empty_locked_cte", "WITH q AS(SELECT * FROM e FOR UPDATE) SELECT * FROM q"),
        ("used_locked_cte", prefix + "SELECT * FROM q"),
        ("locked_outer_select", "WITH q AS(SELECT * FROM t) SELECT * FROM q FOR UPDATE")]


def locks(connection) -> list:
    """Reading pg_locks references only system relations and cannot itself lock the test tables."""
    sql = """SELECT c.relname,l.mode FROM pg_locks l JOIN pg_class c ON c.oid=l.relation
      JOIN pg_namespace n ON n.oid=c.relnamespace WHERE l.pid=pg_backend_pid()
      AND n.nspname='public' AND c.relname IN('t','e') AND l.granted ORDER BY c.relname,l.mode"""
    return checked(connection, sql)["rows"]


def trial(connection, observer, sql: str, explain: bool) -> dict:
    """Start with no target locks; EXPLAIN and execution occupy separate transactions."""
    checked(connection, "BEGIN")
    before = locks(connection)
    if before: raise AssertionError("Target relation locks leaked into initial snapshot")
    result = describe(connection, connection.query(("EXPLAIN " if explain else "") + sql))
    after = locks(connection) if result["status"] == "ok" else None
    held = row_held(observer) if result["status"] == "ok" and not explain else False
    checked(connection, "ROLLBACK")
    if locks(connection): raise AssertionError("Target lock survived full rollback")
    return {"before": before, "result": result, "after": after, "row_t_held": held}


def row_held(observer) -> bool:
    """A separate transaction probes the one physical tuple and always releases its own acquisition."""
    checked(observer, "BEGIN")
    result = observer.query("SELECT k FROM t WHERE k=1 FOR UPDATE NOWAIT")
    checked(observer, "ROLLBACK")
    if result["status"] == "ok": return False
    if result["sqlstate"] == "55P03": return True
    raise AssertionError("Unexpected independent row ownership probe failure")


def request(name: str, sql: str) -> dict:
    """Declare only the authentic input tables and an empty fixed ownership snapshot."""
    tables = {table: {"name": table, "schema": "public", "columns": [
        {"name": "k", "ty": "int32", "nullable": True}]} for table in ("t", "e")}
    return {"id": "initialization/" + name, "sql": sql, "mode": "compare", "parameters": {},
        "catalog": {"search_path": ["public"], "tables": tables},
        "database": {"t": [[{"type": "int32", "value": 1}]], "e": []},
        "execution": {"max_rows": 256, "max_outcomes": 256, "settings": {}, "table_metadata": {}, "locks": {"complete": True, "transaction": "self", "held_rows": [], "row_share_conflicts": []}}}


def compare(record: dict, response: dict) -> dict:
    """Check every complete outcome; refusals are uncovered cases rather than semantic passes."""
    if response.get("status") != "ok":
        accepted = response.get("code") in BOUNDARIES.get(record["id"], set())
        return {"status": "boundary_verified" if accepted else "failed", "response": response,
                "errors": [] if accepted else ["unexpected_refusal"]}
    observed = record["execution"]
    wire = request(record["id"], record["sql"])
    columns = [{"name": "k", "type": "integer", "type_oid": 23}]
    after = {"database": {"t": {"columns": columns, "rows": [["1"]]}, "e": {"columns": columns, "rows": []}}}
    events = [{"kind": "table_lock", "table": name, "mode": "row_share"}
              for name, mode in observed["after"] if mode == "RowShareLock"]
    if observed["row_t_held"]: events.append({"kind": "row_lock", "table": "t", "occurrence": 0, "strength": "update"})
    good = response.get("complete") and response.get("validation") == "ALLOY_TABLE_ROUNDTRIP" and bool(response.get("outcomes"))
    for outcome in response.get("outcomes", []):
        good &= observation_matches(outcome, observed["result"], after, wire["catalog"], {}, False)
        good &= outcome["outcome"]["effects"] == events
    good &= schema_matches(response["schema"], observed["result"]["columns"], {})
    return {"status": "passed" if good else "failed", "response": response}


def observe_all(database) -> list[dict]:
    """Keep a separate observer session and isolate EXPLAIN from the actual statement transaction."""
    with PgConnection(database) as connection, PgConnection(database) as observer:
        checked(connection, "CREATE TABLE t(k int)")
        checked(connection, "CREATE TABLE e(k int)")
        checked(connection, "INSERT INTO t VALUES(1)")
        return [{"id": name, "sql": sql, "planning": trial(connection, observer, sql, True),
                 "execution": trial(connection, observer, sql, False)} for name, sql in cases()]


def main() -> None:
    """Record observed relation modes and optionally replay the current compiled Rust/Alloy profile."""
    parser = argparse.ArgumentParser()
    parser.add_argument("--engine", action="store_true")
    options = parser.parse_args()
    with TemporaryPostgres() as database:
        records = observe_all(database)
        if {record["id"] for record in records} != ADMITTED | set(BOUNDARIES): raise AssertionError("Unclassified gate case")
        version, directory = database.execute("SELECT version();").strip(), database.directory
    if options.engine:
        submitted = [request(name, sql) for name, sql in cases()]
        responses = index_responses(submitted, execute(submitted, timeout=240))
        for record in records: record["engine"] = compare(record, responses["initialization/" + record["id"]])
    failures = [record["id"] for record in records if record["planning"]["result"]["status"] != "ok" or
                record["execution"]["result"]["status"] != "ok" or record.get("engine", {}).get("status") == "failed"]
    covered = sum(record.get("engine", {}).get("status") == "passed" for record in records)
    report = {"status": "failed" if failures else ("passed" if options.engine else "observed"), "cases": len(records), "records": records,
        "failures": failures, "postgres_version": version, "cluster_removed": not directory.exists(),
        "engine_matched": covered, "engine_uncovered": len(records) - covered,
        "expected_admitted": sorted(ADMITTED), "allowed_boundary_codes": {key: sorted(value) for key, value in BOUNDARIES.items()},
        "cli_sha256": digest(CLI.read_bytes()) if options.engine else None,
        "scope": "All admitted outcomes checked; RowShare and tuple ownership only. AccessShare/RowExclusive, wait completion and concurrent changes are not modeled. Separate transactions prevent EXPLAIN contamination."}
    write_json(RESULTS / "lock_initialization.json", report)
    print(json.dumps({key: value for key, value in report.items() if key != "records"}))
    if failures: raise SystemExit(1)


if __name__ == "__main__":
    main()

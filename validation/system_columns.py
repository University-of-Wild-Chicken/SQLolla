"""Observe bounded heap-header behavior in actual PG16; this is not engine execution coverage."""
import json

from corpus_runtime import checked, describe
from libpq_client import PgConnection
from pg_runtime import TemporaryPostgres
from run import CLI, RESULTS, digest, write_json
from corpus_tuples import capture, header_bag, replay
from corpus_wire import typed_rows
from cte import compare as compare_data
from oracle_contract import index_responses
from rejections import execute
from copy import deepcopy


def cases() -> list[tuple]:
    """Include a plain UPDATE with an existing own lock, falsifying a command-kind xmax shortcut."""
    returning = " RETURNING k,v,xmin,xmax,(xmax<>0) AS nonzero_xmax"
    return [
        ("fresh", "INSERT INTO t VALUES(2,20)" + returning, False),
        ("conflict_update", "INSERT INTO t VALUES(1,20) ON CONFLICT(k) DO UPDATE SET v=excluded.v" + returning, False),
        ("changed_key", "INSERT INTO t VALUES(1,20) ON CONFLICT(k) DO UPDATE SET k=3" + returning, False),
        ("ordinary_update", "UPDATE t SET v=20 WHERE k=1" + returning, False),
        ("own_locked_update", "UPDATE t SET v=20 WHERE k=1" + returning, True),
        ("skip_update", "INSERT INTO t VALUES(1,20) ON CONFLICT(k) DO UPDATE SET v=20 WHERE false" + returning, False),
        ("do_nothing", "INSERT INTO t VALUES(1,20) ON CONFLICT(k) DO NOTHING" + returning, False),
        ("mixed", "INSERT INTO t VALUES(2,20),(1,30) ON CONFLICT(k) DO UPDATE SET v=excluded.v" + returning, False),
        ("later_check_error", "INSERT INTO t VALUES(1,20),(2,-1) ON CONFLICT(k) DO UPDATE SET v=excluded.v" + returning, False),
        ("returning_error", "INSERT INTO t VALUES(1,20) ON CONFLICT(k) DO UPDATE SET v=20 RETURNING 1/(v-20)", False),
    ]


def snapshot(connection) -> dict:
    """Keep user data and raw header identities separately, with stable unique-key ordering."""
    rows = checked(connection, "SELECT k,v,xmin::text,xmax::text FROM t ORDER BY k")["rows"]
    return {"rows": [row[:2] for row in rows], "headers": [{"xmin": int(row[2]), "raw_xmax": int(row[3])} for row in rows]}


def observe(connection, case: tuple) -> dict:
    """Allocate the top-level writer before the query, without deriving input from RETURNING."""
    name, sql, own_lock = case
    checked(connection, "TRUNCATE t")
    checked(connection, "INSERT INTO t VALUES(1,10)")
    checked(connection, "BEGIN")
    xid = int(checked(connection, "SELECT pg_current_xact_id()::text")["rows"][0][0])
    if not 3 <= xid < 2**32: raise AssertionError("Feasibility profile excludes xid wraparound")
    if own_lock: checked(connection, "SELECT * FROM t FOR UPDATE")
    before = snapshot(connection)
    captured = capture(connection, catalog()["tables"], xid, ("t",) if own_lock else ())
    result = describe(connection, connection.query(sql))
    after = snapshot(connection) if result["status"] == "ok" else None
    checked(connection, "ROLLBACK")
    rollback = snapshot(connection)
    return {"id": name, "sql": sql, "write_xid_before_query": xid, "own_lock_before": own_lock,
            "before": before, "result": result, "after": after, "after_rollback": rollback, "captured": captured}


def verify(record: dict) -> None:
    """Assert header invariants and explicitly retain physical residuals after data rollback."""
    xid, name, result = record["write_xid_before_query"], record["id"], record["result"]
    if record["after_rollback"]["rows"] != record["before"]["rows"]: raise AssertionError(record)
    touched = name not in {"fresh", "do_nothing"}
    expected = xid if touched else 0
    if record["after_rollback"]["headers"][0]["raw_xmax"] != expected: raise AssertionError(record)
    if name in {"later_check_error", "returning_error"}:
        expected_code = "23514" if name == "later_check_error" else "22012"
        if result.get("sqlstate") != expected_code or result.get("exposed_rows") != []: raise AssertionError(record)
        return
    if result["status"] != "ok": raise AssertionError(record)
    for row in result["rows"]:
        own = name in {"conflict_update", "changed_key", "own_locked_update"} or (name == "mixed" and row[0] == "1")
        if int(row[2]) != xid or int(row[3]) != (xid if own else 0): raise AssertionError(record)
        if row[4] != ("t" if own else "f"): raise AssertionError(record)
    if name == "skip_update" and record["after"]["headers"][0]["raw_xmax"] != xid: raise AssertionError(record)


def subtransaction_boundary(connection) -> dict:
    """Demonstrate why a savepoint's actual write xid cannot be guessed from the top-level xid."""
    checked(connection, "CREATE TABLE xid_capture(v int)")
    checked(connection, "BEGIN")
    top = int(checked(connection, "SELECT pg_current_xact_id()::text")["rows"][0][0])
    checked(connection, "SAVEPOINT boundary")
    actual = int(checked(connection, "INSERT INTO xid_capture VALUES(1) RETURNING xmin::text")["rows"][0][0])
    returned = checked(connection, "UPDATE t SET v=20 RETURNING xmin::text")["rows"][0][0]
    checked(connection, "ROLLBACK")
    if actual == top or int(returned) != actual: raise AssertionError((top, actual, returned))
    return {"top_xid": top, "captured_write_subxid": actual, "returned_xmin": returned,
            "scope": "Excluded from the initial top-level profile; capture probe is a context setup write, not a query answer lookup."}


def catalog() -> dict:
    """Mirror the actual fixture DDL, including nullable CHECK input and its primary-key nullability."""
    return {"search_path": ["public"], "tables": {"t": {"schema": "public", "name": "t", "columns": [
        {"name": "k", "ty": "int32", "nullable": False}, {"name": "v", "ty": "int32", "nullable": True}]}}}


def request(record: dict) -> dict:
    """Bind exact pre-query data, constraints and independently captured version context."""
    return {"id": "system_columns/" + record["id"], "sql": record["sql"], "mode": "compare",
            "catalog": catalog(), "parameters": {},
            "database": {"t": typed_rows(record["captured"]["relations"]["t"]["rows"], ["int32", "int32"])},
            "execution": {"max_rows": 256, "max_outcomes": 256, "settings": {}, "tuples": record["captured"]["context"],
            "table_metadata": {"t": {"complete": True, "primary_key": {"name": "t_pkey", "columns": [0]},
            "checks": [{"name": "t_v_check", "sql": "v >= 0", "parser": None}]}}}}


def compare(record: dict, submitted: dict, response: dict) -> dict:
    """Compare returned values/types, full row poststate and physical journal/abort observations."""
    state = record["after"] if record["result"]["status"] == "ok" else record["after_rollback"]
    relation = deepcopy(record["captured"]["relations"]["t"])
    relation["rows"] = state["rows"]
    stripped = deepcopy(response)
    for outcome in stripped.get("outcomes", []): outcome["outcome"]["effects"] = []
    oracle = {"result": record["result"], "after": {"database": {"t": relation}}, "observed_phase": "statement"}
    result = compare_data(submitted, oracle, stripped)
    for outcome in response.get("outcomes", []):
        try:
            events = outcome["outcome"]["effects"]
            if replay(record["before"], events) != header_bag(state): result["errors"].append("physical_header_mismatch")
            if replay(record["before"], events, True) != header_bag(record["after_rollback"]): result["errors"].append("abort_header_mismatch")
            if outcome["status"] == "sql_error" and any(event["observation"]["transition"]["phase"] != "aborted_residual" for event in events): result["errors"].append("unrolled_failure_journal")
        except (ValueError, KeyError, TypeError) as failure: result["errors"].append(str(failure))
    result["status"] = "failed" if result["errors"] else "passed"
    result["response"] = response
    return result


def main() -> None:
    """Record real nonempty observations and their bounded source-backed interpretation."""
    with TemporaryPostgres() as database:
        with PgConnection(database) as connection:
            checked(connection, "CREATE TABLE t(k int PRIMARY KEY,v int CHECK(v>=0))")
            records = [observe(connection, case) for case in cases()]
            for record in records: verify(record)
            boundary = subtransaction_boundary(connection)
        version, directory = database.execute("SELECT version();").strip(), database.directory
    admitted = [record for record in records if record["sql"].startswith("INSERT")]
    requests = [request(record) for record in admitted]
    responses = index_responses(requests, execute(requests, timeout=180))
    compared = [compare(record, wire, responses[wire["id"]]) for record, wire in zip(admitted, requests)]
    failures = [record["id"] for record in compared if record["status"] != "passed"]
    report = {"status": "failed" if failures else "passed", "engine_execution_coverage": True,
              "compared_insert_cases": len(compared), "comparisons": compared, "failures": failures,
              "cli_sha256": digest(CLI.read_bytes()),
              "profile": "exclusive top-level writer; no concurrent actors, MultiXact, triggers or xid wraparound",
              "postgres_version": version, "cluster_removed": not directory.exists(), "records": records,
              "subtransaction_boundary": boundary, "cases": len(records),
              "sources": ["https://www.postgresql.org/docs/16/ddl-system-columns.html",
                "https://github.com/postgres/postgres/blob/REL_16_16/src/backend/access/heap/heapam.c#L3317-L3366",
                "https://github.com/postgres/postgres/blob/REL_16_16/src/backend/executor/nodeModifyTable.c#L2377"]}
    write_json(RESULTS / "system_columns.json", report)
    print(json.dumps({key: value for key, value in report.items() if key not in {"records", "subtransaction_boundary", "sources", "comparisons"}}))

    if failures: raise SystemExit(1)


if __name__ == "__main__":
    main()

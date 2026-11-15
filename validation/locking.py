"""Observe actual row-lock ownership with independent PostgreSQL sessions and NOWAIT probes."""
from itertools import product
import json

from cases import fixture, table
from corpus_runtime import checked, describe
from corpus_semantics import pg_rows, rows_key
from libpq_client import PgConnection
from oracle_contract import index_responses, top_level_ordered
from pg_runtime import TemporaryPostgres
from rejections import execute
from run import CLI, RESULTS, digest, write_json

MODES = {"key_share": "KEY SHARE", "share": "SHARE", "no_key_update": "NO KEY UPDATE", "update": "UPDATE"}


def case(label: str, sql: str, rival: str | None = None, own: str | None = None) -> dict:
    """Supply the exact synthetic initial ownership rather than silently assuming no conflicts."""
    request = fixture("locking/" + label, sql, {"r": table([("id", "int32"), ("x", "int32")], [[1, 0], [2, 10], [3, 20]])}, ["int32"], "locking")["request"]
    held = [{"table": "public.r", "occurrence": 1, "owner": owner, "strength": strength}
            for owner, strength in [("rival", rival), ("subject", own)] if strength is not None]
    request["execution"] = {"max_outcomes": 256, "max_rows": 256, "settings": {},
        "locks": {"complete": True, "transaction": "subject", "held_rows": held, "row_share_conflicts": []}}
    return {"request": request, "rival": rival, "own": own}


def cases() -> list[dict]:
    """Exercise all16compatibilitypairs plus skip/pagination/reentrance/empty-source boundaries."""
    rows = [case(wanted + "/" + held, "SELECT id FROM r ORDER BY id FOR " + MODES[wanted] + " NOWAIT", held)
            for wanted, held in product(MODES, repeat=2)]
    rows.extend([case("skip", "SELECT id FROM r ORDER BY id LIMIT 1 OFFSET 1 FOR UPDATE SKIP LOCKED", "update"),
                 case("offset", "SELECT id FROM r ORDER BY id LIMIT 1 OFFSET 1 FOR UPDATE"),
                 case("zero", "SELECT id FROM r ORDER BY id LIMIT 0 OFFSET 2 FOR UPDATE"),
                 case("zero_expression", "SELECT 1/(id-1) FROM r LIMIT 0 FOR UPDATE"),
                 case("none", "SELECT id FROM r WHERE id=0 FOR SHARE"),
                 case("reentrant", "SELECT id FROM r ORDER BY id FOR KEY SHARE", own="update")])
    return rows


def begin(connection, held: str | None) -> None:
    """Acquire an optional preexisting row lock before the statement savepoint."""
    checked(connection, "BEGIN")
    if held: checked(connection, "SELECT id FROM r WHERE id=2 FOR " + MODES[held])
    checked(connection, "SAVEPOINT target_statement")


def blocked(observer, identifier: int, mode: str) -> bool:
    """A separate transaction probes one mode and always releases its own trial lock."""
    checked(observer, "BEGIN")
    result = observer.query(f"SELECT id FROM r WHERE id={identifier} FOR {MODES[mode]} NOWAIT")
    checked(observer, "ROLLBACK")
    if result["status"] == "error" and result["sqlstate"] != "55P03": raise ValueError(result)
    return result["status"] == "error"


def observed_locks(observer) -> dict:
    """Identify held strength by all four independent compatibility probes per row."""
    patterns = {frozenset(["update"]): "key_share", frozenset(["no_key_update", "update"]): "share",
                frozenset(["share", "no_key_update", "update"]): "no_key_update", frozenset(MODES): "update"}
    held = {}
    for identifier in range(1, 4):
        pattern = frozenset(mode for mode in MODES if blocked(observer, identifier, mode))
        if pattern: held[identifier - 1] = patterns[pattern]
    return held


def observe(subject, rival, observer, fixture: dict) -> dict:
    """Keep the subject transaction open while independently inspecting resulting ownership."""
    begin(rival, fixture["rival"])
    begin(subject, fixture["own"])
    result = subject.query(fixture["request"]["sql"])
    if result["status"] == "error": checked(subject, "ROLLBACK TO target_statement")
    result = describe(subject, result)
    checked(rival, "ROLLBACK")
    rows = observed_locks(observer)
    relation = checked(subject, "SELECT count(*) FROM pg_locks WHERE pid=pg_backend_pid() AND relation='r'::regclass AND mode='RowShareLock' AND granted")
    checked(subject, "ROLLBACK")
    return {"result": result, "held_rows": rows, "row_share": relation["rows"][0][0] == "1"}


def matches(fixture: dict, expected: dict, response: dict) -> bool:
    """Validate outputs and held-lock observations independently of engine-claimed success."""
    if response.get("status") != "ok" or response.get("validation") != "ALLOY_TABLE_ROUNDTRIP": return False
    ordered = top_level_ordered(fixture["request"]["sql"])
    for outcome in response["outcomes"]:
        value, reference = outcome["outcome"], expected["result"]
        if reference["status"] == "error":
            if outcome["status"] == "sql_error" and value["sqlstate"] == reference["sqlstate"]: return True
        elif outcome["status"] == "succeeded":
            if value["ordering"] != ("ordered" if ordered else "bag"): continue
            if rows_key(value["rows"], ordered) != rows_key(pg_rows(reference, {}), ordered): continue
            held = {event["occurrence"]: event["strength"] for event in value["effects"] if event["kind"] == "row_lock"}
            relations = [event for event in value["effects"] if event["kind"] == "table_lock"]
            if held == expected["held_rows"] and relations == [{"kind": "table_lock", "table": "public.r", "mode": "row_share"}] and expected["row_share"]: return True
    return False


def main() -> None:
    """Run a disposable three-session oracle and the compiled CLI under the same explicit locks."""
    fixtures = cases()
    responses = index_responses([case["request"] for case in fixtures], execute([case["request"] for case in fixtures]))
    with TemporaryPostgres() as database:
        database.execute("CREATE TABLE r(id integer PRIMARY KEY,x integer);INSERT INTO r VALUES(1,0),(2,10),(3,20);")
        with PgConnection(database) as subject, PgConnection(database) as rival, PgConnection(database) as observer:
            results = []
            for fixture in fixtures:
                expected = observe(subject, rival, observer, fixture)
                response = responses[fixture["request"]["id"]]
                results.append({"id": fixture["request"]["id"], "status": "passed" if matches(fixture, expected, response) else "failed", "postgres": expected, "response": response})
        version = database.execute("SELECT version();").strip()
    failures = [row["id"] for row in results if row["status"] == "failed"]
    report = {"status": "failed" if failures else "passed", "cases": len(results), "failures": failures,
              "version": version, "cli_sha256": digest(CLI.read_bytes()), "scope": "Fixed competing row-lock snapshot; no concurrent row updates, wait completion or isolation-schedule claim.", "results": results}
    write_json(RESULTS / "locking.json", report)
    print(json.dumps({key: value for key, value in report.items() if key != "results"}))
    if failures: raise SystemExit(1)


if __name__ == "__main__":
    main()

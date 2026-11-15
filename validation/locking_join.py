"""Independently probe every joined base-row lock and compare complete outputs and ownership."""
import json

from cases import fixture, table
import corpus_locks
from corpus_runtime import checked, describe, snapshot
from corpus_semantics import effects_key, observation_matches, schema_matches
from libpq_client import PgConnection
from oracle_contract import index_responses, top_level_ordered
from pg_runtime import TemporaryPostgres
from rejections import execute
from run import CLI, RESULTS, digest, write_json

INPUTS = {"r": [[1, 1], [2, 2], [3, None]], "s": [[1, 1], [2, 1], [3, 3]], "t": [[1, 1], [2, 2]]}


def requests() -> list[dict]:
    """Exercise duplicate join matches, empty joins, all row modes and private-column isolation."""
    sources = {name: table([("id", "int32"), ("x", "int32")], rows) for name, rows in INPUTS.items()}
    joins = ["SELECT r.id,s.id FROM r JOIN s ON r.x=s.x ORDER BY r.id,s.id FOR " + mode
             for mode in corpus_locks.MODES.values()]
    joins += ["SELECT r.x FROM r JOIN s ON r.x=s.x FOR UPDATE",
              "SELECT r.id,s.id FROM r JOIN s ON r.x=s.x WHERE r.id=2 FOR UPDATE",
              "SELECT r.id,s.id,t.id FROM r JOIN s ON r.x=s.x JOIN t ON t.x=s.x FOR UPDATE",
              "SELECT r.id,s.id FROM r CROSS JOIN s WHERE r.id=2 AND s.id=1 FOR SHARE",
              "SELECT 1/(r.id-1) FROM r JOIN s ON r.x=s.x LIMIT 0 FOR UPDATE"]
    result = []
    for index, sql in enumerate(joins):
        request = fixture("joined-locks/" + str(index), sql, sources, [], "joined_locks")["request"]
        request["execution"] = {"max_rows": 256, "max_outcomes": 256, "settings": {},
            "locks": {"complete": True, "transaction": "industrial_subject", "held_rows": [], "row_share_conflicts": []}}
        result.append(request)
    return result


def observe(subject, observer, request: dict) -> dict:
    """Inspect physical ownership while the original SELECT transaction remains open."""
    checked(subject, "BEGIN")
    before = snapshot(subject)
    physical = corpus_locks.identities(subject, before)
    checked(subject, "SAVEPOINT observed_query")
    result = subject.query(request["sql"])
    if result["status"] == "error": checked(subject, "ROLLBACK TO observed_query")
    result = describe(subject, result)
    after = snapshot(subject)
    ownership = corpus_locks.observe(subject, observer, physical)
    checked(subject, "ROLLBACK")
    return {"result": result, "before": before, "after": after, "ownership": ownership}


def compare(request: dict, expected: dict, response: dict) -> list[str]:
    """Require every returned alternative to satisfy independently measured bag and ownership data."""
    if response.get("status") != "ok" or response.get("validation") != "ALLOY_TABLE_ROUNDTRIP" or not response.get("complete"):
        return ["incomplete_execution"]
    if not response["outcomes"]: return ["missing_outcomes"]
    ordered = top_level_ordered(request["sql"])
    for outcome in response["outcomes"]:
        if not observation_matches(outcome, expected["result"], expected["after"], request["catalog"], {}, ordered): return ["postgres_outcome_mismatch"]
        if effects_key(outcome["outcome"]["effects"]) != effects_key(expected["ownership"]["effects"]): return ["lock_ownership_mismatch"]
    if expected["result"]["status"] == "ok" and not schema_matches(response["schema"], expected["result"]["columns"], {}): return ["schema_mismatch"]
    return []


def main() -> None:
    """Run the pinned PostgreSQL multi-session oracle and actual Rust/Alloy compiler together."""
    submitted = requests()
    responses = index_responses(submitted, execute(submitted))
    with TemporaryPostgres() as database:
        database.execute("CREATE TABLE r(id integer,x integer);CREATE TABLE s(id integer,x integer);CREATE TABLE t(id integer,x integer);INSERT INTO r VALUES(1,1),(2,2),(3,NULL);INSERT INTO s VALUES(1,1),(2,1),(3,3);INSERT INTO t VALUES(1,1),(2,2);")
        with PgConnection(database) as subject, PgConnection(database) as observer:
            results = []
            for request in submitted:
                expected = observe(subject, observer, request)
                errors = compare(request, expected, responses[request["id"]])
                results.append({"id": request["id"], "sql": request["sql"], "errors": errors, "postgres": expected, "response": responses[request["id"]]})
        version, directory = database.execute("SELECT version();").strip(), database.directory
    failures = [row["id"] for row in results if row["errors"]]
    report = {"status": "failed" if failures else "passed", "cases": len(results), "failures": failures,
        "results": results, "cli_sha256": digest(CLI.read_bytes()), "postgres_version": version, "cluster_removed": not directory.exists(),
        "scope": "Unpaginated distinct-origin inner joins under an explicit nonconflicting lock snapshot; partial joined conflict acquisition remains refused."}
    write_json(RESULTS / "locking_join.json", report)
    print(json.dumps({key: value for key, value in report.items() if key != "results"}))
    if failures: raise SystemExit(1)


if __name__ == "__main__":
    main()

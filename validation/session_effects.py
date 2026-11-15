"""Independent PostgreSQL notification delivery and transaction advisory ownership oracle."""
import json
import argparse
from pathlib import Path

from corpus_runtime import checked, describe, quote_identifier
from effect_listener import configure, delivered
from libpq_client import PgConnection
from pg_runtime import TemporaryPostgres
from run import CLI, RESULTS, digest, write_json
from session_compare import compare_all


def notify_cases() -> list[dict]:
    """Keep channel/payload byte boundaries, duplicate order, NULL and transaction end explicit."""
    values = [("sqlolla_events", "one", True), ("sqlolla_events", "one", False), ("sqlolla_events", None, True),
              (None, "one", True), ("", "one", True), ("x" * 63, "", True), ("x" * 64, "", True),
              ("sqlolla_events", "x" * 7999, True), ("sqlolla_events", "x" * 8000, True),
              ("sqlolla_events", "é" * 3999, True), ("sqlolla_events", "é" * 4000, True)]
    result = [{"id": "notify/" + str(index), "calls": [(channel, payload)], "commit": commit}
              for index, (channel, payload, commit) in enumerate(values)]
    result += [{"id": "notify/duplicates", "calls": [("sqlolla_events", "a"), ("sqlolla_events", "b"), ("sqlolla_events", "a")], "commit": True},
               {"id": "notify/later_error", "calls": [("sqlolla_events", "a"), ("", "b")], "commit": True}]
    return result


def notifications(database) -> list[dict]:
    """Use an independent listener; pending invocations must not be mislabeled delivered events."""
    result = []
    with PgConnection(database) as listener, PgConnection(database) as sender:
        configure(listener)
        for channel in ["sqlolla_events", "x" * 63]: checked(listener, "LISTEN " + quote_identifier(channel))
        for case in notify_cases():
            delivered(listener)
            checked(sender, "BEGIN")
            sql = "SELECT " + ",".join(f"pg_notify(${2*i+1},${2*i+2})" for i in range(len(case["calls"])))
            parameters = [{"oid": 25, "value": value} for pair in case["calls"] for value in pair]
            observation = describe(sender, sender.query(sql, parameters))
            before = delivered(listener, 0.03)
            end = sender.query("COMMIT" if case["commit"] else "ROLLBACK")
            after = delivered(listener)
            companion = commit_companion(sender, listener, sql, parameters) if not case["commit"] else after
            result.append(dict(case, sql=sql, postgres=observation, before_end=before, end=end,
                               after_end=after, committed_delivery=companion))
    return result


def commit_companion(sender, listener, sql: str, parameters: list) -> list:
    """Independently observe delivery for a rollback case using identical input in a new transaction."""
    checked(sender, "BEGIN")
    observed = sender.query(sql, parameters)
    checked(sender, "COMMIT")
    if observed["status"] != "ok": raise ValueError("Successful rollback companion unexpectedly failed")
    return delivered(listener)


def held(connection) -> list[list]:
    """Observe the exact currently owned advisory keys through pg_locks, including namespace tags."""
    return checked(connection, "SELECT classid::text,objid::text,objsubid::text,mode,granted::text FROM pg_locks WHERE pid=pg_backend_pid() AND locktype='advisory' ORDER BY classid,objid,objsubid,mode")["rows"]


def advisory_cases() -> list[str]:
    """Bigint/pair namespaces, signed keys, NULL and repeated acquisitions stay distinct."""
    return ["SELECT pg_advisory_xact_lock(1)", "SELECT pg_advisory_xact_lock(-1::bigint)",
            "SELECT pg_advisory_xact_lock(1,2)", "SELECT pg_advisory_xact_lock(-1,-2)",
            "SELECT pg_advisory_xact_lock(NULL)", "SELECT pg_advisory_xact_lock(1),pg_advisory_xact_lock(1)",
            "SELECT pg_advisory_xact_lock_shared(1)",
            "SELECT pg_advisory_xact_lock(1),pg_advisory_xact_lock_shared(1)",
            "SELECT pg_advisory_xact_lock_shared(1),pg_advisory_xact_lock(1)",
            "SELECT pg_advisory_xact_lock(NULL) = pg_advisory_xact_lock(NULL)",
            "SELECT DISTINCT pg_advisory_xact_lock(NULL)",
            "SELECT pg_advisory_xact_lock(NULL) ORDER BY 1", "SELECT pg_advisory_xact_lock(NULL)::text",
            "SELECT CASE WHEN false THEN pg_advisory_xact_lock(1) END", "SELECT pg_advisory_xact_lock(true)"]


def advisory(database) -> list[dict]:
    """Capture held ownership inside each transaction and independently verify release at both endings."""
    result = []
    with PgConnection(database) as connection:
        for index, sql in enumerate(advisory_cases()):
            for commit in [False, True]:
                checked(connection, "BEGIN")
                before = held(connection)
                observation = describe(connection, connection.query(sql))
                after = held(connection) if observation["status"] == "ok" else None
                end = connection.query("COMMIT" if commit else "ROLLBACK")
                result.append({"id": f"advisory/{index}/{commit}", "sql": sql, "commit": commit, "before": before,
                               "postgres": observation, "after": after, "end": end, "after_end": held(connection)})
    return result


def conflict(database) -> dict:
    """A real second owner demonstrates conflict; a timeout is an observation, not a guessed model outcome."""
    with PgConnection(database) as holder, PgConnection(database) as contender:
        checked(holder, "BEGIN")
        checked(contender, "BEGIN")
        checked(holder, "SELECT pg_advisory_xact_lock(7)")
        result = {"holder": held(holder), "try": describe(contender, checked(contender, "SELECT pg_try_advisory_xact_lock(7)"))}
        checked(contender, "SET LOCAL lock_timeout='50ms'")
        result["blocking_with_explicit_timeout"] = contender.query("SELECT pg_advisory_xact_lock(7)")
        checked(contender, "ROLLBACK")
        checked(holder, "ROLLBACK")
        result["after_rollback"] = held(holder)
        return result


def main() -> None:
    """Compare actual values, transactional ownership and commit delivery with complete engine outcomes."""
    parser = argparse.ArgumentParser()
    parser.add_argument("--oracle-only", action="store_true")
    options = parser.parse_args()
    with TemporaryPostgres() as database:
        result = {"notifications": notifications(database), "advisory": advisory(database), "conflict": conflict(database)}
        version, directory = database.execute("SELECT version()").strip(), database.directory
    comparisons = [] if options.oracle_only else compare_all(result)
    failures = [case["id"] for case in comparisons if case["status"] == "failed"]
    report = {"status": "oracle_only" if options.oracle_only else "failed" if failures else "passed",
              "comparisons": len(comparisons), "failures": failures, "results": comparisons,
              "postgres_version": version, "cluster_removed": not directory.exists(),
              "harness_sha256": digest(Path(__file__).read_bytes()), "observations": result,
              "cli_sha256": None if options.oracle_only else digest(CLI.read_bytes()),
              "contracts": ["contracts/m2/SQLollaM2/SessionEffects.lean"],
              "sources": ["https://www.postgresql.org/docs/16/sql-notify.html", "https://www.postgresql.org/docs/16/functions-admin.html"],
              "claim": "Actual PG observations only; no engine comparison." if options.oracle_only else
              "Every complete engine outcome compared with PG rows/errors, exact advisory ownership, and listener commit delivery. Pending invocation multiplicity follows the checked contract; PG exposes delivery rather than its internal queue. Alloy checks the evaluator-backed table."}
    write_json(RESULTS / "session-effects.json", report)
    print(json.dumps({key: value for key, value in report.items() if key not in ("observations", "results")}))
    if failures: raise SystemExit(1)


if __name__ == "__main__":
    main()

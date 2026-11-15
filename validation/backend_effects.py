"""Actual backend signaling tests target only worker connections created in a disposable local cluster."""
import argparse
import json
from pathlib import Path
import time

from backend_context import capture, checked, notices
from backend_compare import compare_all
from corpus_runtime import describe
from libpq_client import PgConnection
from pg_runtime import TemporaryPostgres
from run import CLI, RESULTS, digest, write_json


def scalar_cases() -> list[str]:
    """Absent, NULL and invalid inputs cannot signal an unrelated process."""
    return ["SELECT pg_terminate_backend(0)", "SELECT pg_terminate_backend(-1)",
            "SELECT pg_terminate_backend(0,1)", "SELECT pg_terminate_backend(0,-1)",
            "SELECT pg_terminate_backend(NULL)", "SELECT pg_terminate_backend(0,NULL)",
            "SELECT pg_terminate_backend(NULL,-1)", "SELECT pg_terminate_backend(true)",
            "SELECT pg_terminate_backend(0::bigint)", "SELECT pg_terminate_backend(0,0::bigint)"]


def scalar_observations(database) -> list:
    """Observe warning emission separately from Boolean values and SQL errors."""
    results = []
    with PgConnection(database) as subject:
        warnings = notices(subject)
        for index, sql in enumerate(scalar_cases()):
            context = capture(subject)
            warnings.clear()
            result = describe(subject, subject.query(sql))
            results.append({"id": f"backend/scalar/{index}", "sql": sql, "context": context,
                            "postgres": result, "notices": list(warnings), "signal_requested": None})
    return results


def worker_closed(worker) -> dict:
    """Bound observation of our own worker's eventual exit; it is not the timeout-zero return contract."""
    deadline = time.monotonic() + 2
    while time.monotonic() < deadline:
        try:
            result = worker.query("SELECT 1")
        except RuntimeError:
            return {"closed": True, "method": "Owned libpq connection no longer accepts a query"}
        if result["status"] == "error": return {"closed": True, "result": result}
        time.sleep(0.01)
    return {"closed": False}


def worker_observation(database, denied: bool, failure: bool = False) -> dict:
    """The only positive termination target is the connection created in this lexical scope."""
    with PgConnection(database) as subject, PgConnection(database) as worker:
        pid = int(checked(worker, "SELECT pg_backend_pid()::text")["rows"][0][0])
        actor = int(checked(subject, "SELECT pg_backend_pid()::text")["rows"][0][0])
        if pid <= 0 or pid == actor: raise ValueError("Unsafe worker identity")
        if denied: checked(subject, "SET ROLE sqlolla_unprivileged")
        context, warnings = capture(subject, {pid}), notices(subject)
        sql = "SELECT pg_terminate_backend(" + str(pid) + ")"
        if failure: sql += ",1/x FROM backend_divisor"
        checked(subject, "BEGIN")
        result = describe(subject, subject.query(sql))
        checked(subject, "ROLLBACK")
        alive = worker.query("SELECT 1") if denied else worker_closed(worker)
        present = checked(subject, "SELECT count(*)::text FROM pg_stat_activity WHERE pid=" + str(pid))["rows"][0][0]
        return {"id": "backend/permission" if denied else "backend/worker_statement_error" if failure else "backend/owned_worker", "sql": sql,
                "context": context, "postgres": result, "notices": list(warnings),
                "signal_requested": None if denied else pid, "worker_after_rollback": alive,
                "target_present_after_rollback": present == "1",
                "assumption": "Private owned idle worker, fixed target lifetime until this call, ordinary same-server OS signal capability.", **table_context(failure)}


def table_context(used: bool) -> dict:
    """A real base table prevents the independent PostgreSQL planner from folding the failing division."""
    if not used: return {}
    return {"catalog": {"search_path": ["public"], "tables": {"public.backend_divisor": {
        "schema": "public", "name": "backend_divisor", "columns": [{"name": "x", "ty": "int32", "nullable": True, "qualifier": None, "schema": None}]}}},
        "database": {"public.backend_divisor": [[{"type": "int32", "value": 0}]]}}


def warning_before_error(database) -> dict:
    """An absent-PID warning is already external when a later row-dependent division fails."""
    with PgConnection(database) as subject:
        context, warnings = capture(subject), notices(subject)
        sql = "SELECT pg_terminate_backend(0),1/x FROM backend_divisor"
        result = describe(subject, subject.query(sql))
        return {"id": "backend/warning_statement_error", "sql": sql, "context": context,
                "postgres": result, "notices": list(warnings), "signal_requested": None, **table_context(True)}


def main() -> None:
    """Capture the independent oracle before enabling its pure engine callback."""
    parser = argparse.ArgumentParser()
    parser.add_argument("--oracle-only", action="store_true")
    options = parser.parse_args()
    with TemporaryPostgres() as database:
        database.execute("CREATE ROLE sqlolla_unprivileged; CREATE TABLE backend_divisor(x integer); INSERT INTO backend_divisor VALUES(0)")
        values = scalar_observations(database) + [worker_observation(database, denied) for denied in (False, True)]
        values += [worker_observation(database, False, True), warning_before_error(database)]
        version, directory = database.execute("SELECT version()").strip(), database.directory
    comparisons = [] if options.oracle_only else compare_all(values)
    failures = [item["id"] for item in comparisons if item["status"] == "failed"]
    report = {"status": "oracle_only" if options.oracle_only else "failed" if failures else "passed",
              "cases": len(values), "results": values, "comparisons": comparisons, "failures": failures,
              "cli_sha256": None if options.oracle_only else digest(CLI.read_bytes()), "postgres_version": version,
              "cluster_removed": not directory.exists(), "harness_sha256": digest(Path(__file__).read_bytes()),
              "claim": "Actual oracle only." if options.oracle_only else "Every complete engine outcome compared with PG values/errors and actual warnings. Positive signal request tested only against an owned worker; eventual disappearance is observed separately and is not implied by the modeled timeout-zero return.",
              "source": "https://github.com/postgres/postgres/blob/REL_16_16/src/backend/storage/ipc/signalfuncs.c"}
    write_json(RESULTS / "backend-effects.json", report)
    print(json.dumps({key: value for key, value in report.items() if key not in ("results", "comparisons")}))
    if failures: raise SystemExit(1)


if __name__ == "__main__":
    main()

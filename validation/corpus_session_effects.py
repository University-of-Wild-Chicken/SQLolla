"""Observe industrial session effects without treating a rollback as notification delivery."""
import json
import os

from corpus_notify_context import channel_contract
from effect_listener import configure, delivered
from libpq_client import PgConnection
from oracle import quote_identifier
from pg_runtime import TemporaryPostgres, run_binary
from run import digest


def checked(connection, sql: str) -> dict:
    """Setup failures abort observation rather than becoming subject SQL outcomes."""
    result = connection.query(sql)
    if result["status"] != "ok": raise ValueError(result)
    return result


def ownership(connection) -> dict:
    """Capture all granted advisory ownership in the current database, preserving key namespaces."""
    sql = """SELECT pid::text,classid::text,objid::text,objsubid::text,mode,
      (pid=pg_backend_pid())::text FROM pg_locks WHERE locktype='advisory' AND granted
      AND database=(SELECT oid FROM pg_database WHERE datname=current_database())
      ORDER BY pid,classid,objid,objsubid,mode"""
    held = []
    for pid, first, second, namespace, mode, subject in checked(connection, sql)["rows"]:
        if namespace == "1": key = {"kind": "int64", "key": signed((int(first) << 32) | int(second), 64)}
        elif namespace == "2": key = {"kind": "int32_pair", "first": signed(int(first), 32), "second": signed(int(second), 32)}
        else: raise ValueError("Unexpected advisory namespace")
        if mode not in ("ShareLock", "ExclusiveLock"): raise ValueError("Unexpected advisory mode")
        held.append({"key": key, "owner": "industrial_subject" if subject == "true" else "backend:" + pid, "shared": mode == "ShareLock"})
    return {"complete": True, "transaction": "industrial_subject", "held_advisory": held}


def signed(value: int, bits: int) -> int:
    """Interpret exactly the observed unsigned PostgreSQL key bits in their declared namespace."""
    return value - (1 << bits) if value >= (1 << (bits - 1)) else value


def portable_state(state: dict) -> dict:
    """Compare business rows and sequence state without process-local catalog OIDs."""
    result = {}
    for kind in ("database", "sequences"):
        result[kind] = {name: {"columns": [{key: column[key] for key in ("name", "type")} for column in value["columns"]],
                             "rows": value["rows"]} for name, value in state[kind].items()}
    return result


def state_hash(state: dict) -> str:
    """Bind a complete portable business pre/poststate to its evidence digest."""
    return digest(json.dumps(portable_state(state), sort_keys=True).encode())


def environment(connection) -> dict:
    """Clock-independent companions still require the same user, schema, path and formatting context."""
    sql = "SELECT current_schema(),current_database(),current_user,current_setting('search_path'),current_setting('TimeZone'),current_setting('DateStyle'),current_setting('IntervalStyle'),current_setting('standard_conforming_strings')"
    return dict(zip(("schema", "database", "user", "path", "timezone", "datestyle", "intervalstyle", "strings"), checked(connection, sql)["rows"][0]))


def dump(database) -> str:
    """Read only the owned oracle database; credentials remain in a transient child environment."""
    environment = dict(os.environ, PGPASSWORD=database.password, PGCLIENTENCODING="UTF8")
    arguments = ["--clean", "--if-exists", "--no-owner", "--no-privileges", "--no-comments", "--no-security-labels",
                 "-h", "127.0.0.1", "-p", str(database.port), "-U", "sqlolla_test", "-d", "postgres"]
    return run_binary("pg_dump", arguments, env=environment, timeout=60).stdout


def listen(observer, channels: list) -> None:
    """Register every proved possible channel before the subject can commit."""
    configure(observer)
    checked(observer, "UNLISTEN *")
    for channel in channels: checked(observer, "LISTEN " + quote_identifier(channel))
    delivered(observer, 0.02)


def actual_delivery(events: list) -> list:
    """Backend PIDs identify the isolated sender, while channel/payload order is the observed value."""
    return [{key: event[key] for key in ("channel", "payload")} for event in events]


def delivery(events: list) -> list:
    """Pending invocations become first-occurrence ordered channel/payload delivery at commit."""
    result = []
    for event in events:
        value = {key: event[key] for key in ("channel", "payload")}
        if value not in result: result.append(value)
    return result


def companion(database, context: dict, source_before: dict, source_environment: dict, channels: dict) -> dict:
    """Commit unchanged SQL in a separate clone, then destroy that clone rather than restoring guessed rows."""
    archive = dump(database)
    with TemporaryPostgres() as clone:
        clone.execute(archive)
        with PgConnection(clone) as subject, PgConnection(clone) as observer:
            checked(subject, "SET timezone='UTC'")
            result = run_companion(subject, observer, context, source_before, source_environment, channels)
        directory = clone.directory
    return dict(result, clone_removed=not directory.exists(), dump_sha256=digest(archive.encode()),
                parameters_sha256=digest(json.dumps(context["parameters"], sort_keys=True).encode()),
                sql_sha256=digest(context["sql"].encode()))


def run_companion(subject, observer, context: dict, source_before: dict, source_environment: dict, channels: dict) -> dict:
    """Validate clone prestate and environment before running the same statement and observing commit delivery."""
    from corpus_runtime import describe, parameters, snapshot
    if context["prerequisite_sql"]: raise ValueError("Uncommitted prerequisites need a separate clone contract")
    typed, initial = parameters(subject, context), snapshot(subject)
    if portable_state(initial) != portable_state(source_before) or environment(subject) != source_environment:
        raise ValueError("Notification clone does not match source state/environment")
    clone_channels = channel_contract(subject, context, typed)
    if clone_channels != channels: raise ValueError("Companion channel context differs")
    listen(observer, channels["channels"])
    checked(subject, "BEGIN")
    checked(subject, "SAVEPOINT notify_subject")
    result = subject.query(context["sql"], typed)
    if result["status"] == "error": checked(subject, "ROLLBACK TO SAVEPOINT notify_subject")
    result, after = describe(subject, result), snapshot(subject)
    premature = delivered(observer, 0.02)
    end = checked(subject, "COMMIT")
    events = delivered(observer)
    return {"result": result, "before_sha256": state_hash(initial), "after_sha256": state_hash(after),
            "environment": source_environment, "before_commit_delivery": premature, "commit": end,
            "delivered": actual_delivery(events), "clock_contract": "No live clock expression; coalesce tails skipped only for proved non-NULL supplied inputs."}


def prepare(database, subject, observer, case: dict, context: dict, before: dict) -> dict | None:
    """Capture explicit session state and optional independent notification commit evidence."""
    obligations = case["inventory"]["effect_obligations"]
    if not set(obligations) & {"pg_notify", "pg_advisory_xact_lock", "pg_terminate_backend"}: return None
    evidence = {"initial": ownership(subject), "obligations": obligations, "profile": "Private fixed-ownership PostgreSQL fixture"}
    if "pg_notify" in obligations:
        from corpus_runtime import parameters
        channels = channel_contract(subject, context, parameters(subject, context))
        evidence["notification_channels"] = channels
        evidence["notification_companion"] = companion(database, context, before, environment(subject), channels)
        listen(observer, channels["channels"])
    return evidence


def before_end(observer, evidence: dict | None) -> None:
    """Record that the original uncommitted transaction has delivered no notification."""
    if evidence is not None and "notification_companion" in evidence:
        evidence["before_end_delivery"] = actual_delivery(delivered(observer, 0.02))


def equivalent_result(left: dict, right: dict) -> bool:
    """Require equal complete result/error observations while ignoring cluster-local type OIDs."""
    if left["status"] != right["status"]: return False
    if left["status"] == "error":
        return all(left.get(key) == right.get(key) for key in ("sqlstate", "constraint_name", "column_name", "exposed_rows"))
    return (left["command"], left["affected_rows"], [(c["name"], c["type"]) for c in left["columns"]], sorted(left["rows"], key=str)) == (
        right["command"], right["affected_rows"], [(c["name"], c["type"]) for c in right["columns"]], sorted(right["rows"], key=str))


def finish(observer, evidence: dict | None, before: dict, after: dict, rolled_back: dict, result: dict) -> None:
    """Require the companion to have reproduced this source execution, then retain rollback observations."""
    if evidence is None: return
    evidence["advisory_before"] = before["advisory_locks"]
    evidence["advisory_after"] = after["advisory_locks"]
    evidence["advisory_after_rollback"] = rolled_back["advisory_locks"]
    if "notification_companion" in evidence:
        companion = evidence["notification_companion"]
        evidence["rollback_delivery"] = actual_delivery(delivered(observer))
        evidence["companion_matches_source"] = (companion["before_sha256"] == state_hash(before)
            and companion["after_sha256"] == state_hash(after) and equivalent_result(companion["result"], result))
        if not evidence["companion_matches_source"]: raise ValueError("Notification companion changed relevant query behavior")


def execution_context(evidence: dict | None) -> dict | None:
    """Expose only actual before-state ownership as executor input, never expected output effects."""
    return None if evidence is None else evidence["initial"]


def observed_advisory(events: list, initial: dict) -> list:
    """Compare complete subject ownership, including any previously held locks, to pg_locks."""
    from session_compare import advisory_ownership
    held = [item for item in initial["held_advisory"] if item["owner"] == initial["transaction"]]
    held += [event["held"] for event in events if event["kind"] == "advisory_lock"]
    unique = {json.dumps(item, sort_keys=True): item for item in held}
    normalized = []
    for item in unique.values():
        if item["owner"] != initial["transaction"]: raise ValueError("Advisory owner mismatch")
        normalized.append({"kind": "advisory_lock", "held": dict(item, owner="subject")})
    return advisory_ownership(normalized)


def matches(events: list, evidence: dict | None) -> bool:
    """Validate independently observed event families without masking another family's missing evidence."""
    if evidence is None: return not any(event["kind"] in ("notification", "advisory_lock") for event in events)
    if observed_advisory(events, evidence["initial"]) != sorted(evidence["advisory_after"]): return False
    if evidence["advisory_after_rollback"] != evidence["advisory_before"]: return False
    notifications = [event for event in events if event["kind"] == "notification"]
    companion = evidence.get("notification_companion")
    if companion is None: return not notifications
    return (evidence.get("companion_matches_source") and not evidence["before_end_delivery"]
            and not evidence["rollback_delivery"] and not companion["before_commit_delivery"]
            and all(event["channel"] in evidence["notification_channels"]["channels"] for event in notifications)
            and delivery(notifications) == companion["delivered"])

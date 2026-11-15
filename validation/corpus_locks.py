"""Independently observe industrial SELECT row locks in an isolated, fixed ownership snapshot."""
import json

from oracle import literal, quote_identifier

MODES = {"key_share": "KEY SHARE", "share": "SHARE", "no_key_update": "NO KEY UPDATE", "update": "UPDATE"}
PATTERNS = {frozenset(["update"]): "key_share", frozenset(["no_key_update", "update"]): "share",
            frozenset(["share", "no_key_update", "update"]): "no_key_update", frozenset(MODES): "update"}


def checked(connection, sql: str) -> dict:
    """Treat any observer setup failure as a failed oracle, never a SQLolla query result."""
    result = connection.query(sql)
    if result["status"] != "ok": raise ValueError({"lock_observer_sql": sql, "result": result})
    return result


def applies(case: dict) -> bool:
    """Observe explicit locking SELECTs even when nested inside WITH or a mutation statement."""
    return "locking" in case["inventory"]["features"]


def identities(connection, before: dict) -> dict:
    """Map physical tuples to the declared bag ordinals, retaining duplicate occurrences."""
    result = {}
    for name, relation in before["database"].items():
        if relation["kind"] != "r": continue
        rows = checked(connection, "SELECT ctid::text,* FROM " + quote_identifier(name))["rows"]
        rows.sort(key=lambda row: json.dumps(row[1:], ensure_ascii=False))
        if [row[1:] for row in rows] != relation["rows"]: raise ValueError("Lock identity snapshot changed")
        result[name] = [row[0] for row in rows]
    return result


def blocked(observer, name: str, tuple_id: str, mode: str) -> bool:
    """Probe actual conflict behavior in another transaction and release each trial lock."""
    checked(observer, "BEGIN")
    result = observer.query("SELECT ctid FROM " + quote_identifier(name) + " WHERE ctid=" + literal(tuple_id) + "::tid FOR " + MODES[mode] + " NOWAIT")
    checked(observer, "ROLLBACK")
    if result["status"] == "error" and result["sqlstate"] != "55P03": raise ValueError(result)
    if result["status"] == "ok" and len(result["rows"]) != 1: raise ValueError("Prestate lock tuple disappeared outside the subject transaction")
    return result["status"] == "error"


def observe(subject, observer, physical: dict) -> dict:
    """Read table ownership and independently distinguish all four held row strengths."""
    relations = checked(subject, "SELECT c.relname FROM pg_locks l JOIN pg_class c ON c.oid=l.relation JOIN pg_namespace n ON n.oid=c.relnamespace WHERE l.pid=pg_backend_pid() AND n.nspname='public' AND c.relkind='r' AND l.mode='RowShareLock' AND l.granted ORDER BY c.relname")["rows"]
    effects = []
    for (name,) in relations:
        effects.append({"kind": "table_lock", "table": "public." + name, "mode": "row_share"})
        for ordinal, tuple_id in enumerate(physical[name]):
            pattern = frozenset(mode for mode in MODES if blocked(observer, name, tuple_id, mode))
            if pattern: effects.append({"kind": "row_lock", "table": "public." + name, "occurrence": ordinal, "strength": PATTERNS[pattern]})
    return {"initial": {"complete": True, "transaction": "industrial_subject", "held_rows": [], "row_share_conflicts": []},
            "effects": effects, "physical_occurrences": physical,
            "method": "Private PostgreSQL cluster; no competing actors. Independent session probes each physical tuple with all four NOWAIT modes before subject rollback; relation locks read from pg_locks."}

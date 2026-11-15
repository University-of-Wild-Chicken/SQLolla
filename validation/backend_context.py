"""Capture backend identities and role permissions only inside the owned PostgreSQL oracle."""
import ctypes


def checked(connection, sql: str) -> dict:
    """Require actual catalog setup observations instead of inferring permission from expected results."""
    value = connection.query(sql)
    if value["status"] != "ok": raise ValueError(value)
    return value


def capture(connection, owned_targets: set[int] | None = None) -> dict:
    """Signal capability is an explicit controlled-worker assumption, never a query-result lookup."""
    own = checked(connection, "SELECT pg_backend_pid()::text,rolsuper::text,pg_has_role(current_user,'pg_signal_backend','USAGE')::text FROM pg_roles WHERE rolname=current_user")["rows"][0]
    sql = """SELECT a.pid::text,COALESCE(r.rolsuper,true)::text,
      (a.usesysid IS NOT NULL AND pg_has_role(current_user,a.usesysid,'USAGE'))::text,a.backend_type
      FROM pg_stat_activity a LEFT JOIN pg_roles r ON r.oid=a.usesysid ORDER BY a.pid"""
    targets = [{"pid": int(pid), "superuser": superuser == "true", "actor_has_role": member == "true",
                "signal_capability": int(pid) in (owned_targets or set()) and kind == "client backend"}
               for pid, superuser, member, kind in checked(connection, sql)["rows"]]
    return {"complete": True, "stable_snapshot": True, "actor_pid": int(own[0]),
            "actor_superuser": own[1] == "true", "actor_signal_role": own[2] == "true", "targets": targets}


NOTICE_RECEIVER = ctypes.CFUNCTYPE(None, ctypes.c_void_p, ctypes.c_void_p)


def notices(connection) -> list:
    """Capture actual server warning SQLSTATE and message on the existing private libpq connection."""
    values = []
    def receive(_argument, result):
        """Copy diagnostic strings while libpq still owns the supplied notice result."""
        code = connection.library.PQresultErrorField(result, 67)
        message = connection.library.PQresultErrorField(result, 77)
        severity = connection.library.PQresultErrorField(result, 86)
        values.append({"sqlstate": None if code is None else code.decode(),
                       "message": None if message is None else message.decode(),
                       "severity": None if severity is None else severity.decode()})
    callback = NOTICE_RECEIVER(receive)
    connection.library.PQsetNoticeReceiver.argtypes = [ctypes.c_void_p, NOTICE_RECEIVER, ctypes.c_void_p]
    connection.library.PQsetNoticeReceiver.restype = ctypes.c_void_p
    connection.library.PQsetNoticeReceiver(connection.connection, callback, None)
    connection.notice_receiver_callback = callback
    return values

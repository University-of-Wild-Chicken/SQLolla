"""Actual libpq notification delivery observations for the isolated PostgreSQL oracle."""
import ctypes
import time


def checked(connection, sql: str) -> dict:
    """Listener setup errors fail the observer without importing the corpus orchestrator."""
    result = connection.query(sql)
    if result["status"] != "ok": raise ValueError(result)
    return result


class Notification(ctypes.Structure):
    """Match libpq's owned PGnotify record; PQfreemem releases the entire allocation."""
    _fields_ = [("channel", ctypes.c_char_p), ("pid", ctypes.c_int), ("payload", ctypes.c_char_p)]


def configure(connection) -> None:
    """Declare only the libpq asynchronous notification APIs on this private connection."""
    library = connection.library
    library.PQconsumeInput.argtypes, library.PQconsumeInput.restype = [ctypes.c_void_p], ctypes.c_int
    library.PQnotifies.argtypes, library.PQnotifies.restype = [ctypes.c_void_p], ctypes.POINTER(Notification)
    library.PQfreemem.argtypes, library.PQfreemem.restype = [ctypes.c_void_p], None


def drain(connection) -> list[dict]:
    """Read already-arrived events without manufacturing delivery from SQL call syntax."""
    if not connection.library.PQconsumeInput(connection.connection): raise RuntimeError("Notification input failed")
    values = []
    while True:
        pointer = connection.library.PQnotifies(connection.connection)
        if not pointer: return values
        try:
            value = pointer.contents
            values.append({"channel": value.channel.decode(), "payload": value.payload.decode(), "backend_pid": value.pid})
        finally:
            connection.library.PQfreemem(pointer)


def delivered(connection, timeout: float = 0.15) -> list[dict]:
    """Pump independent backend round trips for a bounded delivery window, preserving event order."""
    values, deadline = [], time.monotonic() + timeout
    while True:
        checked(connection, "SELECT 1")
        values += drain(connection)
        if time.monotonic() >= deadline: return values
        time.sleep(0.005)

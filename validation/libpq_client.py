"""Minimal test-only libpq adapter for exact SQL, typed parameters and SQLSTATE."""
import ctypes
import ctypes.util
import os
from pathlib import Path

from pg_runtime import PG_BIN


def load_library():
    """Load the oracle distribution's libpq without adding global PATH entries."""
    if os.name == "nt":
        directory = os.add_dll_directory(str(PG_BIN))
        library = ctypes.CDLL(str(PG_BIN / "libpq.dll"))
        library.directory_handle = directory
    else:
        library = ctypes.CDLL(ctypes.util.find_library("pq"))
    signatures = {"PQconnectdbParams": ([ctypes.POINTER(ctypes.c_char_p), ctypes.POINTER(ctypes.c_char_p), ctypes.c_int], ctypes.c_void_p),
        "PQstatus": ([ctypes.c_void_p], ctypes.c_int), "PQfinish": ([ctypes.c_void_p], None),
        "PQexecParams": ([ctypes.c_void_p, ctypes.c_char_p, ctypes.c_int, ctypes.POINTER(ctypes.c_uint), ctypes.POINTER(ctypes.c_char_p), ctypes.c_void_p, ctypes.c_void_p, ctypes.c_int], ctypes.c_void_p),
        "PQclear": ([ctypes.c_void_p], None), "PQresultStatus": ([ctypes.c_void_p], ctypes.c_int),
        "PQresultErrorField": ([ctypes.c_void_p, ctypes.c_int], ctypes.c_char_p),
        "PQcmdTuples": ([ctypes.c_void_p], ctypes.c_char_p), "PQcmdStatus": ([ctypes.c_void_p], ctypes.c_char_p),
        "PQnfields": ([ctypes.c_void_p], ctypes.c_int), "PQntuples": ([ctypes.c_void_p], ctypes.c_int),
        "PQfname": ([ctypes.c_void_p, ctypes.c_int], ctypes.c_char_p), "PQftype": ([ctypes.c_void_p, ctypes.c_int], ctypes.c_uint),
        "PQgetisnull": ([ctypes.c_void_p, ctypes.c_int, ctypes.c_int], ctypes.c_int),
        "PQgetvalue": ([ctypes.c_void_p, ctypes.c_int, ctypes.c_int], ctypes.c_char_p)}
    for name, (arguments, result) in signatures.items():
        getattr(library, name).argtypes, getattr(library, name).restype = arguments, result
    return library


def text(value) -> str | None:
    """Decode nullable UTF-8 fields returned by PostgreSQL without losing NULL."""
    return None if value is None else value.decode("utf-8")


class PgConnection:
    """Keep one private transaction session for before/after effect observations."""

    def __init__(self, database) -> None:
        """Connect to the owned ephemeral oracle with credentials kept out of output."""
        self.library = load_library()
        self.type_names = {}
        keys = [b"host", b"port", b"dbname", b"user", b"password", b"client_encoding", None]
        values = [b"127.0.0.1", str(database.port).encode(), b"postgres", b"sqlolla_test", database.password.encode(), b"UTF8", None]
        self.connection = self.library.PQconnectdbParams((ctypes.c_char_p * len(keys))(*keys), (ctypes.c_char_p * len(values))(*values), 0)
        if self.library.PQstatus(self.connection):
            self.close()
            raise RuntimeError("Isolated libpq connection failed")

    def query(self, sql: str, parameters: list[dict] | None = None) -> dict:
        """Submit exact SQL bytes using explicit parameter OIDs and text values."""
        parameters = parameters or []
        oids = (ctypes.c_uint * len(parameters))(*(item["oid"] for item in parameters))
        values = (ctypes.c_char_p * len(parameters))(*(None if item["value"] is None else item["value"].encode() for item in parameters))
        result = self.library.PQexecParams(self.connection, sql.encode(), len(parameters), oids, values, None, None, 0)
        if not result: raise RuntimeError("libpq returned no result")
        try:
            return self.decode(result)
        finally:
            self.library.PQclear(result)

    def decode(self, result) -> dict:
        """Preserve typed result columns, ordered text rows, affected count and SQLSTATE."""
        library, status = self.library, self.library.PQresultStatus(result)
        if status not in {0, 1, 2}:
            return {"status": "error", "sqlstate": text(library.PQresultErrorField(result, 67)),
                    "message": text(library.PQresultErrorField(result, 77)), "detail": text(library.PQresultErrorField(result, 68)),
                    "constraint_name": text(library.PQresultErrorField(result, 110)),
                    "column_name": text(library.PQresultErrorField(result, 99)), "table_name": text(library.PQresultErrorField(result, 116)),
                    "exposed_rows": []}
        columns = [{"name": text(library.PQfname(result, index)), "type_oid": library.PQftype(result, index)} for index in range(library.PQnfields(result))]
        rows = [[None if library.PQgetisnull(result, row, column) else text(library.PQgetvalue(result, row, column))
                 for column in range(len(columns))] for row in range(library.PQntuples(result))]
        return {"status": "ok", "sqlstate": "00000", "command": text(library.PQcmdStatus(result)),
                "affected_rows": int(library.PQcmdTuples(result) or b"0"), "columns": columns, "rows": rows}

    def close(self) -> None:
        """Release the private connection; PostgreSQL rolls back any unfinished work."""
        if self.connection:
            self.library.PQfinish(self.connection)
            self.connection = None

    def __enter__(self):
        """Return the already connected private test session."""
        return self

    def __exit__(self, *_exception) -> None:
        """Close on success, error or cancellation."""
        self.close()

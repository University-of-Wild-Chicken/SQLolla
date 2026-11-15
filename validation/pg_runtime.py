"""Manage a disposable, loopback-only PostgreSQL cluster for differential tests."""
from __future__ import annotations

import os
import csv
from pathlib import Path
import secrets
import shutil
import socket
import subprocess
import tempfile
from process_helpers import process_options

ROOT = Path(__file__).resolve().parents[1]
DEFAULT_BIN = ROOT / "tools/.tools/postgres-16.16/pgsql/bin" if os.name == "nt" else Path(shutil.which("pg_ctl") or "/usr/lib/postgresql/16/bin/pg_ctl").parent
PG_BIN = Path(os.environ.get("SQLolla_PG_BIN", DEFAULT_BIN))
RUNS = Path(__file__).resolve().parent / ".runs"


def run_binary(name: str, arguments: list[str], capture: bool = True, **kwargs) -> subprocess.CompletedProcess:
    """Run a pinned PostgreSQL executable without a shell or visible window."""
    streams = {"capture_output": True} if capture else {"stdout": subprocess.DEVNULL, "stderr": subprocess.DEVNULL}
    suffix = ".exe" if os.name == "nt" else ""
    return subprocess.run([str(PG_BIN / f"{name}{suffix}"), *arguments], check=True,
                          encoding="utf-8", **streams,
                          **process_options(), **kwargs)


def available_port() -> int:
    """Choose a free IPv4 loopback port; startup detects a subsequent collision."""
    with socket.socket() as listener:
        listener.bind(("127.0.0.1", 0))
        return listener.getsockname()[1]


def grant_windows_oracle_access(directory: Path) -> None:
    """Allow the exact runner SID after PostgreSQL removes admin group access."""
    if os.name != "nt" or os.environ.get("GITHUB_ACTIONS") != "true":
        return
    system = Path(os.environ["SystemRoot"]) / "System32"
    identity = subprocess.check_output([str(system / "whoami.exe"), "/user", "/fo", "csv", "/nh"], **process_options())
    sid = next(csv.reader([identity.decode("ascii", errors="replace").strip()]))[1]
    subprocess.run([str(system / "icacls.exe"), str(directory), "/grant", f"*{sid}:(OI)(CI)M", "/Q"],
                   check=True, capture_output=True, **process_options())


def remove_owned_directory(directory: Path) -> None:
    """Delete only a resolved descendant of this validation suite's run directory."""
    resolved, parent = directory.resolve(), RUNS.resolve()
    if resolved == parent or not resolved.is_relative_to(parent):
        raise ValueError("Refusing cleanup outside validation/.runs")
    shutil.rmtree(resolved)


class TemporaryPostgres:
    """Own a temporary SCRAM-authenticated PostgreSQL16 cluster and its lifecycle."""

    def __init__(self) -> None:
        """Allocate private per-run state without starting a server."""
        RUNS.mkdir(exist_ok=True)
        self.directory = Path(tempfile.mkdtemp(prefix="pg16-", dir=RUNS))
        self.data = self.directory / "data"
        self.port = available_port()
        self.password = secrets.token_urlsafe(32)
        self.started = False

    def __enter__(self) -> "TemporaryPostgres":
        """Initialize and start the cluster; remove partial setup on any failure."""
        try:
            self.initialize()
            options = f"-h 127.0.0.1 -p {self.port} -c max_connections=10"
            run_binary("pg_ctl", ["-D", str(self.data), "-l", str(self.directory / "server.log"),
                                 "-o", options, "-w", "-t", "20", "start"], capture=False,
                       stdin=subprocess.DEVNULL, timeout=30)
            self.started = True
            return self
        except BaseException:
            self.close()
            raise

    def initialize(self) -> None:
        """Create an UTF-8/C-locale cluster using a transient random password file."""
        grant_windows_oracle_access(self.directory)
        version = run_binary("postgres", ["--version"]).stdout
        if "PostgreSQL) 16." not in version: raise ValueError("Oracle requires an actual PostgreSQL16 runtime")
        password_file = self.directory / "password.txt"
        password_file.write_text(self.password, encoding="utf-8")
        arguments = ["-D", str(self.data), "--username=sqlolla_test", "--encoding=UTF8",
                     "--locale=C", "--auth-host=scram-sha-256", "--auth-local=trust",
                     "--no-instructions", "--pwfile=" + str(password_file)]
        try:
            run_binary("initdb", arguments, timeout=60)
        except subprocess.CalledProcessError as error:
            raise RuntimeError(f"PostgreSQL initdb failed: {error.stderr.strip()}") from error
        finally:
            password_file.unlink(missing_ok=True)

    def execute(self, script: str, timeout: int = 120) -> str:
        """Run oracle SQL with strict error handling and credentials kept out of logs."""
        environment = dict(os.environ, PGPASSWORD=self.password, PGCLIENTENCODING="UTF8")
        arguments = ["-X", "-qAt", "-h", "127.0.0.1", "-p", str(self.port), "-U",
                     "sqlolla_test", "-d", "postgres", "-v", "ON_ERROR_STOP=1"]
        try:
            return run_binary("psql", arguments, input=script, env=environment,
                              timeout=timeout).stdout
        except subprocess.CalledProcessError as error:
            raise RuntimeError(error.stderr.strip()) from error

    def close(self) -> None:
        """Stop the owned cluster before validated recursive cleanup."""
        if (self.data / "postmaster.pid").exists():
            try:
                run_binary("pg_ctl", ["-D", str(self.data), "-m", "fast", "-w", "stop"], timeout=30)
            except subprocess.SubprocessError:
                run_binary("pg_ctl", ["-D", str(self.data), "-m", "immediate", "-w", "stop"], timeout=30)
        self.started = False
        if self.directory.exists():
            remove_owned_directory(self.directory)

    def __exit__(self, *_exception) -> None:
        """Always stop and remove test state, including after an oracle error."""
        self.close()

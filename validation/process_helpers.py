"""Bounded subprocess helpers with platform-specific process-tree cancellation."""
import os
import signal
import subprocess


def process_options() -> dict:
    """Hide Windows children and isolate Unix children into a killable session."""
    return {"creationflags": getattr(subprocess, "CREATE_NO_WINDOW", 0)} if os.name == "nt" else {"start_new_session": True}


def stop_tree(process: subprocess.Popen) -> None:
    """Terminate only the subprocess tree/session started by this validation run."""
    if process.poll() is not None: return
    if os.name == "nt":
        subprocess.run(["taskkill", "/PID", str(process.pid), "/T", "/F"], capture_output=True,
                       check=False, timeout=10, **process_options())
    else:
        os.killpg(process.pid, signal.SIGKILL)
    process.wait(timeout=10)


def communicate(command: list[str], wire: str, timeout: int, cwd=None) -> str:
    """Drain both pipes and kill the owned tree on timeout before collecting output."""
    process = subprocess.Popen(command, cwd=cwd, stdin=subprocess.PIPE, stdout=subprocess.PIPE,
                               stderr=subprocess.PIPE, encoding="utf-8", **process_options())
    try:
        output, errors = process.communicate(wire, timeout=timeout)
    except subprocess.TimeoutExpired:
        stop_tree(process)
        process.communicate(timeout=10)
        raise
    if process.returncode:
        raise subprocess.CalledProcessError(process.returncode, command, output, errors)
    return output

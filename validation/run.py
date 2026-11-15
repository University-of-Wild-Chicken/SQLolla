"""Compare every generated case with actual PostgreSQL, Rust and decoded Alloy bags."""
from __future__ import annotations

import argparse
from collections import Counter
import hashlib
import json
import os
from pathlib import Path
import subprocess
import time

from cases import all_cases
from oracle import oracle_script, rows_to_bag, typed_bag
from pg_runtime import ROOT, TemporaryPostgres
from process_helpers import process_options, stop_tree

RESULTS = Path(__file__).resolve().parent / "results"
CLI = ROOT / ("target/debug/sqlolla.exe" if os.name == "nt" else "target/debug/sqlolla")


def write_json(path: Path, value: object) -> None:
    """Save evidence as UTF-8 with stable object keys."""
    path.write_text(json.dumps(value, ensure_ascii=False, indent=2, sort_keys=True), encoding="utf-8")


def digest(data: bytes) -> str:
    """Identify exact fixture and oracle bytes for safe reuse."""
    return hashlib.sha256(data).hexdigest()


def case_digest(cases: list[dict]) -> str:
    """Bind oracle reuse to both query requests and independent PostgreSQL setup."""
    return digest(json.dumps(cases, ensure_ascii=False, sort_keys=True).encode("utf-8"))


def index_lines(text: str) -> dict[str, dict]:
    """Parse one response per request and reject duplicate identifiers."""
    result = {}
    for line in text.splitlines():
        response = json.loads(line)
        identifier = response["id"]
        if identifier in result:
            raise ValueError(f"Duplicate response: {identifier}")
        result[identifier] = response
    return result


def postgres_results(cases: list[dict], reuse: bool) -> tuple[dict, dict]:
    """Run all oracle cases, or reuse only an exact previously executed corpus."""
    output_file, metadata_file = RESULTS / "postgres.jsonl", RESULTS / "oracle-metadata.json"
    if reuse:
        metadata = json.loads(metadata_file.read_text(encoding="utf-8"))
        output = output_file.read_text(encoding="utf-8")
        if metadata["case_sha256"] != case_digest(cases) or metadata["output_sha256"] != digest(output.encode("utf-8")):
            raise ValueError("Oracle cache does not match the complete fixture set")
        return index_lines(output), metadata
    with TemporaryPostgres() as database:
        version = database.execute("SELECT version();").strip()
        output = database.execute(oracle_script(cases), timeout=300)
        directory = database.directory
    output_file.write_text(output, encoding="utf-8")
    metadata = {"version": version, "cases": len(cases), "case_sha256": case_digest(cases),
                "output_sha256": digest(output.encode("utf-8")), "cluster_removed": not directory.exists()}
    write_json(metadata_file, metadata)
    return index_lines(output), metadata


def run_cli(cases: list[dict], binary: Path, timeout: int) -> dict:
    """Run one persistent CLI and capture files without inherited output-pipe deadlocks."""
    request_file, output_file = RESULTS / "requests.jsonl", RESULTS / "cli-output.jsonl"
    request_file.write_text("".join(json.dumps(case["request"], ensure_ascii=False) + "\n" for case in cases), encoding="utf-8")
    with request_file.open("rb") as source, output_file.open("wb") as output, (RESULTS / "cli-stderr.log").open("wb") as error:
        process = subprocess.Popen([str(binary)], cwd=ROOT, stdin=source, stdout=output,
                                   stderr=error, **process_options())
        try:
            exit_code = process.wait(timeout=timeout)
        except subprocess.TimeoutExpired:
            stop_tree(process)
            raise
    if exit_code:
        raise RuntimeError(f"CLI exited {exit_code}; inspect results/cli-stderr.log")
    return index_lines(output_file.read_text(encoding="utf-8"))


def compare_case(case: dict, expected: dict, actual: dict) -> dict | None:
    """Independently compare both engine bags with PostgreSQL, ignoring claimed matches."""
    if actual.get("status") != "ok" or actual.get("alloy_status") != "SAT":
        return {"id": case["request"]["id"], "reason": "engine_error_or_not_sat", "response": actual}
    if "rust_bag" not in actual or "alloy_bag" not in actual:
        return {"id": case["request"]["id"], "reason": "independent_native_encoding_missing", "response": actual}
    pg = rows_to_bag(expected["rows"], case["oracle"]["output_types"])
    rust, alloy = typed_bag(actual["rust_bag"]), typed_bag(actual["alloy_bag"])
    if pg == rust == alloy:
        return None
    return {"id": case["request"]["id"], "reason": "bag_mismatch", "postgres": dict(pg),
            "rust": dict(rust), "alloy": dict(alloy), "sql": case["request"]["sql"]}


def compare_all(cases: list[dict], expected: dict, actual: dict) -> list[dict]:
    """Require exact case coverage and evaluate every case even after a mismatch."""
    identifiers = {case["request"]["id"] for case in cases}
    if set(expected) != identifiers or set(actual) != identifiers:
        raise ValueError("Missing or extra oracle/CLI responses; suite is incomplete")
    failures = [compare_case(case, expected[case["request"]["id"]], actual[case["request"]["id"]]) for case in cases]
    return [failure for failure in failures if failure is not None]


def arguments() -> argparse.Namespace:
    """Expose explicit oracle-only and exact-cache reuse modes."""
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--oracle-only", action="store_true")
    parser.add_argument("--reuse-oracle", action="store_true")
    parser.add_argument("--cli", type=Path, default=CLI)
    parser.add_argument("--timeout", type=int, default=1200)
    return parser.parse_args()


def main() -> None:
    """Persist complete comparison counts and fail the command on any discrepancy."""
    options, started = arguments(), time.monotonic()
    RESULTS.mkdir(exist_ok=True)
    cases = all_cases()
    expected, metadata = postgres_results(cases, options.reuse_oracle)
    if len(expected) != len(cases):
        raise ValueError("Incomplete PostgreSQL oracle output")
    failures = [] if options.oracle_only else compare_all(cases, expected, run_cli(cases, options.cli, options.timeout))
    summary = {"status": "oracle_ready_core_not_compared" if options.oracle_only else ("failed" if failures else "passed"),
               "cases": len(cases), "failures": len(failures), "compared": 0 if options.oracle_only else len(cases),
               "families": dict(Counter(case["family"] for case in cases)), "oracle": metadata,
               "cli_sha256": None if options.oracle_only else digest(options.cli.read_bytes()),
               "cli_output_sha256": None if options.oracle_only else digest((RESULTS / "cli-output.jsonl").read_bytes()),
               "seconds": round(time.monotonic() - started, 3), "skipped": 0}
    write_json(RESULTS / "summary.json", summary)
    write_json(RESULTS / "failures.json", failures)
    print(json.dumps(summary))
    if failures:
        raise SystemExit(1)


if __name__ == "__main__":
    main()

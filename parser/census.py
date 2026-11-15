"""Measure production-parser export readiness on the pinned upstream application corpus.

This is a syntax/export census, not catalog binding, PostgreSQL execution, or
semantic verification. Reports retain source references and hashes, not SQL text.
"""
from __future__ import annotations

import argparse
from collections import Counter
import hashlib
import json
from pathlib import Path
import re
import subprocess

ROOT = Path(__file__).resolve().parent
PIN = "ad15c585155ff7fc2c5fc4c9746905e4959ff5eb"
PROTOCOLS = ("sqlolla.parser.v1", "sqlolla.parser.v2")
SOURCES = ("benchbase", "river", "temporal")


def digest(data: bytes) -> str:
    """Bind exact UTF-8 SQL and artifact bytes without copying their contents."""
    return hashlib.sha256(data).hexdigest()


def verify_dependency(dependency: Path) -> None:
    """Require the reviewed checkout identity and unchanged tracked source."""
    head = subprocess.check_output(["git", "-C", str(dependency), "rev-parse", "HEAD"], text=True).strip()
    if head != PIN:
        raise ValueError("SQLeanParser commit differs from the protocol pin")
    subprocess.run(["git", "-C", str(dependency), "diff", "--quiet", "HEAD", "--"], check=True)
    verify_cst_contract(dependency)


def verify_cst_contract(dependency: Path) -> None:
    """Bind every selected lowering production and the upstream grammar source to reviewed contracts."""
    grammar = dependency / "fixtures/postgresql_grammar/source/src/backend/parser/gram.y"
    if digest(grammar.read_bytes()) != "14502d423caca9995db18e5f7c4beb497b812ca1055692ff24543aab1e0dad28":
        raise ValueError("Pinned PostgreSQL grammar source hash mismatch")
    contracts = ROOT / "SQLollaParser/CstContract.lean"
    referenced = set()
    for path in (ROOT / "SQLollaParser").glob("Cst*.lean"):
        if path == contracts:
            continue
        source = path.read_text(encoding="utf-8")
        referenced.update(map(int, re.findall(r"\.rule (\d+)", source)))
        for group in re.findall(r"\[(\d+(?:,\s*\d+)*)\]", source):
            referenced.update(map(int, group.replace(" ", "").split(",")))
    declared = set(map(int, re.findall(r'^  \((\d+), "', contracts.read_text(encoding="utf-8"), re.M)))
    if referenced != declared:
        raise ValueError("CST dispatch and complete-production contracts differ")


def corpus_inputs(dependency: Path) -> tuple[list[dict], dict]:
    """Read every record in the three fixed industrial populations with its provenance."""
    records, files = [], {}
    for source in SOURCES:
        relative = f"fixtures/external_postgresql/{source}.jsonl"
        data = (dependency / relative).read_bytes()
        lines = data.decode("utf-8").splitlines()
        files[relative] = {"sha256": digest(data), "records": len(lines)}
        for line, raw in enumerate(lines, 1):
            record = json.loads(raw)
            records.append({**record, "corpus_file": relative, "corpus_line": line, "population": source})
    return records, files


def classify(envelope: dict) -> tuple[str, str | None]:
    """Separate export, upstream syntax rejection and preparse resource refusal."""
    if (envelope.get("protocol") not in PROTOCOLS or envelope.get("parser_commit") != PIN
            or envelope.get("assurance") != "syntax-only" or envelope.get("semantic_preservation") is not False):
        raise ValueError("Parser protocol/identity mismatch")
    result = envelope["result"]
    pair = (result.get("parse_status"), result.get("export_status"))
    categories = {("parsed", "exported"): "exported", ("parsed", "unsupported"): "parsed_unsupported",
                  ("rejected", "unsupported"): "syntax_rejected", ("not_attempted", "unsupported"): "preflight_refused"}
    if pair not in categories or (pair[1] == "exported" and not isinstance(result.get("ast"), dict)):
        raise ValueError("Malformed parser result")
    return categories[pair], result.get("diagnostic")


def syntax_tags(value: object) -> list[str]:
    """Inventory exported constructor tags without retaining SQL text or literal contents."""
    if isinstance(value, list):
        return [tag for item in value for tag in syntax_tags(item)]
    if isinstance(value, dict):
        own = [value["tag"]] if isinstance(value.get("tag"), str) else []
        return own + [tag for item in value.values() for tag in syntax_tags(item)]
    return []


def observe(executable: Path, record: dict) -> dict:
    """Invoke one native stdin request and retain only provenance plus observed statuses."""
    sql = record["sql"].encode("utf-8")
    result = {key: record.get(key) for key in ("id", "population", "corpus_file", "corpus_line", "source_url",
                                              "source_file", "transformations", "template_markers", "license")}
    result.update(sql_sha256=digest(sql), source_commit=record.get("commit", record.get("revision")))
    try:
        process = subprocess.run([str(executable)], input=sql, capture_output=True, timeout=10,
                                 creationflags=getattr(subprocess, "CREATE_NO_WINDOW", 0), check=True)
        if process.stderr or len(process.stdout) > 1_048_577:
            raise ValueError("Unexpected parser stream output")
        envelope = json.loads(process.stdout)
        result["outcome"], result["diagnostic"] = classify(envelope)
        result["protocol"] = envelope["protocol"]
        result["syntax_tags"] = dict(sorted(Counter(syntax_tags(envelope["result"].get("ast"))).items()))
        result["position"] = {key: envelope["result"][key] for key in ("line", "column", "offset")
                              if key in envelope["result"]}
    except (OSError, ValueError, KeyError, subprocess.SubprocessError) as error:
        result.update(outcome="infrastructure_error", diagnostic=type(error).__name__)
    return result


def report(executable: Path, records: list[dict], files: dict, results: list[dict]) -> dict:
    """Summarize measured readiness without reusing historical parser or PostgreSQL claims."""
    counts = dict(sorted(Counter(item["outcome"] for item in results).items()))
    reasons = Counter(item["diagnostic"] for item in results if item["diagnostic"])
    populations = {source: dict(sorted(Counter(item["outcome"] for item in results
                                               if item["population"] == source).items())) for source in SOURCES}
    return {"status": "complete" if not counts.get("infrastructure_error") else "incomplete",
            "claim": "production parser syntax/export readiness only; no binding, solver or PostgreSQL execution",
            "protocols": list(PROTOCOLS), "parser_commit": PIN, "executable_sha256": digest(executable.read_bytes()),
            "runner_sha256": digest(Path(__file__).read_bytes()), "corpus_files": files,
            "cst_contract_sha256": digest((ROOT / "SQLollaParser/CstContract.lean").read_bytes()),
            "postgresql_grammar_commit": "f0595c3d839debdb140903621c1b44a4f4d86256",
            "records": len(records), "distinct_sql_texts": len({item["sql_sha256"] for item in results}),
            "outcomes": counts, "diagnostics": dict(sorted(reasons.items())), "populations": populations,
            "observations": results}


def main() -> int:
    """Run the fixed census and write one reproducible, source-bound coverage artifact."""
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--parser", type=Path, default=ROOT / ".lake/build/bin" / ("sqlolla_parser.exe" if __import__("os").name == "nt" else "sqlolla_parser"))
    parser.add_argument("--dependency", type=Path, default=ROOT.parent.parent / "dependencies/SQLeanParser")
    parser.add_argument("--output", type=Path, default=ROOT / "census-results.json")
    args = parser.parse_args()
    verify_dependency(args.dependency)
    records, files = corpus_inputs(args.dependency)
    results = [observe(args.parser.resolve(), record) for record in records]
    evidence = report(args.parser, records, files, results)
    args.output.write_text(json.dumps(evidence, ensure_ascii=False, indent=2) + "\n", encoding="utf-8")
    print(json.dumps({key: evidence[key] for key in ("status", "records", "distinct_sql_texts", "outcomes", "populations")}))
    return 0 if evidence["status"] == "complete" else 1


if __name__ == "__main__":
    raise SystemExit(main())

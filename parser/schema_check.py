"""Validate native parser responses against pinned JSON schemas using jsonschema 4.25.1."""
from __future__ import annotations

import importlib.metadata
import json
from pathlib import Path
import sys

ROOT = Path(__file__).resolve().parent
sys.path.insert(0, str(ROOT / ".tools/schema"))


def validators() -> dict:
    """Load both complete schemas once and reject an unpinned validation dependency."""
    from jsonschema import Draft202012Validator
    if importlib.metadata.version("jsonschema") != "4.25.1":
        raise RuntimeError("Install parser/schema-requirements.txt into parser/.tools/schema")
    result = {}
    for version, filename in (("v1", "parser-ast.json"), ("v2", "parser-ast-v2.json")):
        schema = json.loads((ROOT.parent / "protocol" / filename).read_text(encoding="utf-8"))
        Draft202012Validator.check_schema(schema)
        result[f"sqlolla.parser.{version}"] = Draft202012Validator(schema)
    return result


def main() -> int:
    """Check bounded JSONL responses without logging SQL literals or collecting large error trees."""
    schemas = validators()
    checked = 0
    while line := sys.stdin.buffer.readline(1_048_578):
        if len(line) > 1_048_577 or not line.endswith(b"\n"):
            raise ValueError("Invalid schema-check response frame")
        envelope = json.loads(line)
        validator = schemas[envelope["protocol"]]
        failure = next(validator.iter_errors(envelope), None)
        if failure is not None:
            path = "/".join(map(str, failure.absolute_path))
            raise ValueError(f"Response {checked + 1} violates {failure.validator} at {path}")
        checked += 1
    if checked == 0:
        raise ValueError("Empty schema test input")
    print(json.dumps({"schema_responses": checked, "validator": "jsonschema 4.25.1", "status": "PASS"}))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())

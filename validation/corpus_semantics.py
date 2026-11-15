"""Compare every industrial record's actual PostgreSQL observations with complete core outcomes."""
from collections import Counter
from datetime import datetime
from decimal import Decimal
import json
import corpus_ddl
import corpus_effects
from corpus_effects import ownership_key as effects_key

from corpus_schema import group
from corpus_wire import MANIFEST, cell, request, sql_type, state, typed_rows
from oracle_contract import index_responses, top_level_ordered
from rejections import execute
from run import CLI, digest, write_json


def normalized(value: dict) -> str:
    """Use SQL-family value equality for observations, retaining exact transport in evidence hashes."""
    kind, payload = value["type"], value["value"]
    if kind == "null": return json.dumps(["null", family(payload)], sort_keys=True)
    if kind != "scalar": return json.dumps([kind, payload], sort_keys=True)
    ty, text = payload["ty"], payload["value"]
    if isinstance(ty, dict) and "numeric" in ty: return "numeric:" + decimal_text(Decimal(text))
    if ty == "float64": return "float64:" + ("0" if float(text) == 0 else str(float(text)))
    if ty in ("timestamp", "timestamptz"):
        if text not in ("infinity", "-infinity"): text = datetime.fromisoformat(text.replace(" ", "T")).isoformat()
    if family(ty) == "bpchar": text = text.rstrip(" ")
    if ty == "bytea": text = text.lower()
    if ty == "jsonb": text = json_semantic(json.loads(text, parse_int=Decimal, parse_float=Decimal))
    return json.dumps([family(ty), text], sort_keys=True)


def decimal_text(value: Decimal) -> str:
    """Numeric scale and signed zero do not change SQL duplicate equality."""
    if value.is_nan(): return "NaN"
    if value.is_infinite(): return "-Infinity" if value.is_signed() else "Infinity"
    if value.is_zero(): return "0"
    sign, digits, exponent = value.as_tuple()
    digits = list(digits)
    while digits[-1] == 0: digits.pop(); exponent += 1
    return ("-" if sign else "") + "".join(map(str, digits)) + "e" + str(exponent)


def family(ty: object) -> object:
    """PostgreSQL result OIDs retain type families but omit column typmods."""
    if not isinstance(ty, dict): return ty
    if "array" in ty: return ["array", family(ty["array"])]
    if "char" in ty: return "bpchar"
    if "enum" in ty: return ["enum", ty["enum"]["name"]]
    return next(iter(ty))


def json_semantic(value: object) -> object:
    """Canonicalize JSON numbers without rounding through Python's binary floating point."""
    if isinstance(value, Decimal): return ["number", decimal_text(value)]
    if isinstance(value, dict): return ["object", sorted((key, json_semantic(item)) for key, item in value.items())]
    if isinstance(value, list): return ["array", [json_semantic(item) for item in value]]
    return [type(value).__name__, value]


def rows_key(rows: list, ordered: bool = False) -> list:
    """Keep duplicates and preserve result order only when the bound query observes it."""
    values = [tuple(normalized(value) for value in row) for row in rows]
    return values if ordered else sorted(values)


def pg_rows(result: dict, enums: dict) -> list:
    """Recover actual PostgreSQL result types from OID-backed stable type observations."""
    types = [sql_type(column["type"], enums) for column in result["columns"]]
    return typed_rows(result["rows"], types)


def database_matches(actual: dict, expected: dict, catalog: dict) -> bool:
    """Compare complete changed and unchanged table bags, never just the mutation target."""
    if set(actual) != set(catalog["tables"]): return False
    identifiers = {key: key if key in expected else table["name"] for key, table in catalog["tables"].items()}
    if len(set(identifiers.values())) != len(identifiers) or set(identifiers.values()) != set(expected): return False
    for key, table in catalog["tables"].items():
        rows = typed_rows(expected[identifiers[key]]["rows"], [column["ty"] for column in table["columns"]])
        if any(len(row) != len(table["columns"]) for row in actual[key]): return False
        if rows_key(actual[key]) != rows_key(rows): return False
    return True


def observation_matches(outcome: dict, expected: dict, after: dict, catalog: dict, enums: dict, ordered: bool) -> bool:
    """SQL errors, output ordering, affected counts and full poststate are separate obligations."""
    value = outcome["outcome"]
    if expected["status"] == "error":
        return outcome["status"] == "sql_error" and all(value.get(key) == expected.get(key)
            for key in ("sqlstate", "constraint_name", "column_name", "exposed_rows"))
    if outcome["status"] != "succeeded": return False
    if value["ordering"] != ("ordered" if ordered else "bag"): return False
    if value["affected"] != expected["affected_rows"] or value["command_tag"] != expected["command"]: return False
    if rows_key(value["rows"], ordered) != rows_key(pg_rows(expected, enums), ordered): return False
    return database_matches(value["database"], after["database"], catalog)


def compare(manifest: dict, evidence: dict, context: dict, response: dict, submitted: dict) -> dict:
    """Keep unsupported paths and unobserved effects out of the completed semantic count."""
    base = {"binary_sha256": digest(CLI.read_bytes()), "request_sha256": digest(json.dumps(submitted, sort_keys=True).encode()),
            "response_sha256": digest(json.dumps(response, sort_keys=True).encode()), "fixtures": 1}
    if response.get("status") != "ok": return dict(base, status="uncovered", response=response)
    if response.get("validation") != "ALLOY_TABLE_ROUNDTRIP" or not response.get("complete"):
        return dict(base, status="uncovered", reason="Complete extended outcome evidence absent")
    semantic = context["semantic_context"]
    if evidence["result"]["status"] == "ok" and not schema_matches(response["schema"], evidence["result"]["columns"], semantic["enums"]):
        return dict(base, status="mismatch", reason="output_schema", response=response)
    after = state(manifest, evidence, "after")
    if evidence.get("system_context") is not None:
        after["database"].update(evidence["system_context"]["relations_after"])
    ordered = top_level_ordered(submitted["sql"])
    matches = [value for value in response["outcomes"] if observation_matches(value, evidence["result"], after, submitted["catalog"], semantic["enums"], ordered)]
    if not matches: return dict(base, status="mismatch", response=response)
    matches = [value for value in matches if catalog_matches(value, manifest, evidence, submitted, after)]
    if not matches: return dict(base, status="mismatch", reason="catalog_effects", response=response)
    matches, pending, reason = corpus_effects.compare(evidence, state(manifest, evidence, "before"), after, matches)
    if not matches: return dict(base, status="mismatch", reason=reason, response=response)
    if pending:
        return dict(base, status="observations_match_effects_pending", pending_effects=pending, response=response)
    return dict(base, status="matched_pg_outcome_and_alloy_table", response=response,
                claim="One authentic-context fixture; PostgreSQL observation belongs to complete Rust outcome set. Alloy validates table selection, not independent SQL semantics.")


def catalog_matches(outcome: dict, manifest: dict, evidence: dict, submitted: dict, after: dict) -> bool:
    """Catalog mutation cannot disappear because result rows and physical table state happen to match."""
    if outcome["status"] != "succeeded": return True
    actual = outcome["outcome"].get("catalog")
    before = state(manifest, evidence, "before")
    if actual is None: return False
    if "catalog" not in before or "catalog" not in after: return False
    return corpus_ddl.matches(actual, before["catalog"], after["catalog"], submitted["catalog"], submitted["sql"])


def catalog_unchanged(actual: dict | None, submitted: dict) -> bool:
    """Require the complete unchanged catalog while normalizing only optional transport defaults."""
    if not isinstance(actual, dict): return False
    values = []
    for source in (actual, submitted):
        value = dict(source)
        value["tables"] = corpus_ddl.normalized_tables(source["tables"])
        value.setdefault("namespace", None)
        value.setdefault("views", {})
        value.setdefault("table_semantics", {})
        values.append(value)
    return values[0] == values[1]


def prepare(manifest: dict) -> tuple[list, dict]:
    """Account for every record, including explicit context-construction blockers."""
    contexts = {item["group"]: item for item in manifest["authoritative_contexts"]}
    requests, indexed = [], {}
    for case in manifest["cases"]:
        context = contexts[group(case["id"])]
        case["semantic_fixtures"] = {}
        for label, evidence in [("baseline", case["postgres_execution"]), *case.get("postgres_fixtures", {}).items()]:
            try:
                wire = request(manifest, case, context, evidence)
                wire["id"] += "#" + label
                requests.append(wire)
                indexed[wire["id"]] = (case, label, evidence, context, wire)
            except (ValueError, KeyError) as error:
                case["semantic_fixtures"][label] = {"status": "context_uncovered", "reason": str(error), "fixtures": 0}
    return requests, indexed


def schema_matches(actual: list, expected: list, enums: dict) -> bool:
    """Compare labels and resolved type families even when all result rows are NULL or absent."""
    if len(actual) != len(expected): return False
    return all(a["name"] == b["name"] and family(a["ty"]) == family(sql_type(b["type"], enums)) for a, b in zip(actual, expected))


def summarize(manifest: dict) -> None:
    """Credit a source record only after every declared fixture and its effects have matched."""
    for case in manifest["cases"]:
        stages = [value["status"] for value in case["semantic_fixtures"].values()]
        status = "matched_pg_outcome_and_alloy_table" if stages and all(value == "matched_pg_outcome_and_alloy_table" for value in stages) else "uncovered"
        if "mismatch" in stages: status = "mismatch"
        case["semantic_execution"] = {"status": status, "fixtures": len(stages), "stages": dict(Counter(stages)), "binary_sha256": digest(CLI.read_bytes())}
    manifest["counts"]["semantic_execution"] = dict(Counter(case["semantic_execution"]["status"] for case in manifest["cases"]))
    manifest["counts"]["semantic_fixtures"] = dict(Counter(value["status"] for case in manifest["cases"] for value in case["semantic_fixtures"].values()))


def main() -> None:
    """Update the canonical ledger with current binary evidence; never retain stale passes."""
    manifest = json.loads(MANIFEST.read_text(encoding="utf-8"))
    requests, indexed = prepare(manifest)
    responses = execute(requests, timeout=600)
    responses = index_responses(requests, responses)
    for identifier, (case, label, evidence, context, wire) in indexed.items():
        case["semantic_fixtures"][label] = compare(manifest, evidence, context, responses[identifier], wire)
    summarize(manifest)
    manifest["semantic_coverage_target"] = {"records": 276, "definition": "Correct semantics under explicit contexts, including expected SQL errors; parser acceptance and refusals never count.",
        "counts_are_fixture_evidence": True, "binary_sha256": digest(CLI.read_bytes())}
    write_json(MANIFEST, manifest)
    print(json.dumps(manifest["counts"]["semantic_execution"]))
    if manifest["counts"]["semantic_execution"] != {"matched_pg_outcome_and_alloy_table": 276}: raise SystemExit(1)


if __name__ == "__main__":
    main()

"""Exercise durable real-Alloy enumeration across process restarts and hostile archive edits."""
import json
import os
import tempfile
from pathlib import Path

from cases import fixture, table, typed_cell
from process_helpers import communicate
from run import CLI, ROOT, digest, write_json


def call(action: dict) -> dict:
    """Use a fresh CLI/JVM for every command, proving that progress is not process-local."""
    wire = communicate([str(CLI)], json.dumps(action, ensure_ascii=False) + "\n", 100, ROOT)
    lines = wire.splitlines()
    if len(lines) != 1: raise ValueError("Expected exactly one protocol response")
    return json.loads(lines[0])


def require(value: bool, message: str) -> None:
    """Reject a validation claim immediately when observed evidence contradicts it."""
    if not value: raise ValueError(message)


def request(sql: str = "SELECT x FROM r") -> dict:
    """Declare all tiny symbolic worlds independently from the solver's chosen solution."""
    result = fixture("instances", sql, {"r": table([("x", "int32")], [])}, ["int32"], "instances")["request"]
    result.update(mode="analyze", database={}, bounds={"public.r": 1},
                  domain=[typed_cell(0, "int32"), typed_cell(1, "int32")],
                  property={"kind": "only_value", "column": 0, "value": typed_cell(0, "int32")})
    return result


def start(submitted: dict, kind: str = "example", limit: int = 32) -> dict:
    """Start an immutable bundle with an explicit command identity and bounded journal."""
    result = call({"action": "instances.start", "request": submitted, "kind": kind, "max_instances": limit})
    require(result.get("status") == "ok", str(result))
    return result


def pointer(instance: dict, reference: str):
    """Resolve JSON Pointer independently to check every AI/Graphviz grounding reference."""
    value = instance
    for segment in reference.split("/")[1:]:
        key = segment.replace("~1", "/").replace("~0", "~")
        value = value[int(key)] if isinstance(value, list) else value[key]
    return value


def check_instance(response: dict, kind: str, profile: str) -> dict:
    """Require command identity, complete tuples, row references, and separate semantic identity."""
    require(response["assurance_kind"] == profile, str(response))
    record, instance = response["record"], response["record"]["instance"]
    require(record["solver"]["command_kind"] == ("run" if kind == "example" else "check"), "Wrong command")
    require(instance["kind"] == kind and instance["validation"] == "RUST_REPLAY_CONFIRMED", "Wrong evidence label")
    require(instance["model_revision_sha256"] == response["model_revision_sha256"], "Revision mismatch")
    require(instance["semantic_sha256"] == canonical_hash(instance["semantic"]), "Semantic digest mismatch")
    require(instance["alloy_relations_sha256"] == canonical_hash(instance["alloy_relations"]), "Tuple digest mismatch")
    require(all(pointer(instance, fact["reference"]) == fact["value"] for fact in instance["facts"]), "Broken grounding reference")
    require(all(isinstance(t, list) for rows in instance["alloy_relations"].values() for t in rows), "Tuple order erased")
    require(not response["enumeration_exhausted"], "SAT labeled exhausted")
    return instance


def canonical_hash(value) -> str:
    """Recompute Rust serde_json Value digests without trusting archived checksum strings."""
    return digest(json.dumps(value, sort_keys=True, ensure_ascii=False, separators=(",", ":")).encode())


def exhaust(first: dict, kind: str, profile: str) -> tuple[list, dict]:
    """Request every next solution and distinguish actual UNSAT from a configured limit."""
    response, instances = first, []
    while response.get("enumeration_status") == "instance":
        instance = check_instance(response, kind, profile)
        instances.append(instance)
        response = call({"action": "instances.next", "cursor": response["cursor"]})
        require(response.get("status") == "ok", str(response))
        require(len(instances) <= 32, "Unexpected unbounded tiny-model enumeration")
    require(response["enumeration_status"] == "exhausted" and response["enumeration_exhausted"], str(response))
    require(response["record"]["solver"]["status"] == "UNSAT" and response["cursor"] is None, "False exhaustion")
    return instances, response


def native_checks() -> tuple[dict, dict]:
    """Check complete native worlds, every actual counterexample, and immutable prefix reopening."""
    profile = "native_ra_alloy_with_rust_replay"
    first = start(request())
    repeated = start(request())
    require(first == repeated, "Repeated start changed immutable evidence")
    second = call({"action": "instances.next", "cursor": first["cursor"]})
    repeated_next = call({"action": "instances.next", "cursor": first["cursor"]})
    require(second == repeated_next, "Cursor replay not idempotent: " + json.dumps([second, repeated_next]))
    rows, terminal = exhaust(first, "example", profile)
    observed = {canonical_hash(row["semantic"]["database"]["public.r"]) for row in rows}
    expected = {canonical_hash(bag) for bag in [[], [[typed_cell(0, "int32")]], [[typed_cell(1, "int32")]]]}
    require(observed == expected, "Native enumeration missed or invented a declared world")
    require(all(row["semantic"]["output"] == row["semantic"]["database"]["public.r"] for row in rows), "Native replay mismatch")
    counter, _ = exhaust(start(request(), "counterexample"), "counterexample", profile)
    require(counter and all(any(c[0]["value"] == 1 for c in row["semantic"]["output"]) for row in counter), "Nonviolating counterexample")
    opened = call({"action": "instances.open", "bundle_id": first["bundle_id"]})
    require(opened["record"] == terminal["record"] and len(opened["records"]) == len(rows) + 1, "Open advanced or lost prefix")
    require(opened["bundle"]["request"]["sql"] == "SELECT x FROM r" and opened["bundle"]["ir"] and opened["bundle"]["model"], "Missing SQL/IR/Alloy triple")
    return {"examples": len(rows), "distinct_semantic_worlds": len(observed), "counterexamples": len(counter)}, first


def extended_checks() -> dict:
    """Enumerate evaluator-backed transitions and retain honest table provenance and SQL errors."""
    profile = "evaluator_backed_table_with_rust_replay"
    rows, _ = exhaust(start(request("SELECT x + 1 FROM r")), "example", profile)
    require(len(rows) == 3, "Expected exactly three canonical finite input worlds")
    for row in rows:
        semantic = row["semantic"]
        expected = [[typed_cell(c[0]["value"] + 1, "int32")] for c in semantic["database"]["public.r"]]
        require(semantic["outcome"]["outcome"]["rows"] == expected, "Arithmetic transition mismatch")
    submitted = request("SELECT 10 / x FROM r")
    submitted["property"] = {"kind": "no_sql_error"}
    errors, _ = exhaust(start(submitted, "counterexample"), "counterexample", profile)
    require(len(errors) == 1, "Expected exactly one zero-divisor counterexample")
    outcome = errors[0]["semantic"]["outcome"]
    require(outcome["status"] == "sql_error" and outcome["outcome"]["sqlstate"] == "22012", str(outcome))
    return {"examples": len(rows), "sql_error_counterexamples": len(errors)}


def restart_stability(first: dict) -> dict:
    """Require repeated fresh-process prefix reconstruction to preserve the same next record."""
    responses = [call({"action": "instances.next", "cursor": first["cursor"]}) for _ in range(8)]
    require(all(row.get("status") == "ok" for row in responses), "Restart refusal: " + json.dumps(responses))
    require(all(row == responses[0] for row in responses), "Restart changed committed instance")
    return {"independent_restarts": len(responses), "identical_committed_records": True}


def safe_and_grounded() -> dict:
    """Test initial check-UNSAT and duplicate Unicode facts with escaped identifier pointers."""
    safe = start(request("SELECT x FROM r WHERE x = 0"), "counterexample")
    require(safe["enumeration_status"] == "exhausted" and safe["enumeration_exhausted"], "Safe check not UNSAT")
    require(safe["record"]["ordinal"] == 0 and safe["record"]["instance"] is None, "Invented safe-check example")
    submitted = fixture("unicode", "SELECT x FROM r", {"r": table([("x", "text")], [["<script>Ω"], ["<script>Ω"]])}, ["text"], "instances")["request"]
    submitted["catalog"]["tables"]["public.r/~"] = submitted["catalog"]["tables"].pop("public.r")
    submitted["database"]["public.r/~"] = submitted["database"].pop("public.r")
    first = start(submitted)
    value = check_instance(first, "example", "native_ra_alloy_with_rust_replay")
    facts = [fact for fact in value["facts"] if fact["kind"] == "input_row"]
    require(len(facts) == 2 and len({fact["reference"] for fact in facts}) == 2, "Duplicate occurrences lost")
    require(all("~1~0" in fact["reference"] for fact in facts), "Identifier pointer not escaped")
    other = request("SELECT x FROM r WHERE x = $1")
    other["parameters"] = {"1": typed_cell(0, "int32")}
    revision = start(other, limit=1)["model_revision_sha256"]
    other["parameters"]["1"] = typed_cell(1, "int32")
    require(start(other, limit=1)["model_revision_sha256"] != revision, "Changed parameter retained same revision")
    return {"initial_check_unsat": True, "unicode_duplicate_facts": len(facts), "context_revision_changes": True}


def limits_and_protocol() -> dict:
    """Require explicit capacity, property, schema and path failures without exhaustion claims."""
    first = start(request(), limit=1)
    limited = call({"action": "instances.next", "cursor": first["cursor"]})
    require(limited["enumeration_status"] == "limit" and not limited["enumeration_exhausted"], "Limit claimed UNSAT")
    require(limited["record"]["solver"]["status"] == "NOT_REQUESTED", "Unexpected limit solver claim")
    actions = [{"action": "instances.next", "cursor": "../../outside"},
               {"action": "instances.open", "bundle_id": "0" * 64},
               {"action": "instances.next", "cursor": first["cursor"], "ignored": True},
               {"action": "instances.start", "request": request(), "kind": "example", "max_instances": 0}]
    for action in actions:
        result = call(action)
        require(result["status"] == "error" and not result["enumeration_exhausted"], "Protocol error became safe result")
    concrete = fixture("no-property", "SELECT 1", {}, ["int32"], "instances")["request"]
    result = call({"action": "instances.start", "request": concrete, "kind": "counterexample"})
    require(result.get("code") == "evidence.property", str(result))
    return {"limit_distinct_from_exhaustion": True, "protocol_refusals": len(actions) + 1}


def tamper(path: Path, action: dict, edit, rehash: bool = False) -> dict:
    """Restore an exact owned archive after each hostile edit, even when a test fails."""
    original = path.read_bytes()
    value = json.loads(original)
    edit(value)
    if rehash: value["sha256"] = canonical_hash(value["payload"])
    try:
        path.write_text(json.dumps(value, ensure_ascii=False), encoding="utf-8")
        result = call(action)
        require(result["status"] == "error" and not result["enumeration_exhausted"], "Corruption accepted")
        return {"code": result["code"]}
    finally:
        path.write_bytes(original)


def corruption_checks(store: Path, first: dict) -> dict:
    """Detect damaged digests and validly rehashed but solver-inconsistent typed journal records."""
    directory = store / "bundles" / first["bundle_id"]
    action = {"action": "instances.open", "bundle_id": first["bundle_id"]}
    bad = lambda value: value["payload"]["instance"]["semantic"].update(output=[])
    record = tamper(directory / "0000.json", action, bad)
    require(record["code"] == "store.corrupt", str(record))
    changed = tamper(directory / "0000.json", action, bad, True)
    require(changed["code"] == "resume.prefix_changed", str(changed))
    bundle = tamper(directory / "bundle.json", action, lambda value: value["payload"].update(model="run {}"))
    require(bundle["code"] == "store.corrupt", str(bundle))
    missing, original = directory / "0001.json", (directory / "0001.json").read_bytes()
    try:
        missing.unlink()
        result = call(action)
        require(result.get("code") == "store.corrupt", "Journal gap was ignored")
    finally:
        missing.write_bytes(original)
    require(call(action)["status"] == "ok", "Exact archive restoration did not recover")
    return {"digest_damage": record, "rehashed_semantic_tamper": changed, "bundle_damage": bundle, "gap_refused": True}


def run_suite(store: Path) -> dict:
    """Run all adversarial cases inside one isolated, task-owned archive namespace."""
    os.environ["SQLolla_INSTANCE_STORE"] = str(store)
    native, first = native_checks()
    stability = restart_stability(first)
    extended = extended_checks()
    grounding = safe_and_grounded()
    limits = limits_and_protocol()
    corruption = corruption_checks(store, first)
    return {"native": native, "extended": extended, "limits": limits, "corruption": corruption,
            "restart_stability": stability, "grounding": grounding,
            "fresh_process_per_command": True, "hashes_are_not_authentication": True}


def main() -> None:
    """Publish bounded runtime evidence and remove only this harness's temporary archive."""
    parent = ROOT / ".scratch"
    parent.mkdir(exist_ok=True)
    old = os.environ.get("SQLolla_INSTANCE_STORE")
    try:
        with tempfile.TemporaryDirectory(prefix="instance-validation-", dir=parent) as name:
            result = run_suite(Path(name))
        result.update(status="passed", cli_sha256=digest(CLI.read_bytes()), temporary_store_removed=not Path(name).exists())
        write_json(ROOT / "validation/results/instances.json", result)
        print(json.dumps(result))
    finally:
        if old is None: os.environ.pop("SQLolla_INSTANCE_STORE", None)
        else: os.environ["SQLolla_INSTANCE_STORE"] = old


if __name__ == "__main__":
    main()

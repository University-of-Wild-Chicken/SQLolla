"""Check cross-revision review refinement against real Alloy and PostgreSQL16 behavior."""
from collections import Counter
import json
import os
from pathlib import Path
import tempfile

from cases import fixture, table, typed_cell
from extended_symbolic import matches, oracle
from instances import call, pointer, require, start
from libpq_client import PgConnection
from pg_runtime import TemporaryPostgres
from run import CLI, ROOT, digest


def request(sql: str, values: list, policy: dict) -> dict:
    """Declare every bounded typed world independently of the solver's selected witness."""
    case = fixture("refinement", sql, {"r": table([("x", "int32")], [])}, ["int32"], "refinement")
    result = case["request"]
    result.update(mode="analyze", database={}, bounds={"public.r": 2},
                  domain=[typed_cell(value, "int32") for value in values], property=policy)
    return result


def saved(first: dict, expected: set) -> dict:
    """Find desired typed input/outcome observations without depending on Alloy atom order."""
    found, response = {}, first
    while response.get("enumeration_status") == "instance":
        instance = response["record"]["instance"]
        semantic = instance["semantic"]
        behavior = semantic.get("outcome", {})
        output = behavior.get("outcome", {})
        rows = values(semantic.get("output", output.get("rows", [])))
        key = (values(semantic["database"]["public.r"]), output.get("sqlstate", rows))
        if key in expected:
            found[key] = {"bundle_id": first["bundle_id"], **{k: instance[k] for k in
                          ("instance_id", "model_revision_sha256", "semantic_sha256")}}
        if len(found) == len(expected):
            return found
        response = call({"action": "instances.next", "cursor": response["cursor"]})
    raise ValueError(f"Missing selected Alloy observations: {expected - found.keys()}")


def values(rows: list) -> tuple:
    """Read unary int32 bags with an explicit typed SQL NULL."""
    return tuple(None if cell["type"] == "null" else cell["value"] for (cell,) in rows)


def label(selected: dict, event: str, disposition: str) -> dict:
    """Transmit only canonical archive identities alongside the user's active label."""
    return dict(selected, event_id=event, disposition=disposition)


def context(base: dict, labels: list, expected: str = "ok") -> dict:
    """Open a deterministic snapshot and validate every returned fact reference."""
    action = {"action": "refinement.context", "id": "context", "feedback": labels,
              **{k: base[k] for k in ("bundle_id", "model_revision_sha256")}}
    response = call(action)
    require(response.get("code", response.get("status")) == expected, str(response))
    if expected == "ok":
        packet = response["packet"]
        require(audit_packet(packet, labels, response["references"]), "Incomplete or ungrounded review packet")
    return response


def audit_packet(packet: dict, labels: list, references: list) -> bool:
    """Check exact source artifacts, complete labels and resolvable packet-relative references."""
    triple = packet["source_triple"]
    return (triple["sql"] == packet["request"]["sql"] and bool(triple["ir"]) and bool(triple["alloy"])
            and len(packet["feedback"]) == len(labels)
            and set(references) == {path for entry in packet["feedback"] for path in entry["references"]}
            and all(pointer(packet, path) is not None for path in references))


def check(snapshot: dict, labels: list, sql: str, expected: str = "ok") -> dict:
    """Submit only the proposed SQL against the exact previously returned snapshot digest."""
    action = {"action": "refinement.check", "id": "check", "sql": sql, "feedback": labels,
              **{k: snapshot[k] for k in ("bundle_id", "model_revision_sha256", "feedback_sha256")}}
    response = call(action)
    require(response.get("code", response.get("status")) == expected, str(response))
    require(response["ready"] == ("candidate_request" in response), "Candidate request exposed without readiness")
    return response


def compare_cases(snapshot: dict, report: dict, sql: str, expected: dict) -> bool:
    """Compare each reviewed behavior with all independently observed outcomes for its exact input."""
    request = snapshot["packet"]["request"]
    case = {"request": dict(request, sql=sql)}
    decisions = {decision["event_id"]: decision for decision in report["cases"]}
    for entry in snapshot["packet"]["feedback"]:
        decision = decisions[entry["event_id"]]
        if entry["disposition"] == "unsure":
            require(decision["status"] == "advisory", "Unsure affected the verdict")
            continue
        observation = entry["observation"]
        rows = values(observation["database"]["public.r"])
        pg = next(row["allowed"] for row in expected["observations"]
                  if Counter(row["input"]) == Counter(rows))
        count = sum(matches(case, observation["outcome"], alternate) for alternate in pg)
        passes = (count > 0) == (entry["disposition"] == "accepted_use_case")
        require(decision["matching_outcome_count"] == count, f"Wrong match count: {decision}")
        require(decision["candidate_outcome_count"] == len(pg), f"Incomplete PG alternatives: {decision}")
        require(decision["status"] == ("satisfied" if passes else "unsatisfied"), str(decision))
    return all(decision["status"] in ("satisfied", "advisory") for decision in decisions.values())


def verify(connection, snapshot: dict, labels: list, sql: str, domain: list) -> dict:
    """Require both full feedback replay and the real unchanged bounded assertion verdict."""
    report = check(snapshot, labels, sql)
    request = snapshot["packet"]["request"]
    expected = oracle(connection, {"request": dict(request, sql=sql), "domain": domain})
    fails = expected["has_counterexample"]
    verdict = report["property_check"]
    require(verdict["assumptions"] == "SATISFIABLE" and verdict["property"] == request["property"], str(verdict))
    require(verdict["status"] == ("COUNTEREXAMPLE" if fails else "NO_COUNTEREXAMPLE_WITHIN_BOUNDS"), str(verdict))
    solver = verdict["solver"]
    require(solver["command_kind"] == "check" and solver["command_label"] == "QueryProperty"
            and solver["status"] == ("SAT" if fails else "UNSAT"), str(solver))
    fits = compare_cases(snapshot, report, sql, expected)
    require(report["ready"] == (fits and not fails), str(report))
    if report["ready"]:
        require(report["candidate_request"] == dict(request, sql=sql), "Candidate changed immutable basis")
    return report


def invalid_labels(base: dict, labels: list, snapshot: dict, sql: str) -> None:
    """Reject stale, forged, duplicate and advisory-only snapshots before candidate use."""
    context(base, [], "refinement.feedback_required")
    context(base, [label for label in labels if label["disposition"] == "unsure"], "refinement.feedback_required")
    context(base, labels + labels[:1], "refinement.duplicate_feedback")
    context(base, [dict(labels[0], semantic_sha256="f" * 64), *labels[1:]], "refinement.semantic_identity")
    check(snapshot, [dict(labels[0], rationale="new reason"), *labels[1:]], sql, "refinement.snapshot_changed")
    check(snapshot, labels, "SELECT x AS renamed FROM r", "refinement.output_schema")
    altered = dict(snapshot["packet"]["request"], bounds={"public.r": 1})
    other = start(altered)
    witness = other["record"]["instance"]
    item = {"bundle_id": other["bundle_id"], **{k: witness[k] for k in
            ("instance_id", "model_revision_sha256", "semantic_sha256")}}
    context(base, [*labels, label(item, "different-bound", "accepted_use_case")], "refinement.incompatible_basis")


def native(connection) -> None:
    """Preserve actual duplicate rows and typed NULLs across bundles and adoption."""
    domain, original = [None, 0, 1], "SELECT x FROM r"
    submitted = request(original, domain, {"kind": "no_value", "column": 0, "value": typed_cell(1, "int32")})
    base, counter = start(submitted), start(submitted, "counterexample")
    selected = saved(base, {((0, 0), (0, 0)), ((None,), (None,)), ((1,), (1,))})
    rejected = saved(counter, {((1,), (1,))})[((1,), (1,))]
    labels = [label(selected[((0, 0), (0, 0))], "retain-duplicates", "accepted_use_case"),
              label(selected[((None,), (None,))], "keep-null", "accepted_use_case"),
              label(rejected, "reject-one", "unintended_behavior"),
              label(selected[((1,), (1,))], "undecided-example", "unsure")]
    snapshot = context(base, labels)
    require(context(base, labels[::-1])["feedback_sha256"] == snapshot["feedback_sha256"], "Order changed snapshot")
    sql = "SELECT x FROM r WHERE COALESCE(x <> 1, TRUE)"
    invalid_labels(base, labels, snapshot, sql)
    context(base, [*labels[:3], dict(labels[3], disposition="accepted_use_case")], "refinement.conflicting_feedback")
    require(verify(connection, snapshot, labels, "SELECT DISTINCT x FROM r WHERE x IS NULL OR x <> 1", domain)["ready"] is False,
            "Loss of a duplicate passed")
    require(verify(connection, snapshot, labels, original, domain)["ready"] is False, "Unsafe SQL was ready")
    yes_only = context(base, labels[:2])
    require(verify(connection, yes_only, labels[:2], original, domain)["ready"] is False, "Review alone overrode Alloy")
    promoted = verify(connection, snapshot, labels, sql, domain)
    require(promoted["ready"] and promoted["property_check"]["assurance_kind"] != base["assurance_kind"],
            "Valid candidate across encoding profiles was refused")
    adopted = context(start(promoted["candidate_request"]), labels)
    require(adopted["feedback_sha256"] != snapshot["feedback_sha256"], "New base reused a stale snapshot")
    require(all(value["sql"] == original for value in adopted["packet"]["review_sources"].values()), "Prior source lost")
    require(verify(connection, adopted, labels, sql, domain)["ready"], "Prior reviews failed after adoption")


def finite(connection) -> None:
    """Require all LIMIT choices and complete SQL error observations in the bounded models."""
    domain, original = [0, 1], "SELECT x FROM r LIMIT 1"
    base = start(request(original, domain, {"kind": "max_rows", "max": 1}))
    selected = saved(base, {((0, 1), (0,)), ((0, 1), (1,))})
    labels = [label(selected[((0, 1), (0,))], "limit-yes", "accepted_use_case"),
              label(selected[((0, 1), (1,))], "limit-no", "unintended_behavior")]
    snapshot = context(base, labels)
    require(verify(connection, snapshot, labels, original, domain)["ready"] is False, "Allowed rejected LIMIT branch")
    require(verify(connection, snapshot, labels, "SELECT x FROM r WHERE x = 0 LIMIT 1", domain)["ready"],
            "Preserved LIMIT branch was refused")
    sql_error(connection, domain)


def sql_error(connection, domain: list) -> None:
    """Distinguish original division-by-zero from a safe replacement preserving success."""
    original = "SELECT 10 / x AS x FROM r"
    submitted = request(original, domain, {"kind": "no_sql_error"})
    base, counter = start(submitted), start(submitted, "counterexample")
    selected = saved(base, {((1,), (10,))})[((1,), (10,))]
    rejected = saved(counter, {((0,), "22012")})[((0,), "22012")]
    labels = [label(selected, "division-yes", "accepted_use_case"),
              label(rejected, "zero-divisor", "unintended_behavior")]
    snapshot = context(base, labels)
    require(verify(connection, snapshot, labels, original, domain)["ready"] is False, "Allowed division by zero")
    require(verify(connection, snapshot, labels, "SELECT 10 / x AS x FROM r WHERE x <> 0", domain)["ready"],
            "Safe division was refused")


def main() -> None:
    """Run isolated real PostgreSQL, Alloy and durable review journals, then remove owned records."""
    scratch = (ROOT / ".scratch").resolve()
    scratch.mkdir(exist_ok=True)
    previous = os.environ.get("SQLolla_INSTANCE_STORE")
    with tempfile.TemporaryDirectory(prefix="refinement-", dir=scratch) as store:
        require(Path(store).resolve().parent == scratch, "Unexpected refinement journal location")
        os.environ["SQLolla_INSTANCE_STORE"] = store
        try:
            with TemporaryPostgres() as server, PgConnection(server) as connection:
                native(connection)
                finite(connection)
                postgres = server.execute("SELECT version();").strip()
        finally:
            if previous is None:
                os.environ.pop("SQLolla_INSTANCE_STORE", None)
            else:
                os.environ["SQLolla_INSTANCE_STORE"] = previous
    require(not Path(store).exists() and not server.directory.exists(), "Owned test directories still exist")
    print(json.dumps({"status": "passed", "cli_sha256": digest(CLI.read_bytes()), "postgres": postgres,
                      "cases": ["typed_null_and_duplicates", "cross_bundle_adoption", "unordered_limit", "sql_error"]}))


if __name__ == "__main__":
    main()

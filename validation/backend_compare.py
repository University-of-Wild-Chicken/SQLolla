"""Compare backend signal observations without equating successful requests with immediate exit."""
from copy import deepcopy

from corpus_semantics import catalog_unchanged, observation_matches, schema_matches
from oracle_contract import index_responses
from rejections import execute


def request(case: dict) -> dict:
    """Only catalog observations and explicit controlled-target assumptions become executor input."""
    return {"id": case["id"], "sql": case["sql"], "catalog": case.get("catalog", {"search_path": ["public"], "tables": {}}),
            "database": case.get("database", {}), "parameters": {}, "mode": "compare", "execution": {
                "max_rows": 256, "max_outcomes": 256, "settings": {}, "table_metadata": {},
                "backend_signals": case["context"]}}


def expected_events(case: dict) -> list:
    """Actual notices and the successful owned-worker call identify separate external observations."""
    events = []
    for notice in case["notices"]:
        if notice["severity"] != "WARNING": raise ValueError("Unexpected notice severity")
        events.append({"kind": "sql_warning", "sqlstate": notice["sqlstate"], "message": notice["message"]})
    if case["signal_requested"] is not None:
        events.append({"kind": "backend_termination_requested", "pid": case["signal_requested"]})
    return events


def compare(case: dict, wire: dict, response: dict) -> dict:
    """Compare every complete outcome including warnings, permission errors and catalog frame."""
    errors, expected = [], case["postgres"]
    if response.get("status") != "ok" or response.get("validation") != "ALLOY_TABLE_ROUNDTRIP" or not response.get("complete"):
        errors.append("incomplete_execution")
    else:
        if not response["outcomes"]: errors.append("empty_outcome_set")
        for outcome in response["outcomes"]:
            if not observation_matches(outcome, expected, {"database": {}}, wire["catalog"], {}, False): errors.append("postgres_result")
            if outcome["outcome"]["effects"] != expected_events(case): errors.append("external_observations")
            if outcome["status"] == "succeeded" and not catalog_unchanged(outcome["outcome"].get("catalog"), wire["catalog"]): errors.append("catalog_frame")
        if expected["status"] == "ok" and not schema_matches(response["schema"], expected["columns"], {}): errors.append("schema")
    if case["signal_requested"] is not None and (not case["worker_after_rollback"].get("closed") or case["target_present_after_rollback"]):
        errors.append("owned_worker_did_not_exit")
    if case["id"] == "backend/permission" and (case["worker_after_rollback"]["status"] != "ok" or not case["target_present_after_rollback"]):
        errors.append("permission_denial_changed_worker")
    return {"id": case["id"], "status": "failed" if errors else "passed", "errors": errors, "request": wire, "response": response}


def refusals(positive: dict) -> list:
    """Unspecified lifecycle, waiting, capability and width cases are explicit coverage refusals."""
    pid = positive["signal_requested"]
    variants = [("self", f"SELECT pg_terminate_backend({positive['context']['actor_pid']})"),
                ("wait", f"SELECT pg_terminate_backend({pid},1)"),
                ("repeat", f"SELECT pg_terminate_backend({pid}),pg_terminate_backend({pid})"),
                ("width", "SELECT pg_terminate_backend(0,2147483648)"),
                ("capability", f"SELECT pg_terminate_backend({pid})")]
    result = []
    for name, sql in variants:
        wire = request(dict(positive, id="backend/refusal/" + name, sql=sql))
        wire = deepcopy(wire)
        if name == "capability":
            for target in wire["execution"]["backend_signals"]["targets"]: target["signal_capability"] = False
        result.append(wire)
    return result


def compare_all(cases: list) -> list:
    """No missing or duplicated response can disappear from this maintained test gate."""
    submitted = [request(case) for case in cases]
    responses = index_responses(submitted, execute(submitted, timeout=180))
    result = [compare(case, wire, responses[wire["id"]]) for case, wire in zip(cases, submitted)]
    rejected = refusals(next(case for case in cases if case["signal_requested"] is not None))
    responses = index_responses(rejected, execute(rejected))
    for wire in rejected:
        response = responses[wire["id"]]
        passed = response.get("status") == "error" and response.get("code", "").startswith("unsupported.backend_")
        result.append({"id": wire["id"], "status": "passed" if passed else "failed", "response": response,
                       "claim": "Coverage refusal, not a PostgreSQL semantic comparison."})
    return result

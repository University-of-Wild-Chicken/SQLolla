"""Independent observation projection for controlled notification/advisory scalar statements."""
from corpus_semantics import catalog_unchanged, observation_matches, schema_matches
from corpus_wire import cell
from oracle_contract import index_responses, top_level_ordered
from rejections import execute


def request(case: dict) -> dict:
    """No hidden database access: the isolated transaction begins with complete empty ownership."""
    parameters = {str(index + 1): cell(value, "text") for index, value in
                  enumerate(value for pair in case.get("calls", []) for value in pair)}
    return {"id": case["id"], "sql": case["sql"], "catalog": {"search_path": ["public"], "tables": {}},
            "database": {}, "parameters": parameters, "mode": "compare", "execution": {
                "max_rows": 256, "max_outcomes": 256, "settings": {}, "table_metadata": {},
                "session_effects": {"complete": True, "transaction": "subject", "held_advisory": []}}}


def delivery(events: list) -> list:
    """Project pending invocations onto the separately specified first-occurrence commit delivery."""
    result = []
    for event in events:
        if event.get("kind") != "notification": raise ValueError("Unexpected non-notification effect")
        value = {key: event[key] for key in ("channel", "payload")}
        if value not in result: result.append(value)
    return result


def actual_delivery(events: list) -> list:
    """PG backend IDs vary; exact channel/payload order is the observed notification identity."""
    return [{key: event[key] for key in ("channel", "payload")} for event in events]


def advisory_ownership(events: list) -> list:
    """Encode signed typed keys using PostgreSQL's observed pg_locks namespace representation."""
    result = []
    for event in events:
        if event.get("kind") != "advisory_lock": raise ValueError("Unexpected non-advisory effect")
        held, key = event["held"], event["held"]["key"]
        if held["owner"] != "subject": raise ValueError("Advisory owner escaped the supplied transaction")
        if key["kind"] == "int64": first, second, namespace = (key["key"] >> 32) & 0xffffffff, key["key"] & 0xffffffff, 1
        elif key["kind"] == "int32_pair": first, second, namespace = key["first"] & 0xffffffff, key["second"] & 0xffffffff, 2
        else: raise ValueError("Unknown advisory key namespace")
        result.append([str(first), str(second), str(namespace), "ShareLock" if held["shared"] else "ExclusiveLock", "true"])
    return sorted(result)


def effects_match(case: dict, outcome: dict) -> bool:
    """Check transactional rollback, exact ownership, and independently observed commit delivery."""
    events = outcome["outcome"]["effects"]
    if case["postgres"]["status"] == "error": return not events and not case["after_end"]
    if case["id"].startswith("notify/"):
        if case["before_end"]: return False
        if not case["commit"] and case["after_end"]: return False
        return delivery(events) == actual_delivery(case["committed_delivery"])
    return not case["before"] and not case["after_end"] and advisory_ownership(events) == sorted(case["after"])


def compare(case: dict, submitted: dict, response: dict) -> dict:
    """Require every returned alternative, not only one witness, to match this deterministic fixture."""
    errors, expected = [], case["postgres"]
    if response.get("status") != "ok" or response.get("validation") != "ALLOY_TABLE_ROUNDTRIP" or not response.get("complete"):
        errors.append("incomplete_execution")
    else:
        outcomes = response["outcomes"]
        if not outcomes: errors.append("empty_outcome_set")
        for outcome in outcomes:
            if not observation_matches(outcome, expected, {"database": {}}, submitted["catalog"], {}, top_level_ordered(submitted["sql"])):
                errors.append("postgres_value_error_or_state")
            if outcome["status"] == "succeeded" and not catalog_unchanged(outcome["outcome"].get("catalog"), submitted["catalog"]):
                errors.append("catalog_frame")
            try:
                if not effects_match(case, outcome): errors.append("observed_effects")
            except (ValueError, KeyError): errors.append("effect_shape")
        if expected["status"] == "ok" and not schema_matches(response["schema"], expected["columns"], {}): errors.append("schema")
    return {"id": case["id"], "status": "failed" if errors else "passed", "errors": sorted(set(errors)),
            "request": submitted, "response": response}


def refusals() -> list[dict]:
    """Known conflicting ownership refuses to guess a wait schedule or timeout outcome."""
    blocked = request({"id": "session/refusal/conflict", "sql": "SELECT pg_advisory_xact_lock(7)"})
    blocked["execution"]["session_effects"]["held_advisory"] = [{"key": {"kind": "int64", "key": 7}, "owner": "other", "shared": False}]
    incomplete = request({"id": "session/refusal/incomplete", "sql": "SELECT pg_advisory_xact_lock(7)"})
    incomplete["execution"]["session_effects"]["complete"] = False
    return [blocked, incomplete]


def compare_all(observed: dict) -> list:
    """Index exact IDs so missing, duplicate or unexpected responses cannot count as passing tests."""
    cases = observed["notifications"] + observed["advisory"]
    submitted = [request(case) for case in cases]
    responses = index_responses(submitted, execute(submitted, timeout=180))
    result = [compare(case, wire, responses[wire["id"]]) for case, wire in zip(cases, submitted)]
    rejected = refusals()
    responses = index_responses(rejected, execute(rejected))
    for wire in rejected:
        response = responses[wire["id"]]
        passed = response.get("status") == "error" and response.get("code") in ("unsupported.advisory_wait", "unsupported.session_effect_context")
        result.append({"id": wire["id"], "status": "passed" if passed else "failed", "response": response,
                       "claim": "Explicit admission refusal, not a PostgreSQL semantic match."})
    return result

"""Compare independently observed effect families without confusing ownership sets and event traces."""
import json
import corpus_sequences
import corpus_session_effects
import corpus_backend_effects
import corpus_tuples

LOCK_KINDS = {"table_lock", "row_lock"}


def selected(outcome: dict, kinds: set[str]) -> list:
    """Keep complete observations of the requested family, without deduplicating ordered events."""
    return [event for event in outcome["outcome"]["effects"] if event["kind"] in kinds]


def ownership_key(effects: list) -> list:
    """Ownership sets are compared canonically; sequence and notification event order is separate."""
    return sorted(json.dumps(value, sort_keys=True) for value in effects)


def compare(evidence: dict, before: dict, after: dict, matches: list) -> tuple[list, list, str | None]:
    """Filter complete outcomes by actual ownership and sequence poststate; retain unobserved families."""
    pending = list(evidence["unobserved_effects"])
    locks = evidence.get("row_locks")
    if locks is not None:
        matches = [value for value in matches if ownership_key(selected(value, LOCK_KINDS)) == ownership_key(locks["effects"])]
        if not matches: return [], pending, "row_lock_effects"
    elif any(selected(value, LOCK_KINDS) for value in matches): pending.append("row_lock_observation")
    if corpus_sequences.context(before) is not None and corpus_sequences.context(after) is not None:
        matches = [value for value in matches if corpus_sequences.matches(value["outcome"]["effects"], before, after)]
        if not matches: return [], pending, "sequence_state"
    elif evidence["sequence_state_changed"] or any(selected(value, {"sequence_value"}) for value in matches):
        pending.append("sequence_state_observation")
    matches, pending, reason = sessions(evidence, before, after, matches, pending)
    if reason is not None: return [], pending, reason
    matches, pending, reason = backends(evidence, matches, pending)
    if reason is not None: return [], pending, reason
    matches, pending, reason = tuples(evidence, matches, pending)
    if reason is not None: return [], pending, reason
    other = {event["kind"] for value in matches for event in value["outcome"]["effects"]} - LOCK_KINDS - {"sequence_value", "advisory_lock", "notification", "tuple_version"}
    pending.extend("uncompared_effect:" + kind for kind in sorted(other))
    return matches, sorted(set(pending)), None


def sessions(evidence: dict, before: dict, after: dict, matches: list, pending: list) -> tuple:
    """Discharge only the independently observed advisory and notification families."""
    observed = evidence.get("session_effects")
    if observed is not None:
        matches = [value for value in matches if corpus_session_effects.matches(value["outcome"]["effects"], observed)]
        if not matches: return [], pending, "session_effect_observations"
        admitted = {"pg_advisory_xact_lock"}
        if observed.get("notification_companion") is not None: admitted.add("pg_notify")
        pending = [value for value in pending if value not in admitted]
    elif after["advisory_locks"] != before["advisory_locks"] or any(selected(value, {"advisory_lock", "notification"}) for value in matches):
        pending.append("session_effect_observation")
    return matches, pending, None


def backends(evidence: dict, matches: list, pending: list) -> tuple:
    """A structural zero-input proof discharges only actual absent invocations of the source projection."""
    observed = evidence.get("backend_effects")
    if observed is not None:
        matches = [value for value in matches if corpus_backend_effects.matches(value["outcome"]["effects"], observed)]
        if not matches: return [], pending, "backend_effect_observations"
        pending = [value for value in pending if value != "pg_terminate_backend"]
    return matches, pending, None


def tuples(evidence: dict, matches: list, pending: list) -> tuple:
    """Validate tuple row/header lineage and actual raw header residues independently of SQL values."""
    observed = evidence.get("tuple_observations")
    if observed is not None:
        matches = [value for value in matches if corpus_tuples.matches(value["outcome"]["effects"], observed)]
        if not matches: return [], pending, "tuple_header_observations"
    elif any(selected(value, {"tuple_version"}) for value in matches):
        pending.append("tuple_header_observation")
    return matches, pending, None

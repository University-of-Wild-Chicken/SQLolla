"""Prove the industrial backend call is not invoked from complete inputs and exact source topology."""
import json

from backend_context import capture, checked
from corpus_notify_context import parse
from run import digest


def column(name: str) -> dict:
    """The admitted source profile uses one unqualified column of the sole system relation."""
    return {"tag": "column", "name": name, "qualifier": None, "schema": None}


def topology(ast: dict) -> int:
    """Reject discarded/eager calls, modifiers and hidden evaluations rather than relying on empty output."""
    expected = {"tag": "query", "ctes": [], "recursive": False, "limit": None, "offset": None,
                "offset_first": False, "locking": [], "order_by": []}
    if {key: value for key, value in ast.items() if key != "body"} != expected:
        raise ValueError("Backend no-invocation proof requires the exact direct query topology")
    body = ast["body"]
    projection = {"tag": "expression", "alias": None, "value": {"tag": "call", "name": ["pg_terminate_backend"], "args": [column("pid")]}}
    source = {"tag": "table", "name": "pg_stat_activity", "schema": None, "alias": None, "columns": []}
    if {key: value for key, value in body.items() if key != "where"} != {
        "tag": "select", "distinct": False, "distinct_on": [], "group_by": [], "having": None, "items": [projection], "source": source}:
        raise ValueError("Backend projection/source is outside the no-invocation contract")
    where = body["where"]
    parameter = where.get("left", {}).get("right", {})
    if parameter.get("tag") != "numbered_parameter" or set(parameter) != {"tag", "index"}:
        raise ValueError("Backend filter parameter is not explicit")
    expected_filter = {"tag": "binary", "op": "and", "left": {"tag": "binary", "op": "eq", "left": column("datname"), "right": parameter},
        "right": {"tag": "binary", "op": "ne", "left": column("pid"), "right": {"tag": "call", "name": ["pg_backend_pid"], "args": []}}}
    if where != expected_filter: raise ValueError("Backend filter has additional evaluation or a different predicate")
    return int(parameter["index"])


def projected(system: dict, stage: str) -> dict:
    """The observed query-visible system relation contains every row of the declared pid/datname projection."""
    relation = system["relations_" + stage]["pg_catalog.pg_stat_activity"]
    if [(column["name"], column["type"]) for column in relation["columns"]] != [("pid", "integer"), ("datname", "name")]:
        raise ValueError("Backend projection schema differs")
    if any(len(row) != 2 for row in relation["rows"]): raise ValueError("Backend row width differs")
    return {"columns": ["pid", "datname"], "rows": sorted(relation["rows"], key=str)}


def prepare(connection, case: dict, context: dict, system: dict | None) -> dict | None:
    """Derive selected input cardinality independently before executing the source statement."""
    if "pg_terminate_backend" not in case["inventory"]["effect_obligations"]: return None
    if system is None: raise ValueError("Backend source requires complete observed system input")
    from corpus_runtime import parameters
    ast, provenance = parse(context["sql"])
    index, before = topology(ast), projected(system, "before")
    backend = capture(connection)
    identity = checked(connection, "SELECT 'pg_stat_activity'::regclass::oid='pg_catalog.pg_stat_activity'::regclass::oid")["rows"][0][0]
    if identity != "t": raise ValueError("Source system relation is shadowed")
    canonical = connection.query("SELECT $" + str(index) + "::name::text", parameters(connection, context))
    if canonical["status"] != "ok": raise ValueError(canonical)
    name = canonical["rows"][0][0]
    selected = [int(pid) for pid, datname in before["rows"] if name is not None and datname == name and int(pid) != backend["actor_pid"]]
    if set(int(row[0]) for row in before["rows"]) != {target["pid"] for target in backend["targets"]}:
        raise ValueError("Backend snapshot changed during context capture")
    return dict(provenance, initial=backend, before=before, selected_input_pids=selected,
                canonical_database_parameter=name, parameter_index=index,
                before_sha256=digest(json.dumps(before, sort_keys=True).encode()),
                status="no_invocation_pending_poststate" if not selected else "positive_invocation_requires_observer",
                scope="Exact direct source topology over every role-visible pid/datname row. No invocation follows from zero selected inputs, not from zero returned rows.")


def finish(evidence: dict | None, system: dict | None, result: dict) -> None:
    """Source execution and observed backend poststate must agree with the structural zero-input proof."""
    if evidence is None or evidence["selected_input_pids"]: return
    after = projected(system, "after")
    evidence["after"] = after
    evidence["unchanged_backends"] = after == evidence["before"]
    expected_result = (result["status"] == "ok" and result["command"] == "SELECT 0"
                       and result["affected_rows"] == 0 and result["rows"] == [])
    evidence["status"] = "no_invocation_proved" if evidence["unchanged_backends"] and expected_result else "observation_mismatch"


def execution_context(evidence: dict | None) -> dict | None:
    """Only the independently captured initial inventory enters the pure execution request."""
    return None if evidence is None else evidence["initial"]


def matches(events: list, evidence: dict | None) -> bool:
    """The strict zero-input proof discharges only a zero-call case, never positive termination semantics."""
    relevant = [event for event in events if event["kind"] in ("backend_termination_requested", "sql_warning")]
    return evidence is not None and evidence.get("status") == "no_invocation_proved" and not relevant

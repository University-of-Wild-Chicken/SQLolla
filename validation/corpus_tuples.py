"""Explicit single-writer tuple-header context and independent observed-header comparison."""
from collections import Counter, defaultdict, deque
from copy import deepcopy

from oracle import quote_identifier


def capture(connection, tables: dict, write_xid: int, own_locked: tuple = ()) -> dict:
    """Capture before the analyzed statement in a top-level transaction, never from its RETURNING."""
    from corpus_runtime import checked, describe
    if not 3 <= write_xid < 2**32: raise ValueError("Tuple profile excludes reserved/wrapped writer identity")
    relations, headers = {}, {}
    for identity, table in tables.items():
        name = quote_identifier(table["schema"]) + "." + quote_identifier(table["name"])
        relations[identity] = describe(connection, checked(connection, "SELECT * FROM " + name + " ORDER BY ctid"))
        raw = checked(connection, "SELECT xmin::text,xmax::text FROM " + name + " ORDER BY ctid")["rows"]
        headers[identity] = [header(row, write_xid, identity in own_locked) for row in raw]
        if len(raw) != len(relations[identity]["rows"]): raise ValueError("Snapshot changed during capture")
    context = {"complete": True, "exclusive_snapshot": True, "top_level_no_savepoints": True,
               "write_xid": write_xid, "tables": headers}
    return {"context": context, "relations": relations,
            "provenance": "Owned isolated PG cluster; BEGIN without SAVEPOINT; writer xid and ctid-aligned headers captured before the analyzed statement."}


def header(row: list, write_xid: int, own_locked: bool) -> dict:
    """Only fixture-established pristine/own-lock states are admitted; raw bits do not prove flags."""
    xmin, xmax = map(int, row)
    if xmax == 0: state = "invalid"
    elif xmax == write_xid and own_locked: state = "own_lock"
    else: raise ValueError("Unattested raw xmax/MultiXact state is outside this capture profile")
    return {"xmin": xmin, "raw_xmax": xmax, "state": state}


def values(row: list) -> tuple:
    """Focused integer-row witnesses compare SQL values without relying on output occurrence order."""
    return tuple(None if value["type"] == "null" else str(value["value"]) for value in row)


def header_bag(snapshot: dict) -> Counter:
    """Physical observation equality preserves multiplicity of rows carrying identical header values."""
    return Counter((tuple(row), head["xmin"], head["raw_xmax"]) for row, head in zip(snapshot["rows"], snapshot["headers"]))


def replay(before: dict, events: list, rollback: bool = False) -> Counter:
    """Independently replay stable slots and validate their original/current row/header witnesses."""
    initial = {index: (tuple(row), head) for index, (row, head) in enumerate(zip(before["rows"], before["headers"]))}
    current = dict(initial)
    for event in events:
        if event["kind"] != "tuple_version": raise ValueError("Unexpected effect in focused tuple oracle")
        observation = event["observation"]
        step = observation["transition"]
        index = step["occurrence"]
        original = initial.get(index)
        prior = current.get(index)
        if (None if original is None else original[0]) != (None if observation["original_row"] is None else values(observation["original_row"])): raise ValueError("Original row lineage mismatch")
        if not rollback and (None if prior is None else prior[0]) != (None if observation["before_row"] is None else values(observation["before_row"])): raise ValueError("Predecessor row mismatch")
        if not rollback and prior and (prior[1]["xmin"], prior[1]["raw_xmax"]) != (step["before"]["xmin"], step["before"]["raw_xmax"]): raise ValueError("Header chain mismatch")
        if rollback:
            if original is not None: current[index] = (original[0], step["rollback"])
            else: current.pop(index, None)
        else: current[index] = (values(observation["after_row"]), step["after"])
    return Counter((row, head["xmin"], head["raw_xmax"]) for row, head in current.values())


def align(captured: dict, before: dict) -> dict:
    """Reindex ctid-aligned headers to the recorded bag order with stable duplicate occurrences."""
    aligned = deepcopy(captured)
    for identity, relation in captured["relations"].items():
        key = identity if identity in before else identity.rsplit(".", 1)[-1]
        recorded = before[key]["rows"]
        positions = defaultdict(deque)
        for index, row in enumerate(relation["rows"]): positions[tuple(row)].append(index)
        order = []
        for row in recorded:
            if not positions[tuple(row)]: raise ValueError("Tuple snapshot row not present in recorded before state")
            order.append(positions[tuple(row)].popleft())
        if any(positions.values()): raise ValueError("Tuple snapshot has unmatched before occurrences")
        aligned["context"]["tables"][identity] = [captured["context"]["tables"][identity][index] for index in order]
        aligned["relations"][identity]["rows"] = deepcopy(recorded)
    return aligned


def observe(connection, tables: dict) -> dict:
    """Observe raw poststate headers without pretending their raw bits attest transaction flags."""
    from corpus_runtime import checked, describe
    relations = {}
    for identity, table in tables.items():
        name = quote_identifier(table["schema"]) + "." + quote_identifier(table["name"])
        rows = describe(connection, checked(connection, "SELECT * FROM " + name + " ORDER BY ctid"))
        headers = checked(connection, "SELECT xmin::text,xmax::text FROM " + name + " ORDER BY ctid")["rows"]
        relations[identity] = {"relation": rows, "headers": [{"xmin": int(a), "raw_xmax": int(b)} for a, b in headers]}
    return relations


def semantic_row(row: list) -> tuple:
    """Use the independent differential harness's typed SQL-family observation equality."""
    from corpus_semantics import normalized
    return tuple(normalized(cell) for cell in row)


def typed_observation(relation: dict, table: dict) -> list:
    """Declared authentic column types resolve enum/array identities absent from plain type names."""
    from corpus_wire import typed_rows
    return typed_rows(relation["rows"], [column["ty"] for column in table["columns"]])


def initial_slots(captured: dict, tables: dict) -> dict:
    """Stable input occurrences bind both typed user values and their independently observed headers."""
    result = {}
    for identity, relation in captured["relations"].items():
        rows = typed_observation(relation, tables[identity])
        for index, (row, head) in enumerate(zip(rows, captured["context"]["tables"][identity])):
            result[(identity, index)] = (semantic_row(row), head)
    return result


def replay_slots(events: list, initial: dict, rollback: bool) -> dict:
    """Reject invented identities or broken row/header lineage before comparing the physical bag."""
    current = dict(initial)
    for event in events:
        if event["kind"] != "tuple_version": continue
        value, step = event["observation"], event["observation"]["transition"]
        key = (step["table"], step["occurrence"])
        original, previous = initial.get(key), current.get(key)
        if (None if original is None else original[0]) != (None if value["original_row"] is None else semantic_row(value["original_row"])): raise ValueError("Original tuple row mismatch")
        if (None if original is None else original[1]) != step["original"]: raise ValueError("Original header mismatch")
        if not rollback and (None if previous is None else previous[0]) != (None if value["before_row"] is None else semantic_row(value["before_row"])): raise ValueError("Prior tuple row mismatch")
        if not rollback and previous and previous[1] != step["before"]: raise ValueError("Prior tuple header mismatch")
        if rollback:
            if original is None: current.pop(key, None)
            else: current[key] = (original[0], step["rollback"])
        else: current[key] = (semantic_row(value["after_row"]), step["after"])
    return current


def observed_bag(observation: dict, tables: dict) -> Counter:
    """Physical row/header equality preserves multiplicities and is independent of ctid ordering."""
    result = Counter()
    for identity, value in observation.items():
        rows = typed_observation(value["relation"], tables[identity])
        if len(rows) != len(value["headers"]): raise ValueError("Poststate header count mismatch")
        for row, head in zip(rows, value["headers"]): result[(identity, semantic_row(row), head["xmin"], head["raw_xmax"])] += 1
    return result


def slots_bag(slots: dict) -> Counter:
    """Canonicalization discards only physical slot ordering, preserving row/header association."""
    return Counter((table, row, head["xmin"], head["raw_xmax"]) for (table, _), (row, head) in slots.items())


def matches(events: list, evidence: dict) -> bool:
    """Compare complete raw header poststate and rollback residue against actual independent PG reads."""
    try:
        tables = evidence["catalog_tables"]
        initial = initial_slots(evidence["before"], tables)
        after = replay_slots(events, initial, False)
        rolled_back = replay_slots(events, initial, True)
        return (slots_bag(after) == observed_bag(evidence["after"], tables)
                and slots_bag(rolled_back) == observed_bag(evidence["after_rollback"], tables))
    except (ValueError, KeyError, TypeError): return False


def requested_tables(sql: str, catalog_tables: dict) -> dict:
    """Resolve INSERT targets from the pinned parsed AST only when RETURNING names system columns."""
    import re
    if not re.search(r"\b(?:xmin|xmax)\b", sql, re.IGNORECASE): return {}
    from corpus_notify_context import parse
    ast, _ = parse(sql)
    result = {}
    for node in nodes(ast):
        if node.get("tag") != "insert" or not any(value.get("tag") == "column" and value.get("name") in {"xmin", "xmax"} for value in nodes(node.get("returning", []))): continue
        candidates = [(key, table) for key, table in catalog_tables.items() if table["name"] == node["table"]]
        if len(candidates) != 1: raise ValueError("System RETURNING target lacks unambiguous catalog identity")
        key, table = candidates[0]
        result[key] = table
    return result


def nodes(value: object):
    """Walk typed syntax structures without parsing identifiers or SQL expressions independently."""
    if isinstance(value, dict):
        yield value
        for child in value.values(): yield from nodes(child)
    elif isinstance(value, list):
        for child in value: yield from nodes(child)

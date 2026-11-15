"""Capture genuine sequence metadata and compare retained nontransactional state."""
from oracle import quote_identifier


def checked(connection, sql: str) -> dict:
    """Require an actual PostgreSQL catalog observation rather than synthesized metadata."""
    result = connection.query(sql)
    if result["status"] != "ok": raise ValueError(result)
    return result


def capture(connection, records: dict) -> dict:
    """Bind bounds, OIDs and privilege evidence to the same session as state observation."""
    sql = """SELECT n.nspname,c.relname,c.oid::text,s.seqincrement::text,s.seqmin::text,
      s.seqmax::text,s.seqcycle::text,s.seqcache::text,
      (has_sequence_privilege(c.oid,'USAGE') OR has_sequence_privilege(c.oid,'UPDATE'))::text
      FROM pg_sequence s JOIN pg_class c ON c.oid=s.seqrelid
      JOIN pg_namespace n ON n.oid=c.relnamespace ORDER BY n.nspname,c.relname"""
    result = {"complete": True, "exclusive_snapshot": True, "sequences": {}}
    for row in checked(connection, sql)["rows"]:
        schema, name, oid, increment, minimum, maximum, cycle, cache, permitted = row
        state = observed_state(connection, records, schema, name)
        identity = schema + "." + name
        if identity in result["sequences"]: raise ValueError("Ambiguous sequence identity")
        result["sequences"][identity] = {"oid": int(oid), "schema": schema, "name": name,
            "state": state, "increment": int(increment), "minimum": int(minimum),
            "maximum": int(maximum), "cycle": cycle == "true", "cache": int(cache),
            "can_use": permitted == "true"}
    return result


def observed_state(connection, records: dict, schema: str, name: str) -> dict:
    """Reuse exact public snapshots and directly inspect any other declared sequence."""
    if schema == "public" and name in records:
        record = records[name]
        values = dict(zip([column["name"] for column in record["columns"]], record["rows"][0]))
        return {"last_value": int(values["last_value"]), "is_called": values["is_called"] == "t"}
    sql = "SELECT last_value::text,is_called::text FROM " + quote_identifier(schema) + "." + quote_identifier(name)
    value, called = checked(connection, sql)["rows"][0]
    return {"last_value": int(value), "is_called": called == "true"}


def context(before: dict) -> dict | None:
    """Only same-run catalog evidence can supply an executable sequence context."""
    return before.get("sequence_context")


def relation_lookup(connection) -> dict:
    """Capture names and privileges in the allocation session, never reconstruct OIDs from SQL text."""
    sql = """SELECT json_build_object('complete',true,'current_database',current_database(),
      'search_path',current_schemas(true),
      'schema_usage',(SELECT json_object_agg(nspname,has_schema_privilege(oid,'USAGE')) FROM pg_namespace),
      'relations',(SELECT json_agg(json_build_object('schema',n.nspname,'name',c.relname,'oid',c.oid) ORDER BY c.oid)
        FROM pg_class c JOIN pg_namespace n ON n.oid=c.relnamespace))::text"""
    import json
    return typed_lookup(json.loads(checked(connection, sql)["rows"][0][0]))


def typed_lookup(observed: dict) -> dict:
    """PostgreSQL JSON renders oid as text; transport preserves identity as a checked unsigned integer."""
    relations = []
    for relation in observed["relations"]:
        oid = int(relation["oid"])
        if not 0 < oid <= 4294967295: raise ValueError("Invalid observed relation OID")
        relations.append(dict(relation, oid=oid))
    return dict(observed, relations=relations)


def final_states(events: list, initial: dict) -> dict:
    """Apply recorded allocations to their named sequence state without inventing unseen events."""
    states = {name: dict(value["state"]) for name, value in initial["sequences"].items()}
    for event in events:
        if event["kind"] != "sequence_value": continue
        if event["sequence"] not in states: raise ValueError("Unknown sequence event identity")
        states[event["sequence"]] = {"last_value": event["value"], "is_called": True}
    return states


def matches(events: list, before: dict, after: dict) -> bool:
    """Compare every sequence's final state; this does not independently observe each internal call."""
    initial, final = context(before), context(after)
    if initial is None or final is None: return False
    expected = {name: value["state"] for name, value in final["sequences"].items()}
    return final_states(events, initial) == expected

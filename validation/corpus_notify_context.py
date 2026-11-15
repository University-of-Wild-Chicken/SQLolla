"""Admit closed notification-channel expressions from the unchanged parser AST for LISTEN coverage."""
import json
import os
from pathlib import Path

from oracle import literal
from pg_runtime import ROOT
from process_helpers import communicate
from run import digest

PARSER = ROOT / "parser/.lake/build/bin" / ("sqlolla_parser.exe" if os.name == "nt" else "sqlolla_parser")
CLOCKS = {"now", "transaction_timestamp", "statement_timestamp", "clock_timestamp", "timeofday",
          "current_timestamp", "localtimestamp", "current_date", "current_time", "localtime"}


def nodes(value):
    """Traverse structured nodes only; SQL comments and string contents never become calls."""
    if isinstance(value, dict):
        yield value
        for child in value.values(): yield from nodes(child)
    elif isinstance(value, list):
        for child in value: yield from nodes(child)


def parse(sql: str) -> tuple[dict, dict]:
    """Bind the syntax evidence to exact SQL bytes and the invoked parser binary."""
    envelope = json.loads(communicate([str(PARSER)], sql, timeout=15, cwd=ROOT))
    if (envelope.get("protocol") not in ("sqlolla.parser.v1", "sqlolla.parser.v2")
        or envelope.get("parser_commit") != "ad15c585155ff7fc2c5fc4c9746905e4959ff5eb"
        or envelope.get("assurance") != "syntax-only" or envelope.get("semantic_preservation") is not False):
        raise ValueError("Notification source parser identity mismatch")
    result = envelope["result"]
    if result.get("parse_status") != "parsed" or result.get("export_status") != "exported":
        raise ValueError("Notification channel syntax is not completely exported")
    return result["ast"], {"sql_sha256": digest(sql.encode()), "parser_sha256": digest(PARSER.read_bytes()),
                           "ast_sha256": digest(json.dumps(result["ast"], sort_keys=True).encode())}


def known_nonnull(node: dict, context: dict) -> bool:
    """Only literal or explicitly supplied non-NULL parameters can prove a dead coalesce tail."""
    if node.get("tag") == "cast":
        target = node["type"].get("name", {})
        if target.get("tag") != "scalar_type" or target.get("name") not in {"text", "timestamp", "timestamptz", "int", "integer", "bigint", "boolean"}:
            return False
        return known_nonnull(node["arg"], context)
    if node.get("tag") == "numbered_parameter": return context["parameters"][int(node["index"]) - 1]["value"] is not None
    return node.get("tag") in ("text", "integer", "boolean", "decimal")


def require_clock_independent(node, context: dict) -> None:
    """A changed companion transaction clock is admissible only when no live source expression reads it."""
    if isinstance(node, list):
        for child in node: require_clock_independent(child, context)
    elif isinstance(node, dict):
        name = node.get("name", [])
        if node.get("tag") == "call" and name and name[-1].lower() in CLOCKS:
            raise ValueError("Commit companion would read a different transaction clock")
        if node.get("tag") == "context_value" and node.get("value", "").lower() in CLOCKS:
            raise ValueError("Commit companion would read a different context clock")
        if node.get("tag") == "coalesce":
            for child in node["args"]:
                require_clock_independent(child, context)
                if known_nonnull(child, context): break
        else:
            for child in node.values(): require_clock_independent(child, context)


def resolve_column(node: dict, ast: dict) -> dict:
    """Resolve one explicitly qualified CTE projection only when its value is row-independent."""
    if node.get("schema") is not None or node.get("qualifier") is None:
        raise ValueError("Channel column requires an unambiguous CTE qualifier")
    definitions = [item for item in nodes(ast) if item.get("name") == node["qualifier"] and "query" in item]
    if len(definitions) != 1 or definitions[0].get("columns"):
        raise ValueError("Ambiguous notification channel CTE")
    query = definitions[0]["query"]
    body = query.get("body", {})
    if query.get("recursive") or body.get("tag") != "select" or body.get("source") is not None:
        raise ValueError("Channel CTE must project a closed expression")
    projections = [item["value"] for item in body["items"] if item.get("alias") == node["name"] and item.get("tag") == "expression"]
    if len(projections) != 1: raise ValueError("Channel alias is not unique")
    return projections[0]


def channel_sql(node: dict, ast: dict, depth: int = 0) -> str:
    """Re-render only a closed text/parameter/concat/coalesce fragment for independent PG evaluation."""
    if depth > 32: raise ValueError("Notification channel dependency depth exceeded")
    tag = node.get("tag")
    child = lambda value: channel_sql(value, ast, depth + 1)
    if tag == "text": return literal(node["value"])
    if tag == "sql_null": return "NULL::text"
    if tag == "numbered_parameter": return "$" + str(int(node["index"]))
    if tag == "column": return child(resolve_column(node, ast))
    if tag == "cast" and node["type"] == {"modifiers": [], "name": {"name": "text", "tag": "scalar_type"}}:
        return "(" + child(node["arg"]) + ")::text"
    if tag == "coalesce": return "coalesce(" + ",".join(map(child, node["args"])) + ")"
    if tag == "call" and node["name"] in (["concat"], ["pg_catalog", "concat"]):
        return "pg_catalog.concat(" + ",".join(map(child, node["args"])) + ")"
    if tag == "call" and node["name"] in (["current_schema"], ["pg_catalog", "current_schema"]) and not node["args"]:
        return "pg_catalog.current_schema()"
    raise ValueError("Notification channel expression is outside the observed closed profile")


def channel_contract(connection, context: dict, typed: list) -> dict:
    """Every notification site supplies a verified channel expression, including CTE alias references."""
    ast, provenance = parse(context["sql"])
    require_clock_independent(ast, context)
    calls = [node for node in nodes(ast) if node.get("tag") == "call" and node.get("name", [])[-1:] == ["pg_notify"]]
    if not calls or any(node["name"] not in (["pg_notify"], ["pg_catalog", "pg_notify"]) for node in calls):
        raise ValueError("Unknown notification call identity")
    expressions = [channel_sql(node["args"][0], ast) for node in calls]
    channels = []
    for expression in expressions:
        result = connection.query("SELECT " + expression, typed)
        if result["status"] != "ok" or len(result["rows"]) != 1: raise ValueError(result)
        channel = result["rows"][0][0]
        if channel and len(channel.encode()) < 64 and channel not in channels: channels.append(channel)
    return dict(provenance, expressions=expressions, channels=channels,
                scope="All source pg_notify call sites; channel dependencies restricted to declared parameters, literals, current_schema and closed CTE aliases.")

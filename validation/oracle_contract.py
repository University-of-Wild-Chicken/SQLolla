"""Independent source-order and complete-response obligations for validation harnesses."""
import re


def skip_quote(sql: str, start: int) -> int:
    """Skip quoted SQL content, including doubled delimiters and escaped string bytes."""
    quote, position = sql[start], start + 1
    escaped = start > 0 and sql[start - 1] in "eE" and (start < 2 or not (sql[start - 2].isalnum() or sql[start - 2] == "_"))
    while position < len(sql):
        if sql[position] == quote:
            if sql[position:position + 2] == quote * 2: position += 2
            else: return position + 1
        elif sql[position] == "\\" and quote == "'" and escaped: position += 2
        else: position += 1
    raise ValueError("Unclosed quoted SQL in oracle contract")


def skip_comment(sql: str, start: int) -> int:
    """Honor nested PostgreSQL block comments instead of interpreting their SQL words."""
    depth, position = 1, start + 2
    while position < len(sql):
        pair = sql[position:position + 2]
        if pair == "/*": depth, position = depth + 1, position + 2
        elif pair == "*/":
            depth, position = depth - 1, position + 2
            if depth == 0: return position
        else: position += 1
    raise ValueError("Unclosed SQL block comment in oracle contract")


def ignored_end(sql: str, position: int) -> int:
    """Exclude strings, identifiers, comments and dollar strings from the keyword stream."""
    if sql[position] in "'\"": return skip_quote(sql, position)
    if sql[position:position + 2] == "/*": return skip_comment(sql, position)
    if sql[position:position + 2] == "--":
        end = sql.find("\n", position)
        return len(sql) if end < 0 else end + 1
    delimiter = re.match(r"\$(?:[A-Za-z_][A-Za-z_0-9]*)?\$", sql[position:])
    if delimiter:
        tag = delimiter[0]
        end = sql.find(tag, position + len(tag))
        if end < 0: raise ValueError("Unclosed SQL dollar string")
        return end + len(tag)
    return position


def source_tokens(sql: str) -> list[tuple[int, str]]:
    """Retain only unquoted keywords and their lexical parenthesis depth."""
    position, depth, tokens = 0, 0, []
    while position < len(sql):
        end = ignored_end(sql, position)
        if end != position: position = end; continue
        character = sql[position]
        if character == "(": depth += 1
        elif character == ")":
            depth -= 1
            if depth < 0: raise ValueError("Unbalanced SQL parentheses")
        elif character.isalpha() or character == "_":
            end = position + 1
            while end < len(sql) and (sql[end].isalnum() or sql[end] in "_$"): end += 1
            tokens.append((depth, sql[position:end].upper()))
            position = end
            continue
        position += 1
    if depth: raise ValueError("Unbalanced SQL parentheses")
    return tokens


def top_level_ordered(sql: str) -> bool:
    """Derive the outer ORDER BY contract from source, never from an engine result flag."""
    tokens = source_tokens(sql)
    if not tokens: return False
    minimum = min(depth for depth, _ in tokens)
    outer = [token for depth, token in tokens if depth == minimum]
    if outer[0] not in ("SELECT", "VALUES", "WITH", "UNION", "INTERSECT", "EXCEPT"): return False
    statement = next((token for token in outer if token in ("SELECT", "VALUES", "INSERT", "UPDATE", "DELETE", "MERGE")), "SELECT")
    if statement not in ("SELECT", "VALUES"): return False
    return any(left == "ORDER" and right == "BY" for left, right in zip(outer, outer[1:]))


def index_responses(requests: list, responses: list) -> dict:
    """Require exactly one response for every unique submitted correlation identity."""
    wanted = [request["id"] for request in requests]
    actual = [response.get("id") for response in responses]
    if len(set(wanted)) != len(wanted): raise ValueError("Duplicate request identity")
    if len(actual) != len(wanted) or len(set(actual)) != len(actual) or set(actual) != set(wanted):
        raise ValueError("Incomplete, duplicate or unexpected response identities")
    return {response["id"]: response for response in responses}

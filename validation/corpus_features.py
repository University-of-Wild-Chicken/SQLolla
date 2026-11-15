"""Conservative lexical feature inventory; these tags are not parser proofs."""
import re

PATTERNS = {
    "cte": r"\bWITH\b", "ordering": r"\bORDER\s+BY\b", "pagination": r"\b(LIMIT|OFFSET|FETCH)\b",
    "grouping": r"\bGROUP\s+BY\b", "having": r"\bHAVING\b", "aggregate": r"\b(COUNT|SUM|AVG|MIN|MAX|ARRAY_AGG|JSON_AGG)\s*\(",
    "locking": r"\bFOR\s+(UPDATE|SHARE|NO\s+KEY\s+UPDATE|KEY\s+SHARE)\b",
    "lock_skipping": r"\b(SKIP\s+LOCKED|NOWAIT)\b", "case": r"\bCASE\b", "cast": r"::|\bCAST\s*\(",
    "subquery": r"\(\s*(SELECT|WITH)\b", "exists": r"\bEXISTS\s*\(", "in": r"\bIN\s*\(",
    "like": r"\b(I?LIKE|SIMILAR\s+TO)\b", "between": r"\bBETWEEN\b", "any_all": r"\b(ANY|ALL)\s*\(",
    "array": r"\b(ARRAY|UNNEST)\b|\[\]", "window": r"\bOVER\s*\(", "returning": r"\bRETURNING\b",
    "upsert": r"\bON\s+CONFLICT\b", "default": r"\bDEFAULT\b", "join": r"\bJOIN\b",
    "set_operation": r"\b(UNION|INTERSECT|EXCEPT)\b", "date_time": r"\b(INTERVAL|DATE|TIMESTAMP|TIMESTAMPTZ|NOW)\b",
    "json": r"\bJSON\w*\b|->|#>", "row_comparison": r"\)\s*(<=|>=|<>|!=|=|<|>)\s*\(",
    "system_catalog": r"\b(information_schema|pg_catalog|pg_class|pg_namespace|pg_stat_activity)\b",
    "volatile_effect": r"\b(pg_notify|pg_terminate_backend|pg_advisory_xact_lock)\s*\(",
    "arithmetic": r"[+*/%]|(?<![<>!:])-(?![->])",
}


def without_comments(sql: str) -> str:
    """Retain quoted values while stripping comments for diagnostic inventory only."""
    token = r"'(?:''|[^'])*'|\"(?:\"\"|[^\"])*\"|--[^\n]*|/\*[\s\S]*?\*/"
    return re.sub(token, lambda match: " " if match[0].startswith(("--", "/*")) else match[0], sql)


def inventory(sql: str) -> dict:
    """Identify visible constructs and effect obligations without asserting semantics."""
    clean = without_comments(sql)
    masked = re.sub(r"'(?:''|[^'])*'", "''", clean)
    first = re.search(r"[A-Za-z_]+", masked)
    effects = [name for name in ["pg_notify", "pg_terminate_backend", "pg_advisory_xact_lock"] if re.search(r"\b" + name + r"\s*\(", masked, re.I)]
    return {"statement_prefix": first[0].upper() if first else "unknown",
            "features": sorted(name for name, pattern in PATTERNS.items() if re.search(pattern, masked, re.I)),
            "functions": sorted(set(re.findall(r"\b([a-zA-Z_][a-zA-Z_0-9]*)\s*\(", masked))),
            "numbered_parameters": sorted(set(int(value) for value in re.findall(r"\$(\d+)", masked))),
            "effect_obligations": effects, "classification_method": "lexical inventory; authoritative typed AST still required"}

"""Adversarial rejection checks for unsupported SQL and ignored request intent."""
from copy import deepcopy
import json

from cases import fixture, unary_table
from run import CLI, ROOT, RESULTS, digest, write_json
from process_helpers import communicate


def base_request(identifier: str) -> dict:
    """Create a valid closed-catalog request that each negative test changes once."""
    tables = {"r": unary_table("int32", (1,)), "s": unary_table("int32", (0,))}
    return fixture(identifier, "SELECT x FROM r", tables, ["int32"], "rejection")["request"]


def sql_rejections() -> list[dict]:
    """Declare syntax outside the supported fragment and SQL binding errors."""
    examples = {"write_without_metadata": "UPDATE r SET x = 2",
                "multiple": "SELECT 1; SELECT 2",
                "recursive": "WITH RECURSIVE q(x) AS (VALUES (1) UNION ALL SELECT x FROM q) SELECT x FROM q",
                "values_local_unknown": "(VALUES (NULL)) UNION (VALUES (1))",
                "nested_unknown": "(SELECT NULL UNION SELECT NULL) UNION SELECT 1",
                "ambiguous": "SELECT x FROM r JOIN s ON r.x = s.x", "hidden_qualifier": "SELECT r.x FROM r AS q",
                "missing_column": "SELECT y FROM r", "mixed_set_types": "SELECT TRUE UNION SELECT 1",
                "duplicate_cte": "WITH q AS (SELECT x FROM r), q AS (SELECT x FROM s) SELECT x FROM q",
                "forward_cte": "WITH q AS (SELECT x FROM later), later AS (SELECT x FROM r) SELECT x FROM q",
                "unicode_integer_whitespace": "SELECT 1 = '\u00a01\u00a0'",
                "unicode_boolean_whitespace": "SELECT TRUE = '\u00a0true\u00a0'",
                "cte_expansion_budget": cte_expansion_sql()}
    cases = []
    for label, sql in examples.items():
        request = base_request("reject/sql/" + label)
        request["sql"] = sql
        cases.append(request)
    return cases


def cte_expansion_sql() -> str:
    """Describe exponential CTE duplication with compact SQL to test binding budgets."""
    definitions = ["q0 AS (SELECT x FROM r)"]
    definitions.extend(f"q{i} AS (SELECT x FROM q{i-1} UNION ALL SELECT x FROM q{i-1})" for i in range(1, 21))
    return "WITH " + ",".join(definitions) + " SELECT x FROM q20"


def data_rejections() -> list[dict]:
    """Cover full-database validation, including tables not scanned by the query."""
    cases = []
    for label in ["extra_table", "missing_table", "unused_wrong_type", "wrong_arity", "wrong_null_type", "nonnullable_null"]:
        request = base_request("reject/data/" + label)
        if label == "extra_table": request["database"]["public.ghost"] = []
        if label == "missing_table": del request["database"]["public.s"]
        if label == "unused_wrong_type": request["database"]["public.s"] = [[{"type": "text", "value": "wrong"}]]
        if label == "wrong_arity": request["database"]["public.r"] = [[]]
        if label == "wrong_null_type": request["database"]["public.r"] = [[{"type": "null", "value": "bool"}]]
        if label == "nonnullable_null":
            request["catalog"]["tables"]["public.r"]["columns"][0]["nullable"] = False
            request["database"]["public.r"] = [[{"type": "null", "value": "int32"}]]
        cases.append(request)
    return cases


def intent_rejections() -> list[dict]:
    """Ensure wrong-mode fields, oversized scopes and missing parameters fail closed."""
    modifications = {"property_in_compare": {"property": {"kind": "max_rows", "max": 0}},
                     "bounds_in_compare": {"bounds": {"public.r": 1}},
                     "domain_in_compare": {"domain": [{"type": "int32", "value": 1}]},
                     "unknown_field": {"execute": True}, "unknown_mode": {"mode": "execute"},
                     "missing_parameter": {"sql": "SELECT $1"},
                     "capacity": {"sql": "SELECT * FROM r AS l CROSS JOIN r AS rr", "database": {"public.r": [[{"type": "int32", "value": 1}]] * 20, "public.s": []}},
                     "analyze_database": {"mode": "analyze", "property": {"kind": "max_rows", "max": 0}, "bounds": {"public.r": 1, "public.s": 1}, "domain": [{"type": "int32", "value": 1}]}}
    cases = []
    for label, change in modifications.items():
        request = base_request("reject/intent/" + label)
        request.update(deepcopy(change))
        cases.append(request)
    return cases


def execute(requests: list[dict], timeout: int = 120) -> list[dict]:
    """Use a separate result file and preserve positional errors without fabricated IDs."""
    wire = "".join(json.dumps(request) + "\n" for request in requests)
    output = communicate([str(CLI)], wire, timeout=timeout, cwd=ROOT)
    return [json.loads(line) for line in output.splitlines()]


def text_rejections() -> list[dict]:
    """Impossible NUL text must fail input admission even when its table or setting is unused."""
    cases = []
    invalid = {"type": "text", "value": "bad\0text"}
    for label in ["parameter", "unused_database", "unused_setting"]:
        request = base_request("reject/text/" + label)
        if label == "parameter": request.update(sql="SELECT $1", parameters={"1": invalid})
        if label == "unused_database":
            request["catalog"]["tables"]["public.s"]["columns"][0]["ty"] = "text"
            request["database"]["public.s"] = [[invalid]]
        if label == "unused_setting": request["execution"] = {"max_rows": 256, "max_outcomes": 256,
            "settings": {"unused": invalid}, "table_metadata": {}}
        cases.append(request)
    return cases


def main() -> None:
    """Require every negative case to fail explicitly and retain response evidence."""
    requests = sql_rejections() + data_rejections() + intent_rejections() + text_rejections()
    responses = execute(requests)
    if len(responses) != len(requests): raise ValueError("Incomplete rejection responses")
    findings = [{"id": request["id"], "response": response} for request, response in zip(requests, responses)]
    failures = [item for item in findings if item["response"].get("status") != "error"]
    failures += [item for item in findings if item["id"].startswith("reject/text/")
                 and item["response"].get("code") not in {"invalid_request", "invalid.text_nul"}]
    missing_ids = [item["id"] for item in findings if item["response"].get("id") != item["id"]]
    report = {"status": "failed" if failures or missing_ids else "passed", "cases": len(requests), "failures": failures,
              "missing_response_ids": missing_ids, "cli_sha256": digest(CLI.read_bytes()), "results": findings}
    write_json(RESULTS / "rejections.json", report)
    print(json.dumps({key: value for key, value in report.items() if key != "results"}, ensure_ascii=True))
    if failures or missing_ids: raise SystemExit(1)


if __name__ == "__main__":
    main()

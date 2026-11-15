"""Compare complete small ordering/pagination outcome sets with PostgreSQL permutations."""
from itertools import permutations
import json

from cases import fixture, table
from oracle import oracle_script
from oracle_contract import index_responses
from pg_runtime import TemporaryPostgres
from rejections import execute
from run import CLI, RESULTS, digest, index_lines, write_json


def queries() -> list[tuple]:
    """Exercise hidden keys, tie boundaries, nullable sorting and derived presentation scope."""
    result = [("plain_order", "SELECT x FROM r ORDER BY x", ["int32"], True),
              ("plain_limit", "SELECT x FROM r LIMIT 1", ["int32"], False),
              ("plain_offset", "SELECT x FROM r OFFSET 1", ["int32"], False),
              ("hidden_key", "SELECT label FROM r ORDER BY x LIMIT 1", ["text"], True),
              ("unordered", "SELECT label FROM r LIMIT 1", ["text"], False),
              ("offset", "SELECT label FROM r LIMIT 2 OFFSET 1", ["text"], False),
              ("distinct", "SELECT DISTINCT x FROM r ORDER BY x", ["int32"], True),
              ("derived", "SELECT label FROM (SELECT label FROM r ORDER BY x LIMIT 1) AS q", ["text"], False),
              ("cte", "WITH q AS (SELECT label FROM r ORDER BY x LIMIT 1) SELECT label FROM q", ["text"], False),
              ("ordinal", "SELECT label,x FROM r ORDER BY 2,1", ["text", "int32"], True),
              ("empty", "SELECT x FROM r ORDER BY x LIMIT 0", ["int32"], True)]
    for direction in ["ASC", "DESC"]:
        for nulls in ["FIRST", "LAST"]:
            result.append((direction + nulls, f"SELECT x,label FROM r ORDER BY x {direction} NULLS {nulls}", ["int32", "text"], True))
    return result


def build_cases() -> tuple[list, list, dict]:
    """Enumerate each physical permutation independently of SQLolla's ordering implementation."""
    rows = [[1, "a"], [1, "b"], [None, "c"]]
    bases, oracle, details = [], [], {}
    for label, sql, types, ordered in queries():
        identifier = "ordering/" + label
        base = fixture(identifier, sql, {"r": table([("x", "int32"), ("label", "text")], rows)}, types, "ordering")
        bases.append(base)
        details[identifier] = {"ordered": ordered, "permutations": []}
        for index, order in enumerate(permutations(rows)):
            variant = fixture(identifier + "/" + str(index), sql, {"r": table([("x", "int32"), ("label", "text")], list(order))}, types, "ordering")
            oracle.append(variant)
            details[identifier]["permutations"].append(variant["request"]["id"])
    return bases, oracle, details


def normalize(rows: list, ordered: bool) -> str:
    """Compare row order only at observable ORDER BY boundaries; bags retain duplicates."""
    encoded = [json.dumps(row, ensure_ascii=False) for row in rows]
    return json.dumps(encoded if ordered else sorted(encoded), ensure_ascii=False)


def actual_outcomes(response: dict, ordered: bool, request: dict) -> set[str]:
    """Require complete table encoding and unchanged read state before accepting any row set."""
    if response.get("status") != "ok" or response.get("validation") != "ALLOY_TABLE_ROUNDTRIP" or not response.get("complete"):
        raise ValueError(response)
    results = set()
    for outcome in response["outcomes"]:
        if outcome["status"] != "succeeded": raise ValueError(outcome)
        value = outcome["outcome"]
        if value["database"] != request["database"] or value["effects"]: raise ValueError("Read changed state/effects")
        if value["ordering"] != ("ordered" if ordered else "bag"): raise ValueError("Wrong ordering contract")
        if value["affected"] != len(value["rows"]) or value["command_tag"] != "SELECT " + str(len(value["rows"])):
            raise ValueError("Wrong affected count/command tag")
        rows = [[None if cell["type"] == "null" else cell["value"] for cell in row] for row in value["rows"]]
        results.add(normalize(rows, ordered))
    return results


def compare(bases: list, details: dict, oracle: dict, responses: list) -> list[dict]:
    """Require exact all-outcome equality in the tiny independently permuted domain."""
    responses = index_responses([case["request"] for case in bases], responses)
    findings = []
    for case in bases:
        request = case["request"]
        response = responses[request["id"]]
        detail = details[request["id"]]
        expected = {normalize(oracle[name]["rows"], detail["ordered"]) for name in detail["permutations"]}
        try:
            actual = actual_outcomes(response, detail["ordered"], request)
            findings.append({"id": request["id"], "status": "passed" if expected == actual else "failed",
                             "expected": sorted(expected), "actual": sorted(actual)})
        except ValueError as error:
            findings.append({"id": request["id"], "status": "failed", "error": str(error)})
    return findings


def main() -> None:
    """Execute real PostgreSQL and SAT, retaining one maintained report for the extension."""
    bases, oracle_cases, details = build_cases()
    with TemporaryPostgres() as database:
        version = database.execute("SELECT version();").strip()
        oracle = index_lines(database.execute(oracle_script(oracle_cases), timeout=120))
        directory = database.directory
    results = compare(bases, details, oracle, execute([case["request"] for case in bases]))
    report = {"status": "passed" if all(row["status"] == "passed" for row in results) else "failed",
              "cases": len(bases), "postgres_executions": len(oracle), "version": version,
              "cluster_removed": not directory.exists(), "cli_sha256": digest(CLI.read_bytes()),
              "scope": "Exact output-set equality over all six physical input permutations; not a universal PostgreSQL completeness proof.",
              "results": results}
    write_json(RESULTS / "ordering.json", report)
    print(json.dumps({key: value for key, value in report.items() if key != "results"}))
    if report["status"] != "passed": raise SystemExit(1)


if __name__ == "__main__":
    main()

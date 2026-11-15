"""Actual PostgreSQL oracle for a reusable bounded regular-language subset, not corpus patterns."""
import argparse
import json
from pathlib import Path

from corpus_runtime import checked, describe
from grouping import compare
from libpq_client import PgConnection
from oracle import literal
from oracle_contract import index_responses
from pg_runtime import TemporaryPostgres
from rejections import execute
from run import CLI, RESULTS, digest, write_json


def queries() -> list[str]:
    """Cross anchors, alternation, repetition, empty matches, classes and newline behavior."""
    patterns = ["", "a", "^a$", "a|bc", "(ab|a)*", "a+", "a?", "a{2,3}", "a{2,}", "a{0}", "a+?",
                "^[a-z]+$", "[^a-c]", "[A-z]", "[]a-]", ".", "^.$", "a$", "^_cc(new|old)[0-9]*$"]
    texts = ["", "a", "aaa", "bc", "ababa", "A_z", "a\n", "\n", "_ccnew", "_ccold12", "x_ccnew", "_ccnew2x"]
    result = ["SELECT " + literal(text) + " " + operator + " " + literal(pattern)
              for pattern in patterns for text in texts for operator in ["~", "!~*"]]
    result += ["SELECT 'é😀x' ~ " + literal(pattern) for pattern in ["é.", "^...$", "[é]", "[^é]"]]
    result += ["SELECT 'abc' ~ " + literal(pattern) for pattern in ["(", ")", "[a", "[z-a]", "a**", "*a", "\\", "a{2,1}"]]
    return result + ["SELECT NULL ~ '('", "SELECT 'abc' ~ NULL", "SELECT CASE WHEN false THEN 'a' ~ '(' ELSE true END"]


def requests() -> list[dict]:
    """Supply only explicit scalar inputs in a closed empty catalog."""
    return [{"id": "regex/" + str(index), "sql": sql, "catalog": {"search_path": ["public"], "tables": {}},
             "database": {}, "parameters": {}, "mode": "compare",
             "execution": {"max_rows": 256, "max_outcomes": 256, "settings": {}, "table_metadata": {}}}
            for index, sql in enumerate(queries())]


def refusal_requests() -> list[dict]:
    """Coverage refusals are tested separately and never counted as PG semantic matches."""
    patterns = ["(?=a)", "[[:digit:]]", "(a)\\1", "a{256}", "(" * 33 + "a" + ")" * 33]
    cases = [{"id": "regex/refusal/" + str(index), "sql": "SELECT 'a' ~ " + literal(pattern)} for index, pattern in enumerate(patterns)]
    cases += [{"id": "regex/refusal/case_locale", "sql": "SELECT 'é' ~* 'É'"}]
    for case in cases:
        case.update(catalog={"search_path": ["public"], "tables": {}}, database={}, parameters={}, mode="compare")
    return cases


def main() -> None:
    """Compare every completed engine outcome with PG; preserve explicit refusal evidence separately."""
    parser = argparse.ArgumentParser()
    parser.add_argument("--oracle-only", action="store_true")
    options, submitted = parser.parse_args(), requests()
    with TemporaryPostgres() as database:
        with PgConnection(database) as connection:
            checked(connection, "SET TIME ZONE 'UTC'")
            expected = {request["id"]: describe(connection, connection.query(request["sql"])) for request in submitted}
        version, directory = database.execute("SELECT version()").strip(), database.directory
    responses = {} if options.oracle_only else index_responses(submitted, execute(submitted, timeout=600))
    results = [{"id": request["id"], "sql": request["sql"], "postgres": expected[request["id"]]} if options.oracle_only
               else compare(request, expected[request["id"]], responses[request["id"]]) for request in submitted]
    rejected = refusal_requests()
    refusals = [] if options.oracle_only else list(index_responses(rejected, execute(rejected)).values())
    failures = [value["id"] for value in results if value.get("status") == "failed"]
    failures += [value.get("id") for value in refusals if value.get("status") != "error" or not value.get("code", "").startswith(("unsupported.regex", "resource.regex"))]
    report = {"status": "oracle_only" if options.oracle_only else "failed" if failures else "passed", "comparisons": len(results),
              "refusals": len(refusals), "failures": failures, "results": results, "refusal_results": refusals,
              "postgres_version": version, "cluster_removed": not directory.exists(), "harness_sha256": digest(Path(__file__).read_bytes()),
              "cli_sha256": None if options.oracle_only else digest(CLI.read_bytes()), "contract": "contracts/m2/SQLollaM2/RegexSubset.lean",
              "source": "https://www.postgresql.org/docs/16/functions-matching.html", "profile": "Exact literals and ASCII ranges; ASCII-only insensitive mode; bounded complete positional matching."}
    write_json(RESULTS / "regex-subset.json", report)
    print(json.dumps({key: value for key, value in report.items() if key not in ("results", "refusal_results")}))
    if failures: raise SystemExit(1)


if __name__ == "__main__":
    main()

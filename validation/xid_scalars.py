"""Independent PostgreSQL xid input, nominal equality and missing ordering/cast overloads."""
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
    """Separate nominal raw identity from implicit numeric casts and wraparound age comparisons."""
    values = ["0", "1", "4294967295", "-1", "-2147483648", "+1", "077", "0x10", " \t1\n",
              "", "foo", "08", "1_0", "0b1", "0x", "2.5", "\u00a01", "4294967296"]
    sql = ["SELECT " + literal(value) + "::xid" for value in values]
    sql += ["SELECT '4294967295'::xid " + op + " " + right for op in ["=", "<>", "<", "<=", ">", ">="]
            for right in ["'-1'::xid", "-1", "-1::bigint"]]
    return sql + ["SELECT 1 = '1'::xid", "SELECT -1 <> '4294967295'::xid", "SELECT 1::xid", "SELECT 1::bigint::xid",
                  "SELECT '1'::xid::int", "SELECT '1'::xid::oid", "SELECT '1'::xid::text",
                  "SELECT DISTINCT v FROM (VALUES('1'::xid),('1'::xid),('2'::xid),(NULL::xid))t(v)",
                  "SELECT v,count(*) FROM (VALUES('1'::xid),('1'::xid),('2'::xid))t(v) GROUP BY v",
                  "SELECT '1'::xid ORDER BY 1", "SELECT v FROM (SELECT '1'::xid v WHERE false)t ORDER BY v",
                  "SELECT '1'::xid UNION SELECT '1'::xid", "SELECT NULL::xid = NULL::xid",
                  "SELECT (x,1) = (x,1) FROM xid_inputs", "SELECT (x,1) < (x,1) FROM xid_inputs",
                  "SELECT ARRAY['1'::xid] = ARRAY['1'::xid]", "SELECT ARRAY['1'::xid] < ARRAY['2'::xid]",
                  "SELECT DISTINCT ARRAY['1'::xid]", "SELECT ARRAY['1'::xid] ORDER BY 1",
                  "SELECT NULL::xid[] < NULL::xid[]", "SELECT ARRAY[]::xid[] < ARRAY[]::xid[]",
                  "SELECT v FROM (VALUES(ARRAY['1'::xid]),(ARRAY['2'::xid]))t(v) ORDER BY v",
                  "SELECT v,count(*) FROM (VALUES(ARRAY['1'::xid]),(ARRAY['1'::xid]))t(v) GROUP BY v"]


def requests() -> list[dict]:
    """All values are explicit scalar SQL; no tuple header or transaction lookup is involved."""
    values = [{"id": f"xid/{index}", "sql": sql, "catalog": {"search_path": ["public"], "tables": {}},
             "database": {}, "parameters": {}, "mode": "compare",
             "execution": {"max_rows": 256, "max_outcomes": 256, "settings": {}, "table_metadata": {}}}
            for index, sql in enumerate(queries())]
    for wire in values:
        if wire["sql"].endswith("FROM xid_inputs"):
            wire["catalog"]["tables"]["public.xid_inputs"] = {"schema": "public", "name": "xid_inputs", "columns": [
                {"name": "x", "ty": "xid", "nullable": True, "qualifier": None, "schema": None}]}
            wire["database"] = {"public.xid_inputs": [[{"type": "scalar", "value": {"ty": "xid", "value": "1"}}]]}
    return values


def profile_refusals() -> list[dict]:
    """Retain actual platform observations while refusing C-width-dependent overflow/error precedence."""
    values = [base + suffix for base in ("42949672960", "0x100000000", "040000000000") for suffix in ("x", "_1")]
    template = requests()[0]
    return [dict(template, id=f"xid/overflow_profile/{ty}/{index}", sql="SELECT " + literal(value) + "::" + ty)
            for ty in ("oid", "xid") for index, value in enumerate(values)]


def compare_refusals(cases: list, expected: dict) -> list:
    """A platform boundary is an admission refusal, never a claimed PostgreSQL error match."""
    responses = index_responses(cases, execute(cases))
    return [{"id": case["id"], "sql": case["sql"], "postgres": expected[case["id"]], "response": responses[case["id"]],
             "status": "passed" if refusal_matches(case["id"], responses[case["id"]]) else "failed",
             "claim": "Explicit platform or parser input refusal, not semantic coverage."} for case in cases]


def parser_refusals() -> list[dict]:
    """Preserve the exact original cast+row combinations as uncovered syntax, not silently rewritten passes."""
    template = requests()[0]
    return [dict(template, id="xid/parser_row/" + op, sql=f"SELECT ('1'::xid,1) {op} ('{right}'::xid,1)")
            for op, right in (("=", "1"), ("<", "2"))]


def refusal_matches(identifier: str, response: dict) -> bool:
    """Only the specifically reviewed refusal family satisfies each negative admission case."""
    if response.get("status") != "error": return False
    if "/parser_row/" in identifier: return response.get("code", "").startswith("unsupported.cst_expression.")
    return response.get("code") == "unsupported.catalog_overflow_profile"


def oracle_value(connection, wire: dict) -> dict:
    """Use an explicitly declared stored xid for row semantics while keeping the original syntax gaps visible."""
    fixture = bool(wire["catalog"]["tables"])
    if fixture:
        checked(connection, "CREATE TABLE xid_inputs(x xid)")
        checked(connection, "INSERT INTO xid_inputs VALUES('1'::xid)")
    result = describe(connection, connection.query(wire["sql"]))
    if fixture:
        if checked(connection, "SELECT x::text FROM xid_inputs")["rows"] != [["1"]]: raise ValueError("Row fixture changed")
        checked(connection, "DROP TABLE xid_inputs")
    return result


def main() -> None:
    """Keep ordinary SQL failures and scalar result matches separate from coverage refusals."""
    parser = argparse.ArgumentParser()
    parser.add_argument("--oracle-only", action="store_true")
    options, submitted, rejected = parser.parse_args(), requests(), profile_refusals() + parser_refusals()
    with TemporaryPostgres() as database:
        with PgConnection(database) as connection:
            expected = {wire["id"]: oracle_value(connection, wire) for wire in submitted + rejected}
        version, directory = database.execute("SELECT version()").strip(), database.directory
    responses = {} if options.oracle_only else index_responses(submitted, execute(submitted, timeout=180))
    results = [{"id": wire["id"], "sql": wire["sql"], "postgres": expected[wire["id"]]} if options.oracle_only else
               compare(wire, expected[wire["id"]], responses[wire["id"]]) for wire in submitted]
    refusal_results = [{"id": wire["id"], "sql": wire["sql"], "postgres": expected[wire["id"]], "status": "oracle_only"}
                       for wire in rejected] if options.oracle_only else compare_refusals(rejected, expected)
    failures = [result["id"] for result in results + refusal_results if result.get("status") == "failed"]
    report = {"status": "oracle_only" if options.oracle_only else "failed" if failures else "passed", "cases": len(results),
              "failures": failures, "results": results, "refusals": len(refusal_results), "refusal_results": refusal_results,
              "postgres_version": version, "cluster_removed": not directory.exists(),
              "harness_sha256": digest(Path(__file__).read_bytes()), "cli_sha256": None if options.oracle_only else digest(CLI.read_bytes()),
              "contract": "contracts/m2/SQLollaM2/XidScalars.lean", "source": "https://github.com/postgres/postgres/blob/REL_16_16/src/backend/utils/adt/xid.c"}
    write_json(RESULTS / "xid-scalars.json", report)
    print(json.dumps({key: value for key, value in report.items() if key not in ("results", "refusal_results")}))
    if failures: raise SystemExit(1)


if __name__ == "__main__":
    main()

"""Independent PG16 oracle for JSON/JSONB constructors, existence and array element access."""
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


def json_queries() -> list[str]:
    """Cross every top-level JSONB shape, retaining duplicate and nested values."""
    values = ["null", '"a"', "2.10", "true", "[]", '["a",null,"a"]', "{}", '{"a":1}', '{"a":{"b":2},"bb":3}']
    queries = ["SELECT " + literal(left) + "::jsonb || " + literal(right) + "::jsonb" for left in values for right in values]
    for value in values:
        queries += ["SELECT " + literal(value) + "::jsonb " + operator + " " + key
                    for operator, key in [("?", "'a'"), ("?|", "ARRAY['a',NULL,'bb']"), ("?&", "ARRAY[NULL]::text[]"),
                                          ("?&", "ARRAY['a','bb']"), ("?|", "ARRAY[]::text[]")]]
    return queries + ["SELECT NULL::jsonb || '{}'::jsonb", "SELECT '{}'::jsonb ? NULL",
                      "SELECT '{}'::jsonb ?| NULL::text[]", "SELECT '{}'::jsonb ? true",
                      "SELECT jsonb_exists('{}','a')", "SELECT jsonb_exists_any('{}',ARRAY['a',NULL])"]


def constructors() -> list[str]:
    """Observe exact plain-JSON text, duplicate keys, non-strict NULL handling and SQL errors."""
    args = ["", "'a',1,'a',2", "'null',NULL,'bool',true,'text','quoted \"text\"'", "1,2,true,false", "NULL,1", "'odd'",
            "ARRAY[1],1", "'array',ARRAY[[1,NULL],[2,3]]", "'json','{ \"x\":1,\"x\":2 }'::json",
            "'time','2020-01-02 03:04:05+00'::timestamptz", "'number',1.20::numeric,'float','Infinity'::float8"]
    queries = ["SELECT " + fn + "(" + arg + ")" for fn in ["json_build_object", "jsonb_build_object"] for arg in args]
    queries += ["SELECT " + fn + "(" + arg + ")" for fn in ["json_build_array", "jsonb_build_array"]
                for arg in ["", "NULL,1,true,'x'", "ARRAY[1,NULL,2]", "'{\"a\":1}'::json,'[2,3]'::jsonb"]]
    return queries + ["SELECT '{  \"a\":1, \"a\":2 }'::json", "SELECT 'null'::json = 'null'::json"]


def arrays_and_bits() -> list[str]:
    """Exercise non-one lower bounds, dimension count, SQL NULL and zero-based bit indexes."""
    queries = ["SELECT " + base + index for base in ["(ARRAY[10,NULL,30])", "('[0:2]={10,NULL,30}'::int[])",
               "(ARRAY[[10,20],[30,40]])", "(NULL::int[])"] for index in ["[0]", "[1]", "[2]", "[1][2]", "[NULL]", "[-1]"]]
    queries += ["SELECT (ARRAY[10,20])[" + index + "]" for index in ["1.4", "1.6", "true", "1::bigint"]]
    return queries + ["SELECT get_bit('10110000'::bit(8)," + index + ")" for index in ["0", "1", "7", "8", "-1", "NULL", "2::smallint"]]


def array_updates() -> list[str]:
    """Cross slice clipping and dimension shapes; observe NULL append separately from strict measurements."""
    bases = ["ARRAY[10,NULL,30]", "'[0:2]={10,NULL,30}'::int[]", "ARRAY[[10,20],[30,40]]", "ARRAY[]::int[]", "NULL::int[]"]
    queries = ["SELECT (" + base + ")" + selector for base in bases for selector in
               ["[1:]", "[:1]", "[:]", "[0:0]", "[-5:2]", "[5:10]", "[NULL:2]", "[1:2][2]", "[1][1:1]"]]
    queries += ["SELECT array_length(" + base + "," + dimension + ")" for base in bases for dimension in ["-2147483648", "0", "1", "2", "NULL"]]
    queries += ["SELECT cardinality(" + base + ")" for base in bases]
    queries += ["SELECT array_append(" + base + "," + value + "),array_prepend(" + value + "," + base + ")"
                for base in bases for value in ["0", "NULL"]]
    return queries + ["SELECT array_append(ARRAY[1],2.5)", "SELECT array_append(NULL,NULL)",
                      "SELECT array_append('[2147483647:2147483647]={1}'::int[],2)",
                      "SELECT '[2147483647:2147483647]={1}'::int[]", "SELECT '[2147483646:2147483646]={1}'::int[]"]


def json_updates() -> list[str]:
    """Cross existing/missing parents, insertion boundaries, NULL path entries and lazy suffix errors."""
    targets = ["{}", "[]", '{"a":1}', '{"a":{"b":2}}', "[1,2]", '[{"a":1},[2,3]]']
    paths = ["{}", "{a}", "{a,b}", "{0}", "{-4}", "{4}", "{NULL}", "{a,NULL}"]
    queries = ["SELECT jsonb_set(" + literal(target) + "," + literal(path) + ",'9'," + create + ")"
               for target in targets for path in paths for create in ["true", "false"]]
    return queries + ["SELECT jsonb_set('1','{}','2')", "SELECT jsonb_set('[1,2]','{2147483648}','3')",
                      "SELECT jsonb_set('[1,2]','{1.0}','3')", "SELECT jsonb_set('[1,2]',ARRAY['1 '],'3')",
                      "SELECT jsonb_set('{}',ARRAY[['a']],'3')", "SELECT jsonb_set('{}',NULL,'3')",
                      "SELECT jsonb_set('{}','{a}',NULL)", "SELECT jsonb_set('{}','{a}','3',NULL)"]


def string_collections() -> list[str]:
    """Distinguish absent delimiters/markers, empty fields, Unicode characters and SQL versus JSON null."""
    texts = ["NULL", "''", "'a,,b,'", "'é😀x'", "'aaaa'"]
    delimiters = ["NULL", "''", "','", "'aa'"]
    queries = ["SELECT string_to_array(" + text + "," + delimiter + "," + marker + ")"
               for text in texts for delimiter in delimiters for marker in ["NULL", "''", "'b'"]]
    arrays = ["ARRAY[1,NULL,2]", "ARRAY[true,NULL,false]", "ARRAY[[1,2],[3,4]]", "ARRAY[]::text[]", "NULL::text[]"]
    queries += ["SELECT array_to_string(" + array + "," + delimiter + "," + marker + ")"
                for array in arrays for delimiter in ["NULL", "'|'", "''"] for marker in ["NULL", "'missing'"]]
    queries += ["SELECT jsonb_typeof(" + value + ")" for value in ["NULL", "'null'", "'true'", "'3.20'", "'\"x\"'", "'[]'", "'{}'"]]
    return queries + ["SELECT string_to_array('a,b',',')", "SELECT array_to_string(ARRAY[1,NULL,2],',')",
                      "SELECT jsonb_typeof(1)", "SELECT string_to_array(true,',')"]


def json_extraction() -> list[str]:
    """Cross extraction overloads, NULL distinctions, signed indexes and structural path failures."""
    values = ["null", '""', '"é😀"', "2.10", "true", "[]", '[1,null,"x"]', "{}",
              '{"a":null,"0":"field","b":[1,{"x":"q"}]}', '{"a":1,"a":2}']
    selectors = ["'a'", "'0'", "0", "-1", "-2147483648", "2147483647", "NULL", "NULL::integer"]
    queries = ["SELECT " + literal(value) + "::jsonb " + op + " " + key
               for value in values for op in ["->", "->>"] for key in selectors]
    paths = ["{}", "{a}", "{b,1,x}", "{0}", "{-1}", "{2147483648}", "{NULL}", "{b,NULL}", "{1.0}"]
    queries += ["SELECT " + literal(value) + "::jsonb " + op + " " + literal(path) + "::text[]"
                for value in values for op in ["#>", "#>>"] for path in paths]
    return queries + ["SELECT NULL::jsonb -> 'x'", "SELECT '{}'::jsonb -> true", "SELECT '[]'::jsonb -> 1::bigint",
                      "SELECT '[1,2]'::jsonb -> 1::smallint", "SELECT '{}'::jsonb #> NULL::text[]",
                      "SELECT '[1,2]'::jsonb #> ARRAY['1 ']", "SELECT '[1,2]'::jsonb #> ARRAY[' 1']",
                      "SELECT '{\"a\":1}'::jsonb #> ARRAY[['a']]",
                      "SELECT coalesce(CASE WHEN jsonb_typeof('{\"river:rescue_count\":2}'::jsonb -> 'river:rescue_count')='number' THEN ('{\"river:rescue_count\":2}'::jsonb ->> 'river:rescue_count')::int END,0)+1"]


def numeric_signatures() -> list[str]:
    """Check numeric overload resolution before NULL folding, including typmods and signed overflow."""
    arguments = ["true", "NULL::boolean", "'-2'::text", "DATE '2020-01-01'", "ARRAY[1]", "", "1,2",
                 "NULL", "'-2'", "'-2'::varchar(3)", "-2::smallint", "-2", "-2::bigint",
                 "-2.10::numeric(4,2)", "NULL::numeric(4,2)", "'-0'::float8", "'NaN'::float8",
                 "'-Infinity'::float8", "'-32768'::smallint", "-2147483648", "'-9223372036854775808'::bigint"]
    return ["SELECT abs(" + argument + ")" for argument in arguments] + [
        "SELECT -(2.10::numeric(4,2))", "SELECT +(2.10::numeric(4,2))",
        "SELECT -(NULL::numeric(4,2))", "SELECT +(NULL::numeric(4,2))"]


def requests() -> list[dict]:
    """All scalar inputs are explicit SQL literals; no production catalog is consulted."""
    queries = json_queries() + constructors() + arrays_and_bits() + array_updates() + json_updates() + string_collections() + json_extraction() + numeric_signatures()
    return [{"id": "collections/" + str(index), "sql": sql, "catalog": {"search_path": ["public"], "tables": {}},
             "database": {}, "parameters": {}, "mode": "compare",
             "execution": {"max_rows": 256, "max_outcomes": 256, "settings": {}, "table_metadata": {}}}
            for index, sql in enumerate(queries)]


def main() -> None:
    """Keep actual oracle evidence even when a newly admitted scalar path still fails."""
    parser = argparse.ArgumentParser()
    parser.add_argument("--oracle-only", action="store_true")
    options, submitted = parser.parse_args(), requests()
    with TemporaryPostgres() as database:
        with PgConnection(database) as connection:
            checked(connection, "SET TIME ZONE 'UTC'")
            expected = {request["id"]: describe(connection, connection.query(request["sql"])) for request in submitted}
        version, directory = database.execute("SELECT version()").strip(), database.directory
    responses = {} if options.oracle_only else index_responses(submitted, execute(submitted, timeout=300))
    results = [{"id": request["id"], "sql": request["sql"], "postgres": expected[request["id"]]} if options.oracle_only
               else compare(request, expected[request["id"]], responses[request["id"]]) for request in submitted]
    failures = [value["id"] for value in results if value.get("status") == "failed"]
    report = {"status": "oracle_only" if options.oracle_only else "failed" if failures else "passed", "cases": len(results),
              "failures": failures, "results": results, "postgres_version": version, "cluster_removed": not directory.exists(),
              "cli_sha256": None if options.oracle_only else digest(CLI.read_bytes()),
              "harness_sha256": digest(Path(__file__).read_bytes()), "session_profile": {"timezone": "UTC"},
              "contracts": ["contracts/m2/SQLollaM2/Collections.lean", "contracts/m2/SQLollaM2/StringCollections.lean",
                            "contracts/m2/SQLollaM2/FunctionResolution.lean", "contracts/m2/SQLollaM2/Values.lean"],
              "sources": ["https://www.postgresql.org/docs/16/functions-json.html", "https://www.postgresql.org/docs/16/arrays.html",
                          "https://www.postgresql.org/docs/16/functions-array.html", "https://www.postgresql.org/docs/16/functions-bitstring.html",
                          "https://www.postgresql.org/docs/16/functions-math.html"],
              "claim": "Actual PG observations versus every Rust outcome; Alloy checks an evaluator-backed transition table."}
    write_json(RESULTS / "collections.json", report)
    print(json.dumps({key: value for key, value in report.items() if key != "results"}))
    if failures: raise SystemExit(1)


if __name__ == "__main__":
    main()

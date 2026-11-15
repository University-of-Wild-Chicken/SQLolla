"""Independent PostgreSQL16 interval ranges, precision, named construction and admission boundaries."""
from collections import Counter
import json

from corpus_runtime import describe
from grouping import compare
from libpq_client import PgConnection
from oracle_contract import index_responses
from pg_runtime import TemporaryPostgres
from rejections import execute
from run import CLI, RESULTS, digest, write_json

FIELDS = ("", "YEAR", "MONTH", "DAY", "HOUR", "MINUTE", "SECOND", "YEAR TO MONTH", "DAY TO HOUR",
          "DAY TO MINUTE", "DAY TO SECOND", "HOUR TO MINUTE", "HOUR TO SECOND", "MINUTE TO SECOND")


def qualified_queries() -> list[str]:
    """Cross all declared ranges with unmarked values and independently signed stored components."""
    sources = ("3", "1 year 2 mons 3 days 04:05:06.789", "-1 year -2 mons -3 days -04:05:06.789")
    result = [f"SELECT INTERVAL '{value}' {fields}".rstrip() for fields in FIELDS for value in sources]
    result += [f"SELECT '3'::text::interval {fields}".rstrip() for fields in FIELDS]
    result += [f"SELECT (INTERVAL '3 seconds')::interval {fields}".rstrip() for fields in FIELDS]
    for precision in range(7):
        result += [f"SELECT INTERVAL '{value}' SECOND({precision})" for value in ("1.2345", "-1.2345", "0.5", "-0.5", "1.999999", "-1.999999")]
    return result


def literal_queries() -> list[str]:
    """Exercise canonical rendering, finite fractions, clock interpretation and actual parse errors."""
    values = ("1.75 months", "-1.75 months", "1.5 years", "0.125 years", "1.5 weeks", "-1.5 days",
              "1.5 hours", "1.5 minutes", "0.0000005 seconds", "0.0000015 seconds", "0.0000025 seconds",
              "1:2", "-1 02:03:04", "-1 mon 1 day 1 hour", "1 day -1 hour", "1-2", "-1-2", "@ 2 days ago",
              "1 hour 2 hour", "1 week 2 days", "1 second 2 milliseconds", "25:60:00", "1-13",
              "-0.0000015 seconds", "0.0000016 seconds", "-0.0000016 seconds", "1.5 microseconds",
              "-1.5 microseconds", "0:00:00.0000015", "-0:00:00.0000015", "0:00.0000015",
              "1.5 seconds 2 milliseconds", "2 milliseconds 1.5 seconds", "1:02.5")
    result = [f"SELECT INTERVAL '{value}'" for value in values]
    result += ["SELECT INTERVAL '1:2' MINUTE TO SECOND", "SELECT '1.2345'::interval(3)",
               "SELECT INTERVAL(3) '1.2345'", "SELECT DATE '2020-01-31' + INTERVAL '1' MONTH"]
    return result


def constructor_queries() -> list[str]:
    """Keep signature errors distinct from strict NULL behavior and finite value calculations."""
    bodies = ("", "1", "1,2,3,4,5,6,7.89", "days=>10", "secs=>1", "secs:=1", "secs=>7.89,years=>1,months=>2,weeks=>3,days=>4,hours=>5,mins=>6",
              "days=>NULL", "secs=>NULL", "days=>1,days=>2", "foo=>1", "1,years=>2", "days=>1,2", "1,2,3,4,5,6,7,8",
              "1::bigint", "days=>true")
    result = ["SELECT make_interval(" + body + ")" for body in bodies]
    result += [f"SELECT make_interval(secs=>{value})" for value in ("0.0000005", "0.0000015", "0.0000025", "-0.0000005", "-0.0000015", "-0.0000025")]
    return result


def requests() -> tuple[list, dict]:
    """Declare every differential case and separately label intentional coverage refusals."""
    refusal = {"SELECT make_interval(years=>2147483647)": "unsupported.interval_constructor_overflow",
               "SELECT make_interval(secs=>'NaN'::float8)": "unsupported.interval_nonfinite",
               "SELECT make_interval(secs=>'Infinity'::float8)": "unsupported.interval_nonfinite",
               "SELECT INTERVAL '1' SECOND(7)": "unsupported.interval_precision",
               "SELECT INTERVAL 'P1Y'": "unsupported.interval_iso_input"}
    sqls = list(dict.fromkeys(qualified_queries() + literal_queries() + constructor_queries() + list(refusal)))
    requests = [{"id": "interval/" + str(index), "sql": sql, "catalog": {"search_path": ["public"], "tables": {}},
                 "database": {}, "parameters": {}, "mode": "compare",
                 "execution": {"max_rows": 256, "max_outcomes": 256, "settings": {}, "table_metadata": {}}}
                for index, sql in enumerate(sqls)]
    return requests, refusal


def result(request: dict, expected: dict, response: dict, refusals: dict) -> dict:
    """Never count a deliberate admission refusal as implemented PostgreSQL semantics."""
    if request["sql"] not in refusals: return dict(compare(request, expected, response), classification="differential")
    matched = response.get("status") == "error" and response.get("code") == refusals[request["sql"]]
    return {"id": request["id"], "sql": request["sql"], "status": "passed" if matched else "failed",
            "classification": "coverage_refusal", "expected_code": refusals[request["sql"]], "postgres": expected, "response": response}


def main() -> None:
    """Compare actual PostgreSQL observations with complete evaluator-backed SAT outcomes."""
    submitted, refusals = requests()
    responses = index_responses(submitted, execute(submitted, timeout=300))
    with TemporaryPostgres() as database:
        with PgConnection(database) as connection:
            results = [result(request, describe(connection, connection.query(request["sql"])), responses[request["id"]], refusals) for request in submitted]
        version, directory = database.execute("SELECT version();").strip(), database.directory
    failures = [row["id"] for row in results if row["status"] == "failed"]
    report = {"status": "failed" if failures else "passed", "cases": len(results), "failures": failures,
              "classifications": dict(Counter(row["classification"] for row in results)), "results": results,
              "postgres_version": version, "cluster_removed": not directory.exists(), "cli_sha256": digest(CLI.read_bytes()),
              "sources": ["https://www.postgresql.org/docs/16/datatype-datetime.html#DATATYPE-INTERVAL-INPUT",
                          "https://www.postgresql.org/docs/16/functions-datetime.html",
                          "https://github.com/postgres/postgres/blob/REL_16_16/src/backend/utils/adt/datetime.c",
                          "https://github.com/postgres/postgres/blob/REL_16_16/src/backend/utils/adt/timestamp.c"],
              "claim": "Independent PostgreSQL fixture comparison; five declared coverage refusals are not semantic support."}
    write_json(RESULTS / "intervals.json", report)
    print(json.dumps({key: value for key, value in report.items() if key != "results"}))
    if failures: raise SystemExit(1)


if __name__ == "__main__":
    main()

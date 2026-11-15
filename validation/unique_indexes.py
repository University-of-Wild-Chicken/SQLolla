"""Actual PostgreSQL standalone/partial unique-index enforcement and conflict inference evidence."""
import json
import copy

from corpus_runtime import checked, describe
from corpus_wire import inspect, typed_rows
from cte import compare
from libpq_client import PgConnection
from oracle_contract import index_responses
from pg_runtime import TemporaryPostgres
from rejections import execute
from run import CLI, RESULTS, digest, write_json


def cases() -> list[tuple[str, str, str]]:
    """Separate index membership, arbiter inference, constraint names and SQL NULL behavior."""
    partial = "CREATE UNIQUE INDEX idx ON t(x) WHERE b"
    total = "CREATE UNIQUE INDEX idx ON t(x)"
    rows = "INSERT INTO t VALUES(1,2,true),(1,3,false),(1,4,NULL)"
    queries = ["INSERT INTO t VALUES(1,5,false) RETURNING *", "INSERT INTO t VALUES(1,5,true) RETURNING *",
               "INSERT INTO t VALUES(1,5,NULL) RETURNING *", "INSERT INTO t VALUES(1,5,true) ON CONFLICT DO NOTHING RETURNING *",
               "INSERT INTO t VALUES(1,5,true) ON CONFLICT(x) WHERE b DO NOTHING RETURNING *",
               "INSERT INTO t VALUES(1,5,true) ON CONFLICT(x) WHERE b DO UPDATE SET y=excluded.y RETURNING *",
               "INSERT INTO t VALUES(1,5,false) ON CONFLICT(x) WHERE b DO UPDATE SET y=excluded.y RETURNING *",
               "INSERT INTO t VALUES(1,5,true) ON CONFLICT(x) WHERE b AND y>0 DO NOTHING RETURNING *",
               "INSERT INTO t VALUES(1,5,false) ON CONFLICT(x) WHERE b AND y>0 DO NOTHING RETURNING *",
               "INSERT INTO t VALUES(1,5,true) ON CONFLICT(x) DO NOTHING RETURNING *",
               "INSERT INTO t VALUES(1,5,true) ON CONFLICT ON CONSTRAINT idx DO NOTHING RETURNING *",
               "UPDATE t SET b=true WHERE y=3 RETURNING *", "DELETE FROM t WHERE b RETURNING *",
               "INSERT INTO t SELECT 2,5,true WHERE false ON CONFLICT(x) DO NOTHING"]
    result = [(partial, rows, sql) for sql in queries]
    result += [(total, "INSERT INTO t VALUES(1,2,true)", sql) for sql in (
        "INSERT INTO t VALUES(1,5,false) ON CONFLICT(x) DO UPDATE SET y=excluded.y RETURNING *",
        "INSERT INTO t VALUES(1,5,false) ON CONFLICT(x) WHERE b DO NOTHING RETURNING *",
        "INSERT INTO t VALUES(1,5,false) ON CONFLICT ON CONSTRAINT idx DO NOTHING")]
    for suffix in ("", " NULLS NOT DISTINCT"):
        result.append(("CREATE UNIQUE INDEX idx ON t(x)" + suffix, "INSERT INTO t VALUES(NULL,2,true)",
                       "INSERT INTO t VALUES(NULL,3,false) ON CONFLICT(x) DO NOTHING RETURNING *"))
    return result + ordered_cases()


def ordered_cases() -> list:
    """Reproduce physical index order across different uniqueness rule kinds."""
    result = []
    unique_then_pk = ["ALTER TABLE t ADD CONSTRAINT t_x_key UNIQUE(x)", "ALTER TABLE t ADD CONSTRAINT t_pkey PRIMARY KEY(y)"]
    pk_then_unique = list(reversed(unique_then_pk))
    index_then_pk = ["CREATE UNIQUE INDEX early_idx ON t(x)", "ALTER TABLE t ADD CONSTRAINT t_pkey PRIMARY KEY(y)"]
    partial_then_pk = ["CREATE UNIQUE INDEX early_idx ON t(x) WHERE b", "ALTER TABLE t ADD CONSTRAINT t_pkey PRIMARY KEY(y)"]
    for definitions in (unique_then_pk, pk_then_unique, index_then_pk, partial_then_pk):
        result.append((definitions, "INSERT INTO t VALUES(1,2,true)", "INSERT INTO t VALUES(1,2,true) RETURNING *"))
        result.append((definitions, "INSERT INTO t VALUES(1,2,true),(3,4,true)", "UPDATE t SET x=1,y=2 WHERE x=3 RETURNING *"))
    return result


def snapshot(connection) -> dict:
    """Observe full physical table bags after restoring the statement savepoint on SQL failure."""
    return {"database": {"public.t": describe(connection, checked(connection, "SELECT * FROM t"))}}


def observe(database, connection, index: int, case: tuple) -> tuple[dict, dict]:
    """Derive metadata from real PG catalogs and independently execute the exact statement text."""
    index_sql, seed, sql = case
    checked(connection, "DROP TABLE IF EXISTS t")
    checked(connection, "CREATE TABLE t(x int,y int,b boolean)")
    for definition in index_sql if isinstance(index_sql, list) else [index_sql]:
        checked(connection, definition)
    checked(connection, seed)
    semantics = inspect(database)
    before = snapshot(connection)
    columns = semantics["catalog"]["tables"]["public.t"]["columns"]
    request = {"id": "unique_indexes/" + str(index), "sql": sql, "catalog": semantics["catalog"],
               "database": {"public.t": typed_rows(before["database"]["public.t"]["rows"], [column["ty"] for column in columns])},
               "parameters": {}, "mode": "compare", "execution": {"max_rows": 256, "max_outcomes": 256,
               "settings": {}, "table_metadata": semantics["table_metadata"]}}
    explained = connection.query("EXPLAIN " + sql)
    checked(connection, "BEGIN")
    checked(connection, "SAVEPOINT statement_boundary")
    result = describe(connection, connection.query(sql))
    if result["status"] == "error": checked(connection, "ROLLBACK TO SAVEPOINT statement_boundary")
    after = snapshot(connection)
    checked(connection, "ROLLBACK")
    return request, {"result": result, "after": after, "explain": explained,
                     "observed_phase": "planning" if explained["status"] == "error" else "statement",
                     "index_observations": semantics["index_observations"]}


def immutability_fixtures(database, connection) -> list:
    """Forge stable predicates only to require refusal, anchored by real PostgreSQL CREATE rejection."""
    base, _ = observe(database, connection, "purity_base",
        ("CREATE UNIQUE INDEX idx ON t(x) WHERE b", "INSERT INTO t SELECT 1,2,true WHERE false", "SELECT x FROM t"))
    predicates = ["now() IS NOT NULL", "current_setting('missing_setting',true) IS NULL",
                  "concat(x::text,'') = '1'", "('2000-01-01'::text)::date IS NOT NULL"]
    result = []
    for index, predicate in enumerate(predicates):
        actual = connection.query("CREATE UNIQUE INDEX rejected_index ON t(x) WHERE " + predicate)
        if actual["status"] != "error" or actual["sqlstate"] != "42P17":
            raise AssertionError((predicate, actual))
        request = copy.deepcopy(base)
        request["id"] = "unique_indexes/immutability_" + str(index)
        request["execution"]["table_metadata"]["public.t"]["unique_indexes"][0]["predicate"]["sql"] = predicate
        result.append((request, actual))
    return result


def compare_admission(request, actual, response) -> dict:
    """A nonexistent stable index is invalid context, never an SQL execution counterexample."""
    code = response.get("code") if response.get("status") == "error" else None
    expected = actual.get("expected_refusal", "unsupported.index_immutability")
    return {"id": request["id"], "status": "passed" if code == expected else "failed",
            "postgres_observation": actual, "response": response,
            "scope": actual.get("scope", "Metadata admission refusal; not a comparison of query outcomes against a nonexistent index.")}


def deferred_fixture(database, connection) -> tuple:
    """Observe a successful statement and failing COMMIT to justify a distinct admission boundary."""
    sql = "INSERT INTO t VALUES(1,3,true) RETURNING *"
    request, statement = observe(database, connection, "deferred",
        ("ALTER TABLE t ADD CONSTRAINT deferred_key UNIQUE(x) DEFERRABLE INITIALLY DEFERRED",
         "INSERT INTO t VALUES(1,2,true)", sql))
    checked(connection, "BEGIN")
    inserted = connection.query(sql)
    commit = connection.query("COMMIT")
    if inserted["status"] != "ok" or commit.get("sqlstate") != "23505":
        raise AssertionError((inserted, commit))
    return request, {"expected_refusal": "unsupported.deferred_constraints", "statement": statement,
                     "commit": commit, "scope": "Statement success is distinct from transaction-end rejection."}


def main() -> None:
    """Compare every complete Rust/Alloy outcome with independently observed PG state and errors."""
    with TemporaryPostgres() as database:
        with PgConnection(database) as connection:
            fixtures = [observe(database, connection, index, case) for index, case in enumerate(cases())]
            admission = immutability_fixtures(database, connection)
            admission.append(deferred_fixture(database, connection))
        version, directory = database.execute("SELECT version();").strip(), database.directory
    submitted = [request for request, _ in fixtures + admission]
    responses = index_responses(submitted, execute(submitted, timeout=240))
    results = [compare(request, oracle, responses[request["id"]]) for request, oracle in fixtures]
    results += [compare_admission(request, oracle, responses[request["id"]]) for request, oracle in admission]
    failures = [result["id"] for result in results if result["status"] != "passed"]
    report = {"status": "failed" if failures else "passed", "cases": len(results), "failures": failures,
              "postgres_version": version, "cluster_removed": not directory.exists(),
              "cli_sha256": digest(CLI.read_bytes()), "results": results,
              "scope": "Ordinary column btree indexes, optional deterministic partial predicates, exact/conjunction inference; no expression indexes or arbitrary implication completeness."}
    write_json(RESULTS / "unique_indexes.json", report)
    print(json.dumps({key: value for key, value in report.items() if key != "results"}))
    if failures: raise SystemExit(1)


if __name__ == "__main__":
    main()

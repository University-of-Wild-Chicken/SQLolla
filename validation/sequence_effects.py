"""Actual PostgreSQL sequence state, lazy scalar/CTE composition and atomic INSERT failure."""
from copy import deepcopy
import json

from corpus_runtime import checked, describe
from corpus_sequences import capture, relation_lookup
from corpus_semantics import catalog_unchanged, observation_matches, schema_matches
from corpus_wire import typed_rows
from libpq_client import PgConnection
from oracle_contract import index_responses, top_level_ordered
from pg_runtime import TemporaryPostgres
from rejections import execute
from run import CLI, RESULTS, digest, write_json

SCHEMAS = {"t": [("id", "int64", True), ("v", "int32", False)],
           "p": [("a", "int64", True), ("b", "int64", True), ("k", "int32", True)],
           "e": [("a", "int64", True), ("b", "int32", True)]}


def query_cases() -> list[dict]:
    """Scalar cases distinguish planning elimination from runtime failure and inherited prefixes."""
    sqls = ["SELECT nextval('s')", "SELECT nextval('s'),nextval('s')",
        "SELECT COALESCE(7::bigint,nextval('s'))", "SELECT COALESCE(NULL::bigint,nextval('s'))",
        "SELECT CASE WHEN false THEN nextval('s') ELSE 7::bigint END",
        "SELECT CASE WHEN true THEN nextval('s') ELSE 7::bigint END",
        "SELECT nextval(NULL::regclass)", "SELECT NULL::bigint+nextval('s')",
        "SELECT NULL::bigint+(nextval('s')/0)", "SELECT concat(NULL::text,nextval('s'))",
        "SELECT nextval('s')/0", "SELECT nextval('s'),1/0", "SELECT NULL::bigint+(1/0)",
        "SELECT nextval('s') FROM(SELECT nextval('s') AS n) q",
        "SELECT (SELECT nextval('s')),nextval('s')",
        "WITH a AS(SELECT nextval('s') AS n) SELECT a.n,a.n FROM a",
        "WITH a AS(SELECT nextval('s') AS n),b AS(SELECT a.n,nextval('s') AS m FROM a) SELECT * FROM b",
        "WITH unused AS(SELECT nextval('s')) SELECT 7",
        "SELECT CASE WHEN false THEN 1 ELSE 7::bigint END",
        "SELECT (CASE WHEN false THEN 1 ELSE 7 END)::bigint",
        "SELECT CASE WHEN false THEN 1 ELSE x::bigint END FROM(VALUES(7))t(x)",
        "SELECT (CASE WHEN false THEN 1 ELSE x END)::bigint FROM(VALUES(7))t(x)"]
    rows = [{"sql": sql, "existing": False} for sql in sqls]
    rows.append({"sql": "WITH a AS(SELECT nextval('s') AS n),b AS(SELECT nextval('s') AS n) SELECT a.n,b.n FROM a CROSS JOIN b",
                 "existing": False, "alternatives": [[["1", "2"]], [["2", "1"]]]})
    rows.append({"sql": "SELECT 1 AS k,nextval('s') AS n UNION ALL SELECT 2,nextval('s')",
                 "existing": False, "alternatives": [[["1", "1"], ["2", "2"]], [["1", "2"], ["2", "1"]]]})
    return rows


def insert_cases() -> list[dict]:
    """Defaults allocate per proposal, including failed or ignored proposals and physical column order."""
    examples = [("INSERT INTO t DEFAULT VALUES RETURNING *", False),
        ("INSERT INTO t(v) VALUES(7) RETURNING *", False),
        ("INSERT INTO t(id,v) VALUES(88,7) RETURNING *", False),
        ("INSERT INTO t(id,v) VALUES(NULL,7) RETURNING *", False),
        ("INSERT INTO t(id,v) VALUES(DEFAULT,8) RETURNING *", False),
        ("INSERT INTO t(v) VALUES(7) RETURNING *", True),
        ("INSERT INTO t(v) VALUES(7) ON CONFLICT(v) DO NOTHING RETURNING *", True),
        ("INSERT INTO t(v) VALUES(7) ON CONFLICT(v) DO UPDATE SET v=excluded.v RETURNING *", True),
        ("INSERT INTO t(v) VALUES(7),(7) RETURNING *", False),
        ("INSERT INTO t(v) VALUES(8),(9) RETURNING *", False),
        ("INSERT INTO t(v) SELECT 7 WHERE false RETURNING *", False),
        ("INSERT INTO t(v) SELECT 7 RETURNING *", False),
        ("INSERT INTO p(b,a,k) VALUES(nextval('s'),nextval('s'),1) RETURNING *", False),
        ("INSERT INTO p(k) VALUES(1) RETURNING *", False),
        ("INSERT INTO p(a,k) VALUES(88,1) RETURNING *", False),
        ("INSERT INTO e DEFAULT VALUES RETURNING *", False),
        ("INSERT INTO e(a) VALUES(nextval('s')) RETURNING *", False),
        ("INSERT INTO e(a,b) VALUES(nextval('s'),DEFAULT) RETURNING *", False)]
    return [{"sql": sql, "existing": existing} for sql, existing in examples]


def setup(database) -> None:
    """Create independent PostgreSQL schema once so captured OIDs stay valid for every request."""
    database.execute("CREATE SEQUENCE s CACHE 1;"
        "CREATE TABLE t(id bigint DEFAULT nextval('s'),v integer NOT NULL DEFAULT 7 UNIQUE);"
        "CREATE TABLE p(a bigint DEFAULT nextval('s'),b bigint DEFAULT nextval('s'),k integer UNIQUE);"
        "CREATE TABLE e(a bigint DEFAULT nextval('s'),b integer DEFAULT 1/0);")


def scheduling_boundaries() -> list[dict]:
    """A known PostgreSQL tuple interleaving remains a coverage refusal; dependent defaults are exercised."""
    return [{"sql": "WITH a AS(SELECT nextval('s') AS n FROM(VALUES(1),(2))x(k)),b AS(SELECT nextval('s') AS n FROM(VALUES(1),(2))x(k)) SELECT a.n,b.n FROM a CROSS JOIN b",
             "existing": False, "expected_refusal": "unsupported.cte_tuple_schedule"},
            {"sql": "WITH a AS(INSERT INTO t(v) VALUES(7) RETURNING id) INSERT INTO p(a,k) SELECT id,1 FROM a RETURNING *", "existing": False},
            {"sql": "WITH a AS(INSERT INTO t(v) VALUES(7) RETURNING id),b AS(INSERT INTO e DEFAULT VALUES RETURNING *) SELECT * FROM a", "existing": False}]


def consumer_boundaries() -> list[dict]:
    """Empty joins, correlated membership and failed insertion expose real partial producer demand."""
    source = "WITH a AS(SELECT nextval('s') AS n FROM(VALUES(1),(2),(3))x(k)) "
    examples = [("WITH a AS(SELECT nextval('s') AS n) SELECT a.n FROM t CROSS JOIN a", False, "unsupported.cte_consumer_demand"),
        (source + "SELECT a.n FROM t LEFT JOIN a ON true", False, "unsupported.cte_consumer_demand"),
        (source + "SELECT (t.id-98) IN(SELECT n FROM a WHERE n>=t.id-98) FROM t", True, "unsupported.cte_consumer_demand"),
        (source + "INSERT INTO t(id,v) SELECT n,7 FROM a RETURNING *", True, "unsupported.cte_consumer_error"),
        (source + "INSERT INTO p(a) SELECT n FROM a RETURNING a,b", False, "unsupported.cte_tuple_schedule"),
        ("WITH e AS(SELECT nextval('s') n,x FROM(VALUES(1),(2))v(x)) SELECT * FROM e WHERE x=1 AND x=2", False, "unsupported.cte_consumer_demand"),
        ("SELECT n,nextval('s') AS m FROM(SELECT nextval('s') AS n FROM(VALUES(1),(2))t(x))q", False, "unsupported.projection_effect_interleaving")]
    return [{"sql": sql, "existing": existing, "expected_refusal": refusal} for sql, existing, refusal in examples]


def lineage_cases() -> list[dict]:
    """A nonempty pure consumer forces every projected occurrence without replaying allocations."""
    return [{"sql": "WITH a AS(SELECT nextval('s') AS n FROM(VALUES(1),(2),(3))x(k)) SELECT a.n FROM t CROSS JOIN a", "existing": True},
        {"sql": "WITH a AS(SELECT nextval('s') AS n FROM(VALUES(1),(2),(3))x(k)),b AS(SELECT a.n FROM t CROSS JOIN a) SELECT * FROM b", "existing": True}]


def producer_boundaries() -> list[dict]:
    """Full consumption of a producer's output does not certify hidden partial effect sites inside it."""
    source = "SELECT nextval('s') AS n FROM(VALUES(1),(2),(3))x(k)"
    return [{"sql": "WITH e AS(" + source + " LIMIT 1) SELECT * FROM e", "existing": False,
             "expected_refusal": "unsupported.nested_lock_demand"},
        {"sql": "WITH e AS(SELECT * FROM(" + source + ")q WHERE n>0) SELECT * FROM e", "existing": False,
             "expected_refusal": "unsupported.nested_lock_demand"},
        {"sql": "WITH e AS(SELECT EXISTS(SELECT 1 FROM(VALUES(1),(2))v(x) WHERE nextval('s')>0)) SELECT * FROM e", "existing": False,
             "expected_refusal": "unsupported.subquery_effect_demand"}]


def membership_cases() -> list[dict]:
    """Full unique positive membership proves demand without assuming PostgreSQL hashed materialization."""
    producer = "WITH q AS(SELECT nextval('s') n,x FROM(VALUES{values})v(x)) "
    statement = "UPDATE t SET v=v+10 WHERE id IN(SELECT x FROM q) RETURNING id,v"
    cases = [("(1),(2)", [[1,7],[2,8]], None), ("(1),(2)", [[1,7]], "unsupported.cte_consumer_demand"),
        ("(1),(2)", [], "unsupported.cte_consumer_demand"), ("(1),(1)", [[1,7]], "unsupported.cte_consumer_demand"),
        ("(1),(NULL)", [[1,7]], "unsupported.cte_consumer_demand")]
    return [{"sql": producer.format(values=values) + statement, "existing": False,
        "target_rows": rows, **({"expected_refusal": refusal} if refusal else {})} for values, rows, refusal in cases]


def metadata() -> dict:
    """Provide complete declared target behavior without substituting expected query outcomes."""
    sequence = {"kind": "expression", "sql": "nextval('s'::regclass)", "parser": None}
    unique = lambda name, position: {"name": name, "columns": [position]}
    return {"t": {"complete": True, "defaults": {"0": sequence, "1": {"kind": "literal", "value": {"type": "int32", "value": 7}}},
                  "unique": [unique("t_v_key", 1)], "unique_enforcement_order": ["t_v_key"]},
            "p": {"complete": True, "defaults": {"0": sequence, "1": sequence},
                  "unique": [unique("p_k_key", 2)], "unique_enforcement_order": ["p_k_key"]},
            "e": {"complete": True, "defaults": {"0": sequence, "1": {"kind": "expression", "sql": "1/0", "parser": None}}}}


def table_state(connection) -> dict:
    """Read all declared tables so unchanged relations remain part of poststate comparison."""
    return {name: describe(connection, checked(connection, "SELECT * FROM " + name)) for name in SCHEMAS}


def request(connection, case: dict, index: int, before: dict) -> dict:
    """Use exact same-session sequence metadata and relation identities for strict regclass resolution."""
    lookup = relation_lookup(connection)
    tables = {name: {"schema": "public", "name": name, "columns": [
        {"name": label, "ty": ty, "nullable": nullable} for label, ty, nullable in columns]} for name, columns in SCHEMAS.items()}
    rows = {name: typed_rows(before[name]["rows"], [ty for _, ty, _ in columns]) for name, columns in SCHEMAS.items()}
    return {"id": "sequence/" + str(index), "sql": case["sql"], "mode": "compare", "parameters": {},
        "catalog": {"search_path": lookup["search_path"], "tables": tables, "relation_lookup": lookup}, "database": rows,
        "execution": {"max_outcomes": 256, "max_rows": 256, "settings": {}, "table_metadata": metadata(),
            "catalog_lookup": lookup, "sequences": capture(connection, {})}}


def observe(connection, case: dict, index: int) -> tuple[dict, dict]:
    """Capture planning, statement and post-rollback state without assuming sequence transactional rollback."""
    checked(connection, "TRUNCATE t,p,e")
    checked(connection, "ALTER SEQUENCE s RESTART WITH 1")
    if case["existing"]: checked(connection, "INSERT INTO t(id,v) VALUES(99,7)")
    for row in case.get("target_rows", []): checked(connection, "INSERT INTO t(id,v) VALUES(" + ",".join(str(value) for value in row) + ")")
    before = table_state(connection)
    wire = request(connection, case, index, before)
    explained = connection.query("EXPLAIN " + case["sql"])
    checked(connection, "BEGIN")
    checked(connection, "SAVEPOINT observed_statement")
    result = connection.query(case["sql"])
    if result["status"] == "error": checked(connection, "ROLLBACK TO observed_statement")
    result = describe(connection, result)
    after = {"database": table_state(connection), "sequences": capture(connection, {})}
    checked(connection, "ROLLBACK")
    rolled_back = {"database": table_state(connection), "sequences": capture(connection, {})}
    return wire, {"result": result, "before": before, "after": after, "rolled_back": rolled_back,
                  "explain": explained, "observed_phase": "planning" if explained["status"] == "error" else "statement"}


def trace_matches(events: list, initial: dict, observed: dict) -> bool:
    """Independently validate each allocation against declared arithmetic and actual final sequence state."""
    state = {name: dict(value["state"]) for name, value in initial["sequences"].items()}
    for event in events:
        if event["kind"] != "sequence_value" or event["sequence"] not in state: return False
        name = event["sequence"]
        specification, prior = initial["sequences"][name], state[name]
        expected = prior["last_value"] + (specification["increment"] if prior["is_called"] else 0)
        if event["value"] != expected: return False
        state[name] = {"last_value": expected, "is_called": True}
    actual = {name: value["state"] for name, value in observed["sequences"].items()}
    return state == actual


def permitted_results(case: dict, expected: dict) -> list[dict]:
    """Independent declared order alternatives never replace the actual PostgreSQL membership check."""
    if "alternatives" not in case: return [expected]
    results = []
    for rows in case["alternatives"]:
        value = deepcopy(expected)
        value["rows"] = rows
        results.append(value)
    return results


def compare(case: dict, wire: dict, expected: dict, response: dict) -> dict:
    """Check every declared branch, PostgreSQL membership, sequence deltas and retained rollback state."""
    if case.get("expected_refusal"): return compare_refusal(case, wire, expected, response)
    errors = []
    if response.get("status") != "ok" or not response.get("complete") or response.get("validation") != "ALLOY_TABLE_ROUNDTRIP": errors.append("incomplete_execution")
    else:
        outcomes = response["outcomes"]
        variants = permitted_results(case, expected["result"])
        if not outcomes: errors.append("empty_outcomes")
        for outcome in outcomes:
            if not any(observation_matches(outcome, result, expected["after"], wire["catalog"], {}, top_level_ordered(wire["sql"])) for result in variants): errors.append("outcome_mismatch")
            if outcome["status"] == "succeeded" and not catalog_unchanged(outcome["outcome"].get("catalog"), wire["catalog"]): errors.append("catalog_mismatch")
            if not trace_matches(outcome["outcome"]["effects"], wire["execution"]["sequences"], expected["after"]["sequences"]): errors.append("sequence_trace_mismatch")
            if outcome["status"] == "sql_error" and outcome["outcome"]["phase"] not in ("unspecified", expected["observed_phase"]): errors.append("phase_mismatch")
        if not any(observation_matches(value, expected["result"], expected["after"], wire["catalog"], {}, top_level_ordered(wire["sql"])) for value in outcomes): errors.append("postgres_not_member")
        if expected["result"]["status"] == "ok" and not schema_matches(response["schema"], expected["result"]["columns"], {}): errors.append("schema_mismatch")
    if expected["after"]["sequences"] != expected["rolled_back"]["sequences"]: errors.append("oracle_sequence_rollback")
    if expected["before"] != expected["rolled_back"]["database"]: errors.append("oracle_table_rollback")
    return {"id": wire["id"], "sql": wire["sql"], "sql_sha256": digest(wire["sql"].encode()),
        "status": "failed" if errors else "passed", "errors": sorted(set(errors)),
        "initial_sequence_context": wire["execution"]["sequences"], "postgres": expected, "response": response}


def compare_refusal(case: dict, wire: dict, expected: dict, response: dict) -> dict:
    """A missing tuple scheduler cannot receive a successful semantic coverage count."""
    valid = response.get("status") == "error" and response.get("code") == case["expected_refusal"] and not response.get("complete", False)
    return {"id": wire["id"], "sql": wire["sql"], "sql_sha256": digest(wire["sql"].encode()),
        "status": "refusal_verified" if valid else "failed", "expected_refusal": case["expected_refusal"],
        "errors": [] if valid else ["missing_refusal"], "postgres": expected, "response": response}


def catalog_evidence(results: list[dict]) -> dict:
    """Retain exact output catalogs once by hash instead of duplicating identical inventories per case."""
    catalogs = {}
    for result in results:
        result["response_sha256"] = digest(json.dumps(result["response"], sort_keys=True).encode())
        for outcome in result["response"].get("outcomes", []):
            catalog = outcome["outcome"].get("catalog")
            if catalog is None: continue
            identity = digest(json.dumps(catalog, sort_keys=True).encode())
            catalogs[identity] = catalog
            outcome["outcome"]["catalog"] = {"evidence_ref": identity}
    return catalogs


def main() -> None:
    """Retain one maintained same-session sequence oracle and remove its disposable PostgreSQL cluster."""
    cases = query_cases() + insert_cases() + scheduling_boundaries() + consumer_boundaries() + lineage_cases() + producer_boundaries() + membership_cases()
    with TemporaryPostgres() as database:
        setup(database)
        with PgConnection(database) as connection:
            evidence = [observe(connection, case, index) for index, case in enumerate(cases)]
        version, directory = database.execute("SELECT version();").strip(), database.directory
        wires = [wire for wire, _ in evidence]
        responses = index_responses(wires, execute(wires, timeout=360))
        results = [compare(case, wire, expected, responses[wire["id"]]) for case, (wire, expected) in zip(cases, evidence)]
    failures = [row["id"] for row in results if row["status"] == "failed"]
    report = {"status": "failed" if failures else "passed", "cases": len(results), "failures": failures,
        "semantic_passes": sum(row["status"] == "passed" for row in results),
        "verified_refusals": sum(row["status"] == "refusal_verified" for row in results),
        "catalog_observations": catalog_evidence(results), "results": results,
        "catalog_context": wires[0]["catalog"], "table_metadata": metadata(),
        "postgres_version": version, "cli_sha256": digest(CLI.read_bytes()), "cluster_removed": not directory.exists(),
        "scope": "Actual PostgreSQL membership, every declared branch checked against independent outcomes and retained sequence state; two schedule cases additionally use explicitly listed order alternatives. Cache=1 exclusive snapshot; no concurrent sequence consumers."}
    write_json(RESULTS / "sequence-effects.json", report)
    print(json.dumps({key: value for key, value in report.items() if key not in ("results", "catalog_context", "catalog_observations", "table_metadata")}))
    if failures: raise SystemExit(1)


if __name__ == "__main__":
    main()

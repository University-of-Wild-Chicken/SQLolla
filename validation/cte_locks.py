"""Actual PostgreSQL CTE/derived locks, same-command snapshots and atomic effect rollback."""
import json

from cases import fixture, table
from corpus_locks import observe as observe_locks
from corpus_runtime import checked, describe
from corpus_semantics import effects_key, observation_matches, schema_matches
from libpq_client import PgConnection
from oracle_contract import index_responses, top_level_ordered
from pg_runtime import TemporaryPostgres
from rejections import execute
from run import CLI, RESULTS, digest, write_json


def examples() -> list[tuple[str, str, bool]]:
    """Positive paths consume their locked inputs fully; uncovered demand is explicitly refused."""
    source = "(SELECT id,v FROM r FOR UPDATE) q"
    return [
        ("derived", "SELECT * FROM " + source, False),
        ("subplan_filter", "SELECT * FROM " + source + " WHERE id NOT IN(SELECT 2)", False),
        ("pure_filter", "SELECT * FROM " + source + " WHERE id=1", True),
        ("limit", "SELECT * FROM " + source + " LIMIT 1", True),
        ("failure_rollback", "SELECT 1/(id-1) FROM " + source, False),
        ("ordered", "SELECT * FROM " + source + " ORDER BY id DESC", False),
        ("grouped", "SELECT count(*) FROM " + source, False),
        ("distinct", "SELECT DISTINCT v FROM " + source, False),
        ("distinct_on", "SELECT DISTINCT ON(v) id,v FROM " + source + " ORDER BY v,id", False),
        ("window", "SELECT id,row_number() OVER(ORDER BY id) FROM " + source, False),
        ("srf", "SELECT id,unnest(ARRAY[id,id]) FROM " + source, False),
        ("union_strength", "SELECT * FROM(SELECT id FROM r FOR SHARE) a UNION ALL SELECT * FROM(SELECT id FROM r FOR UPDATE) b", False),
        ("cte", "WITH q AS(SELECT id FROM r FOR UPDATE) SELECT * FROM q", False),
        ("cte_false", "WITH q AS(SELECT id FROM r FOR UPDATE) SELECT * FROM q WHERE false", True),
        ("cte_limit", "WITH q AS(SELECT id FROM r FOR UPDATE) SELECT * FROM q LIMIT 1", True),
        ("cte_exists", "WITH q AS(SELECT id FROM r FOR UPDATE) SELECT EXISTS(SELECT * FROM q)", True),
        ("cte_unused", "WITH q AS(SELECT id FROM r FOR UPDATE) SELECT 1", False),
        ("delete_filtered", "WITH q AS(SELECT id FROM r WHERE id=1 FOR UPDATE), d AS(DELETE FROM r USING q WHERE r.id=q.id AND r.v<>0 RETURNING r.*) SELECT * FROM d", True),
        ("delete_returning", "WITH q AS(SELECT id FROM r WHERE id=1 FOR UPDATE), d AS(DELETE FROM r USING q WHERE r.id=q.id RETURNING r.*) SELECT * FROM r UNION ALL SELECT * FROM d", False),
        ("delete_false", "WITH q AS(SELECT id FROM r WHERE id=1 FOR UPDATE), d AS(DELETE FROM r USING q WHERE r.id=q.id AND false RETURNING r.*) SELECT * FROM d", True),
        ("update", "WITH q AS(SELECT id FROM r WHERE id=1 FOR UPDATE) UPDATE r SET v=7 FROM q WHERE r.id=q.id RETURNING r.*", False),
        ("delete_all_required", "WITH q AS(SELECT v FROM r FOR UPDATE) DELETE FROM r USING q WHERE r.v=q.v RETURNING r.id", False),
        ("duplicate_source", "WITH q AS(SELECT 0 AS v FROM r FOR UPDATE) DELETE FROM r USING q WHERE r.v=q.v RETURNING r.id", True),
        ("cte_failure", "WITH q AS(SELECT id FROM r FOR UPDATE) SELECT 1/(id-1) FROM q", True),
    ] + fallback_examples() + membership_examples()


def fallback_examples() -> list[tuple[str, str, bool]]:
    """A writable CTE executes fully even when a fallback only tests membership of RETURNING rows."""
    return [("writable_fallback", "WITH q AS(SELECT id FROM r WHERE id=1 FOR UPDATE),d AS(DELETE FROM r USING q WHERE r.id=q.id RETURNING r.*) SELECT * FROM r WHERE id=1 AND id NOT IN(SELECT id FROM d) UNION SELECT * FROM d", False),
        ("writable_locked_fallback", "WITH q AS(SELECT id FROM r WHERE id=1 FOR UPDATE),d AS(UPDATE r SET v=7 FROM q WHERE r.id=q.id RETURNING r.*) SELECT * FROM(SELECT * FROM r WHERE id=1 FOR UPDATE)f WHERE id NOT IN(SELECT id FROM d) UNION SELECT * FROM d", False)]


def membership_examples() -> list[tuple[str, str, bool]]:
    """Positive membership needs a unique true comparison witness for every locked occurrence."""
    source = "WITH q AS(SELECT id FROM r FOR UPDATE) "
    return [("in_unique", source + "DELETE FROM r WHERE id IN(SELECT id FROM q) RETURNING *", False),
        ("in_duplicate", "WITH q AS(SELECT 1 AS id FROM r FOR UPDATE) DELETE FROM r WHERE id IN(SELECT id FROM q) RETURNING *", True),
        ("in_null", "WITH q AS(SELECT NULL::integer AS id FROM r WHERE id=1 FOR UPDATE) DELETE FROM r WHERE id IN(SELECT id FROM q) RETURNING *", True),
        ("in_negated", source + "DELETE FROM r WHERE id NOT IN(SELECT id FROM q) RETURNING *", True),
        ("in_correlated", source + "DELETE FROM r WHERE id IN(SELECT id FROM q WHERE q.id>=r.id) RETURNING *", True),
        ("in_partial", source + "DELETE FROM r WHERE id IN(SELECT id FROM q LIMIT 1) RETURNING *", True),
        ("in_error", source + "DELETE FROM r WHERE id IN(SELECT 1/(id-1) FROM q) RETURNING *", True),
        ("in_numeric_equality", "WITH q AS(SELECT (v::numeric/10)::numeric(8,2) AS k FROM r FOR UPDATE) DELETE FROM r WHERE v::numeric/10 IN(SELECT k FROM q) RETURNING *", False),
        ("in_numeric_duplicates", "WITH q AS(SELECT CASE WHEN id=1 THEN 1.0::numeric ELSE 1.00::numeric END AS k FROM r WHERE id<=2 FOR UPDATE) DELETE FROM r WHERE id::numeric IN(SELECT k FROM q) RETURNING *", True),
        ("in_and", source + "DELETE FROM r WHERE id IN(SELECT id FROM q) AND v>=0 RETURNING *", False),
        ("in_or", source + "DELETE FROM r WHERE id IN(SELECT id FROM q) OR v=0 RETURNING *", True),
        ("in_or_true", source + "DELETE FROM r WHERE id IN(SELECT id FROM q) OR true RETURNING *", True)]


def request(label: str, sql: str) -> dict:
    """Explicit initial ownership and complete mutation metadata contain no inferred lock state."""
    wire = fixture("cte_locks/" + label, sql, {"r": table([("id", "int32"), ("v", "int32")], [[1, 0], [2, 10], [3, 20]])}, [], "cte_locks")["request"]
    wire["execution"] = {"max_rows": 256, "max_outcomes": 256, "settings": {},
        "table_metadata": {"public.r": {"complete": True}},
        "locks": {"complete": True, "transaction": "subject", "held_rows": [], "row_share_conflicts": []}}
    return wire


def observe(subject, observer, sql: str) -> dict:
    """Observe result, full poststate and independent physical ownership before subject rollback."""
    explained = subject.query("EXPLAIN " + sql)
    checked(subject, "BEGIN")
    physical = {"r": [row[0] for row in checked(subject, "SELECT ctid::text FROM r ORDER BY id")["rows"]]}
    checked(subject, "SAVEPOINT target_statement")
    result = subject.query(sql)
    if result["status"] == "error": checked(subject, "ROLLBACK TO target_statement")
    result = describe(subject, result)
    state = describe(subject, checked(subject, "SELECT * FROM r"))
    effects = observe_locks(subject, observer, physical)
    checked(subject, "ROLLBACK")
    return {"result": result, "after": {"database": {"r": state}}, "effects": effects,
            "explain": explained, "observed_phase": "planning" if explained["status"] == "error" else "statement"}


def compare(wire: dict, expected: dict, response: dict, refusal: bool) -> dict:
    """Require every complete outcome to match PostgreSQL; coverage refusals receive separate credit."""
    errors = []
    if refusal:
        if response.get("status") != "error" or not response.get("code", "").startswith("unsupported.") or response.get("complete", False): errors.append("missing_refusal")
    elif response.get("status") != "ok" or not response.get("complete") or response.get("validation") != "ALLOY_TABLE_ROUNDTRIP": errors.append("incomplete_execution")
    else:
        if not response["outcomes"]: errors.append("empty_outcomes")
        for value in response["outcomes"]:
            if not observation_matches(value, expected["result"], expected["after"], wire["catalog"], {}, top_level_ordered(wire["sql"])): errors.append("postgres_outcome_mismatch")
            if effects_key(value["outcome"]["effects"]) != effects_key(expected["effects"]["effects"]): errors.append("held_lock_mismatch")
            if value["status"] == "sql_error" and value["outcome"]["phase"] not in ("unspecified", expected["observed_phase"]): errors.append("phase_mismatch")
        if expected["result"]["status"] == "ok" and not schema_matches(response["schema"], expected["result"]["columns"], {}): errors.append("schema_mismatch")
    return {"id": wire["id"], "sql": wire["sql"], "expected_refusal": refusal,
        "status": "failed" if errors else "passed", "errors": sorted(set(errors)), "postgres": expected, "response": response}


def main() -> None:
    """Keep one maintained differential artifact and remove the private PostgreSQL cluster."""
    cases = examples()
    wires = [request(label, sql) for label, sql, _ in cases]
    responses = index_responses(wires, execute(wires, timeout=300))
    with TemporaryPostgres() as database:
        database.execute("CREATE TABLE r(id integer,v integer);INSERT INTO r VALUES(1,0),(2,10),(3,20);")
        with PgConnection(database) as subject, PgConnection(database) as observer:
            results = [compare(wire, observe(subject, observer, sql), responses[wire["id"]], refusal)
                for wire, (_, sql, refusal) in zip(wires, cases)]
        version, directory = database.execute("SELECT version();").strip(), database.directory
    failures = [row["id"] for row in results if row["status"] == "failed"]
    report = {"status": "failed" if failures else "passed", "cases": len(cases),
        "semantic_cases": sum(not row[2] for row in cases), "verified_refusals": sum(row[2] for row in cases),
        "failures": failures, "results": results, "postgres_version": version,
        "cli_sha256": digest(CLI.read_bytes()), "cluster_removed": not directory.exists(),
        "scope": "Fixed snapshot; explicit row-share/table-row ownership observed independently. No concurrent update schedule or whole transaction lock claim."}
    write_json(RESULTS / "cte-locks.json", report)
    print(json.dumps({key: value for key, value in report.items() if key != "results"}))
    if failures: raise SystemExit(1)


if __name__ == "__main__":
    main()

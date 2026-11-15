"""Adversarial mutations prove that independent oracle comparison rejects altered evidence."""
from copy import deepcopy
from decimal import Decimal, localcontext
import unittest

from corpus_semantics import catalog_unchanged, decimal_text, normalized, observation_matches
from corpus_wire import typed_rows, verified_payload
from corpus_runtime import invalidate_semantics
from corpus_effects import compare as compare_effects
from corpus_ddl import merge_request
from run import compare_case
from extended_symbolic import cases, validate_concrete, witness_admitted
from oracle_contract import index_responses, top_level_ordered


def comparison_fixture() -> tuple:
    """Declare a deterministic two-row ordered observation with a complete unchanged table."""
    case = deepcopy(cases()[2])
    case["request"]["sql"] = "SELECT x FROM r ORDER BY x"
    rows = [[{"type": "int32", "value": 1}], [{"type": "int32", "value": 2}]]
    expected = {"status": "ok", "affected_rows": 2, "command": "SELECT 2", "post_rows": [["1"], ["2"]],
                "rows": [["1"], ["2"]], "columns": [{"name": "x", "type": "integer"}]}
    outcome = {"status": "succeeded", "outcome": {"rows": rows, "ordering": "ordered", "affected": 2,
               "command_tag": "SELECT 2", "database": {"public.r": deepcopy(rows)}, "effects": []}}
    response = {"status": "ok", "complete": True, "validation": "ALLOY_TABLE_ROUNDTRIP",
                "schema": [{"name": "x", "ty": "int32"}], "outcomes": [outcome]}
    return case, {"allowed": [expected]}, response


class ComparatorMutations(unittest.TestCase):
    """Tests target false acceptance, including mutants that preserve a SAT existence verdict."""

    def test_native_suite_requires_independent_encoding(self):
        """A finite outcome table cannot silently replace the native relational Alloy comparison."""
        result = compare_case({"request": {"id": "native-regression"}}, {},
                              {"status": "ok", "alloy_status": "SAT", "validation": "ALLOY_TABLE_ROUNDTRIP"})
        self.assertEqual(result["reason"], "independent_native_encoding_missing")

    def test_view_context_uses_observed_effective_path(self):
        """Default public lookup must not discard PostgreSQL's observed implicit catalog precedence."""
        wire = {"catalog": {"search_path": ["public"], "tables": {},
                            "functions": {"search_path": ["public"], "definitions": []}}}
        observed = {"namespace": {}, "views": {}, "effective_search_path": ["pg_catalog", "public"]}
        merge_request(wire, observed)
        self.assertEqual(wire["catalog"]["search_path"], observed["effective_search_path"])
        self.assertEqual(wire["catalog"]["functions"]["search_path"], observed["effective_search_path"])
        wire["catalog"]["search_path"].append("other")
        self.assertEqual(observed["effective_search_path"], ["pg_catalog", "public"])

    def test_effect_families_do_not_mask_each_other(self):
        """Sequence events must neither corrupt lock comparison nor conceal a wrong retained allocation."""
        lock = {"kind": "table_lock", "table": "t", "mode": "row_share"}
        event = {"kind": "sequence_value", "sequence": "s", "value": 1}
        before = {"advisory_locks": [], "sequence_context": {"sequences": {"s": {"state": {"last_value": 1, "is_called": False}}}}}
        after = deepcopy(before)
        after["sequence_context"]["sequences"]["s"]["state"]["is_called"] = True
        evidence = {"unobserved_effects": [], "sequence_state_changed": True, "row_locks": {"effects": [lock]}}
        outcome = {"outcome": {"effects": [lock, event]}}
        self.assertEqual(compare_effects(evidence, before, after, [outcome]), ([outcome], [], None))
        wrong = deepcopy(outcome)
        wrong["outcome"]["effects"][1]["value"] = 2
        self.assertEqual(compare_effects(evidence, before, after, [wrong])[2], "sequence_state")
        wrong["outcome"]["effects"] = [event]
        self.assertEqual(compare_effects(evidence, before, after, [wrong])[2], "row_lock_effects")

    def test_unobserved_tuple_event_stays_pending(self):
        """Typed row matches cannot discharge a new physical effect without its independent header evidence."""
        state = {"advisory_locks": []}
        evidence = {"unobserved_effects": [], "sequence_state_changed": False}
        outcome = {"outcome": {"effects": [{"kind": "tuple_version"}]}}
        matches, pending, reason = compare_effects(evidence, state, state, [outcome])
        self.assertEqual(matches, [outcome])
        self.assertIn("tuple_header_observation", pending)
        self.assertIsNone(reason)

    def test_zero_effect_output_is_not_backend_absence_proof(self):
        """An empty trace alone cannot discharge the backend obligation for an unobserved source topology."""
        state = {"advisory_locks": []}
        evidence = {"unobserved_effects": ["pg_terminate_backend"], "sequence_state_changed": False}
        outcome = {"outcome": {"effects": []}}
        self.assertIn("pg_terminate_backend", compare_effects(evidence, state, state, [outcome])[1])

    def test_replaced_oracle_cannot_retain_coverage(self):
        """A fresh PostgreSQL observation invalidates both its prior verdict and total coverage."""
        case = {"semantic_fixtures": {"baseline": {"status": "matched_pg_outcome_and_alloy_table"}},
                "semantic_execution": {"status": "matched_pg_outcome_and_alloy_table"}}
        manifest = {"cases": [case], "counts": {"semantic_execution": {"matched_pg_outcome_and_alloy_table": 1},
                    "semantic_fixtures": {"matched_pg_outcome_and_alloy_table": 1}}, "semantic_coverage_target": {"records": 1}}
        invalidate_semantics(manifest, case)
        self.assertNotIn("semantic_fixtures", case)
        self.assertEqual(manifest["counts"]["semantic_execution"], {"stale_oracle_replaced": 1})
        self.assertNotIn("semantic_fixtures", manifest["counts"])
        self.assertNotIn("semantic_coverage_target", manifest)

    def test_hidden_catalog_change_is_rejected(self):
        """Matching rows cannot excuse an omitted catalog or an injected view definition."""
        source = {"search_path": ["public"], "tables": {}}
        self.assertTrue(catalog_unchanged(dict(source), source))
        self.assertFalse(catalog_unchanged(None, source))
        changed = dict(source, views={"public.hidden": {"source_sql": "SELECT 99"}})
        self.assertFalse(catalog_unchanged(changed, source))
        self.assertFalse(catalog_unchanged(dict(source, search_path=["attacker", "public"]), source))

    def test_decimal_precision_never_rounds(self):
        """Distinct 31-digit numbers stay distinct even under deliberately tiny Decimal precision."""
        left, right = Decimal("1234567890123456789012345678901"), Decimal("1234567890123456789012345678902")
        with localcontext() as context:
            context.prec = 2
            self.assertNotEqual(decimal_text(left), decimal_text(right))
            self.assertEqual(decimal_text(Decimal("1.0000")), decimal_text(Decimal("1")))
        wrap = lambda text: {"type": "scalar", "value": {"ty": "jsonb", "value": text}}
        self.assertNotEqual(normalized(wrap(str(left))), normalized(wrap(str(right))))

    def test_engine_cannot_downgrade_order(self):
        """A reversed ORDER BY result cannot be excused by changing its ordering flag to bag."""
        case, observation, response = comparison_fixture()
        self.assertEqual(validate_concrete(case, observation, response)["status"], "passed")
        response["outcomes"][0]["outcome"]["rows"].reverse()
        response["outcomes"][0]["outcome"]["ordering"] = "bag"
        self.assertEqual(validate_concrete(case, observation, response)["status"], "failed")

    def test_extra_and_missing_outcomes_fail(self):
        """Membership alone is insufficient: an extra erroneous alternative must fail."""
        case, observation, response = comparison_fixture()
        changed = deepcopy(response["outcomes"][0])
        changed["outcome"]["rows"][0][0]["value"] = 99
        response["outcomes"].append(changed)
        self.assertEqual(validate_concrete(case, observation, response)["status"], "failed")
        response["outcomes"] = []
        self.assertEqual(validate_concrete(case, observation, response)["status"], "failed")

    def test_hidden_state_or_effects_fail(self):
        """Correct output rows cannot hide an altered unused table state or side effect."""
        for mutation in ("state", "effect", "count", "tag"):
            case, observation, response = comparison_fixture()
            value = response["outcomes"][0]["outcome"]
            if mutation == "state": value["database"]["public.r"][0][0]["value"] = 9
            if mutation == "effect": value["effects"] = [{"kind": "notification", "channel": "x", "payload": "y"}]
            if mutation == "count": value["affected"] = 1
            if mutation == "tag": value["command_tag"] = "UPDATE 2"
            self.assertEqual(validate_concrete(case, observation, response)["status"], "failed", mutation)

    def test_response_identity_is_total(self):
        """Reordered responses are correlated; duplicates, omissions and unexpected IDs fail."""
        requests = [{"id": "a"}, {"id": "b"}]
        self.assertEqual(set(index_responses(requests, list(reversed(requests)))), {"a", "b"})
        for responses in ([{"id": "a"}], [{"id": "a"}] * 2, [{"id": "a"}, {"id": "c"}]):
            with self.assertRaises(ValueError): index_responses(requests, responses)

    def test_oracle_shape_and_hash_are_checked(self):
        """Extra input cells and altered content-addressed evidence cannot disappear in zip."""
        with self.assertRaises(ValueError): typed_rows([["1", "2"]], ["int32"])
        with self.assertRaises(ValueError): typed_rows([[]], ["int32"])
        with self.assertRaises(ValueError): verified_payload({"wrong": {"rows": []}}, "wrong")

    def test_witness_stays_inside_declared_domain(self):
        """An actual PG violation outside the bounded search is not a valid solver witness."""
        request = cases()[0]["request"]
        witness = {"complete": True, "database": {"public.r": [[{"type": "int32", "value": 0}]]}}
        self.assertTrue(witness_admitted(request, witness))
        for mutation in ("value", "bound", "table", "shape", "type", "boolean_value"):
            changed = deepcopy(witness)
            if mutation == "value": changed["database"]["public.r"][0][0]["value"] = 99
            if mutation == "bound": changed["database"]["public.r"] *= 3
            if mutation == "table": changed["database"]["public.unknown"] = []
            if mutation == "shape": changed["database"]["public.r"][0] = []
            if mutation == "type": changed["database"]["public.r"][0][0] = {"type": "null", "value": "text"}
            if mutation == "boolean_value": changed["database"]["public.r"][0][0]["value"] = False
            self.assertFalse(witness_admitted(request, changed), mutation)

    def test_order_contract_ignores_nested_and_quoted_text(self):
        """Only the outer query ORDER BY establishes the observation's sequence contract."""
        for sql in ("SELECT x FROM r ORDER /* nested /* x */ y */ BY x", "(SELECT x FROM r ORDER BY x)",
                    "WITH q AS (SELECT x FROM r) SELECT x FROM q ORDER BY x", "SELECT '\\' ORDER BY 1"):
            self.assertTrue(top_level_ordered(sql), sql)
        for sql in ("SELECT x FROM (SELECT x FROM r ORDER BY x) q", "SELECT 'ORDER BY', $$ORDER BY$$", 
                    "WITH q AS (SELECT x FROM r ORDER BY x) SELECT x FROM q", "SELECT 1 /* ORDER BY */",
                    "INSERT INTO r SELECT x FROM s ORDER BY x RETURNING x", "CREATE VIEW q AS SELECT x FROM r ORDER BY x",
                    "WITH q AS (SELECT x FROM s) INSERT INTO r SELECT x FROM q ORDER BY x RETURNING x"):
            self.assertFalse(top_level_ordered(sql), sql)


if __name__ == "__main__":
    unittest.main()

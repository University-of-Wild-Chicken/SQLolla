//! Aggregate regressions cover empty input, duplicate classes, scope and refusal boundaries.
use serde_json::{json, Value};
use sqlolla_core::*;

/// Build a pinned v2 query envelope independently of production binding.
fn report(body: Value, orders: Vec<Value>) -> Value {
    json!({"protocol":"sqlolla.parser.v2","parser_commit":"ad15c585155ff7fc2c5fc4c9746905e4959ff5eb",
        "assurance":"syntax-only","semantic_preservation":false,"result":{"parse_status":"parsed",
        "export_status":"exported","ast":{"tag":"query","body":body,"ctes":[],"recursive":false,
        "order_by":orders,"limit":null,"offset":null,"offset_first":false,"locking":[]}}})
}

/// Nullable integer keys and values permit independent NULL-group and NULL-argument tests.
fn catalog() -> Catalog {
    serde_json::from_value(json!({"search_path":["public"],"tables":{"t":{
        "schema":"public","name":"t","columns":[
        {"name":"k","ty":"int32","nullable":true},{"name":"v","ty":"int32","nullable":true}]}}}))
    .unwrap()
}

/// Construct unresolved source column syntax without implementing a SQL parser in tests.
fn column(name: &str) -> Value {
    json!({"tag":"column","name":name,"qualifier":null,"schema":null})
}

/// Positive integer literals remain exact decimal magnitude strings.
fn integer(value: u32) -> Value {
    json!({"tag":"integer","magnitude":value.to_string()})
}

/// Build one aggregate with explicit duplicate policy.
fn aggregate(function: &str, argument: Value, distinct: bool) -> Value {
    json!({"tag":"aggregate","fn":function,"arg":argument,"distinct":distinct})
}

/// Construct a complete SELECT body with all grouping and distinct fields retained.
fn select(values: Vec<Value>, groups: Vec<Value>) -> Value {
    let items = values
        .into_iter()
        .map(|value| json!({"tag":"expression","value":value,"alias":null}))
        .collect::<Vec<_>>();
    json!({"tag":"select","items":items,"source":{"tag":"table","name":"t","alias":null,
        "columns":[],"schema":null},"where":null,"distinct":false,"group_by":groups,
        "having":null,"distinct_on":[]})
}

/// Bind and evaluate one fixture while retaining SQL errors separately from refusals.
fn run(body: Value, orders: Vec<Value>, rows: Bag) -> Result<OutcomeSet> {
    let plan = bind_statement(&report(body, orders), &catalog(), &Parameters::new())?;
    evaluate_statement(
        &plan,
        &Database::from([("t".into(), rows)]),
        &ExecutionContext::default(),
    )
}

/// Require one successful complete outcome for deterministic aggregate fixtures.
fn success(result: Result<OutcomeSet>) -> StatementSuccess {
    let mut outcomes = result.unwrap().outcomes;
    assert_eq!(outcomes.len(), 1);
    match outcomes.pop().unwrap() {
        StatementOutcome::Succeeded(value) => value,
        other => panic!("Unexpected outcome: {other:?}"),
    }
}

/// Extract the explicit SQLSTATE rather than accepting an unsupported binder error.
fn sqlstate(result: Result<OutcomeSet>) -> String {
    match result.unwrap().outcomes.into_iter().next().unwrap() {
        StatementOutcome::SqlError(failure) => failure.sqlstate,
        other => panic!("Expected SQL error: {other:?}"),
    }
}

/// Global aggregation over no input emits one row; COUNT is zero and SUM is NULL.
#[test]
fn empty_global_group_and_keyed_empty_differ() {
    let body = select(
        vec![
            json!({"tag":"count_all"}),
            aggregate("sum", column("v"), false),
        ],
        vec![],
    );
    assert_eq!(
        success(run(body, vec![], vec![])).rows,
        vec![vec![Cell::Int64(0), Cell::Null(SqlType::Int64)]]
    );
    let keyed = select(
        vec![column("k"), json!({"tag":"count_all"})],
        vec![column("k")],
    );
    assert!(success(run(keyed, vec![], vec![])).rows.is_empty());
}

/// COUNT(expression) excludes NULL while ordinary SUM retains duplicate occurrences.
#[test]
fn count_sum_and_average_keep_duplicate_occurrences() {
    let calls =
        ["count", "sum", "avg", "min", "max"].map(|name| aggregate(name, column("v"), false));
    let rows = vec![
        vec![Cell::Int32(0), Cell::Int32(2)],
        vec![Cell::Int32(0), Cell::Int32(2)],
        vec![Cell::Int32(0), Cell::Null(SqlType::Int32)],
    ];
    let row = success(run(select(calls.to_vec(), vec![]), vec![], rows))
        .rows
        .remove(0);
    assert_eq!(&row[..2], &[Cell::Int64(2), Cell::Int64(4)]);
    assert_eq!(scalar::output(&row[2]).unwrap(), "2.0000000000000000");
    assert_eq!(&row[3..], &[Cell::Int32(2), Cell::Int32(2)]);
}

/// NULL grouping identity is total and DISTINCT argument equality ignores duplicate occurrences.
#[test]
fn null_keys_form_one_group_and_distinct_sum_deduplicates() {
    let body = select(
        vec![column("k"), aggregate("sum", column("v"), true)],
        vec![column("k")],
    );
    let row = vec![Cell::Null(SqlType::Int32), Cell::Int32(3)];
    let result = success(run(body, vec![], vec![row.clone(), row]));
    assert_eq!(
        result.rows,
        vec![vec![Cell::Null(SqlType::Int32), Cell::Int64(3)]]
    );
}

/// HAVING runs after grouping and ORDER BY may resolve a selected aggregate alias.
#[test]
fn having_and_order_alias_use_aggregate_positions() {
    let count = json!({"tag":"count_all"});
    let mut body = select(vec![column("k"), count.clone()], vec![column("k")]);
    body["items"][1]["alias"] = json!("n");
    body["having"] = json!({"tag":"binary","op":"gt","left":count,"right":integer(1)});
    let order = json!({"expression":column("n"),"direction":"desc","nulls":null});
    let rows = vec![
        vec![Cell::Int32(1), Cell::Int32(0)],
        vec![Cell::Int32(2), Cell::Int32(0)],
        vec![Cell::Int32(1), Cell::Int32(0)],
    ];
    let result = success(run(body, vec![order], rows));
    assert_eq!(result.rows, vec![vec![Cell::Int32(1), Cell::Int64(2)]]);
    assert_eq!(result.ordering, ResultOrdering::Ordered);
}

/// Runtime FILTER suppresses a row-dependent division error before aggregate argument evaluation.
#[test]
fn false_filter_skips_argument_error() {
    let division = json!({"tag":"binary","op":"divide","left":integer(1),"right":column("v")});
    let filter = json!({"tag":"filtered_aggregate","aggregate":aggregate("sum",division,false),
        "predicate":{"tag":"binary","op":"ne","left":column("v"),"right":integer(0)}});
    let rows = vec![
        vec![Cell::Int32(1), Cell::Int32(0)],
        vec![Cell::Int32(1), Cell::Int32(1)],
    ];
    assert_eq!(
        success(run(select(vec![filter], vec![]), vec![], rows)).rows,
        vec![vec![Cell::Int64(1)]]
    );
}

/// Repeated aggregate references share a plan slot rather than varying independently.
#[test]
fn identical_calls_share_one_slot() {
    let call = aggregate("sum", column("v"), false);
    let body = select(vec![call.clone(), call], vec![]);
    let plan = bind_statement(&report(body, vec![]), &catalog(), &Parameters::new()).unwrap();
    let StatementPlan::Query(Plan {
        node: RelNode::Project { input, .. },
        ..
    }) = plan
    else {
        panic!("Expected projection");
    };
    let RelNode::Aggregate { aggregates, .. } = input.node else {
        panic!("Expected aggregate");
    };
    assert_eq!(aggregates.len(), 1);
}

/// Grouped whole expressions can be selected, but their ungrouped source components cannot.
#[test]
fn group_expression_does_not_expose_its_components() {
    let expression = json!({"tag":"binary","op":"add","left":column("k"),"right":integer(1)});
    let accepted = select(vec![expression.clone()], vec![expression.clone()]);
    let rows = vec![vec![Cell::Int32(2), Cell::Int32(9)]];
    assert_eq!(
        success(run(accepted, vec![], rows.clone())).rows,
        vec![vec![Cell::Int32(3)]]
    );
    assert_eq!(
        sqlstate(run(
            select(vec![column("k")], vec![expression]),
            vec![],
            rows
        )),
        "42803"
    );
}

/// Invalid aggregate nesting is a modeled PostgreSQL binding error.
#[test]
fn nested_aggregate_and_ungrouped_column_are_sql_errors() {
    let nested = aggregate("sum", aggregate("count", column("v"), false), false);
    assert_eq!(
        sqlstate(run(select(vec![nested], vec![]), vec![], vec![])),
        "42803"
    );
    let ungrouped = select(vec![column("k"), json!({"tag":"count_all"})], vec![]);
    assert_eq!(sqlstate(run(ungrouped, vec![], vec![])), "42803");
}

/// An aggregate occurring only in ORDER BY still creates a single global group.
#[test]
fn ordering_only_aggregate_creates_global_group() {
    let body = select(vec![integer(7)], vec![]);
    let order = json!({"expression":{"tag":"count_all"},"direction":"asc","nulls":null});
    assert_eq!(
        success(run(body, vec![order], vec![])).rows,
        vec![vec![Cell::Int32(7)]]
    );
}

/// PostgreSQL permits GROUP BY aliases and ordinals while preserving original output labels.
#[test]
fn grouping_aliases_and_ordinals_resolve_to_select_expressions() {
    for group in [column("label"), integer(1)] {
        let mut body = select(vec![column("k"), json!({"tag":"count_all"})], vec![group]);
        body["items"][0]["alias"] = json!("label");
        let row = vec![Cell::Int32(2), Cell::Int32(9)];
        assert_eq!(
            success(run(body, vec![], vec![row.clone(), row])).rows,
            vec![vec![Cell::Int32(2), Cell::Int64(2)]]
        );
    }
}

/// HAVING without explicit aggregate syntax still forms a global group over empty input.
#[test]
fn having_constant_creates_global_group() {
    let mut body = select(vec![integer(7)], vec![]);
    body["having"] = json!({"tag":"boolean","value":true});
    assert_eq!(
        success(run(body, vec![], vec![])).rows,
        vec![vec![Cell::Int32(7)]]
    );
}

/// Mixed numeric display scales are refused when the chosen grouping representative is visible.
#[test]
fn ambiguous_numeric_representatives_fail_closed() {
    let numeric = SqlType::Numeric {
        precision: None,
        scale: None,
    };
    let mut metadata = catalog();
    metadata.tables.get_mut("t").unwrap().columns[0].ty = numeric.clone();
    let body = select(
        vec![column("k"), json!({"tag":"count_all"})],
        vec![column("k")],
    );
    let plan = bind_statement(&report(body, vec![]), &metadata, &Parameters::new()).unwrap();
    let rows =
        ["1.0", "1.00"].map(|text| vec![scalar::input(&numeric, text).unwrap(), Cell::Int32(1)]);
    let result = evaluate_statement(
        &plan,
        &Database::from([("t".into(), rows.to_vec())]),
        &ExecutionContext::default(),
    );
    assert_eq!(
        result.unwrap_err().code,
        "unsupported.aggregate_representation"
    );
}

/// PostgreSQL selects the text MIN overload for unknown NULL but rejects ambiguous SUM.
#[test]
fn unknown_min_resolves_text_while_unknown_sum_is_ambiguous() {
    let null = json!({"tag":"sql_null"});
    let minimum = select(vec![aggregate("min", null.clone(), false)], vec![]);
    assert_eq!(
        success(run(minimum, vec![], vec![])).rows,
        vec![vec![Cell::Null(SqlType::Text)]]
    );
    let sum = select(vec![aggregate("sum", null, false)], vec![]);
    assert_eq!(sqlstate(run(sum, vec![], vec![])), "42725");
}

/// Identical repeated output aliases are unambiguous; different expressions are not.
#[test]
fn repeated_group_alias_preserves_expression_identity() {
    let mut body = select(vec![column("k"), column("k")], vec![column("z")]);
    body["items"][0]["alias"] = json!("z");
    body["items"][1]["alias"] = json!("z");
    let rows = vec![vec![Cell::Int32(3), Cell::Int32(4)]];
    assert_eq!(
        success(run(body.clone(), vec![], rows)).rows,
        vec![vec![Cell::Int32(3), Cell::Int32(3)]]
    );
    body["items"][1]["value"] = column("v");
    assert_eq!(sqlstate(run(body, vec![], vec![])), "42702");
}

/// GROUP BY ordinal positions refer to wildcard-expanded columns rather than syntax items.
#[test]
fn wildcard_grouping_ordinals_use_expanded_positions() {
    let mut body = select(
        vec![json!({"tag":"count_all"})],
        vec![integer(1), integer(2)],
    );
    body["items"]
        .as_array_mut()
        .unwrap()
        .insert(0, json!({"tag":"wildcard","qualifier":null,"schema":null}));
    let row = vec![Cell::Int32(2), Cell::Int32(4)];
    assert_eq!(
        success(run(body, vec![], vec![row.clone(), row])).rows,
        vec![vec![Cell::Int32(2), Cell::Int32(4), Cell::Int64(2)]]
    );
}

/// PostgreSQL distinguishes ordinal constants from unary-plus and cast expressions.
#[test]
fn grouping_constant_sqlstates_are_not_ordinary_constant_groups() {
    let cases = [
        (integer(0), "42P10"),
        (
            json!({"tag":"unary","op":"negative","arg":integer(1)}),
            "42P10",
        ),
        (json!({"tag":"text","value":"a"}), "42601"),
        (json!({"tag":"sql_null"}), "42601"),
        (json!({"tag":"boolean","value":true}), "42601"),
        (
            json!({"tag":"unary","op":"positive","arg":integer(1)}),
            "42803",
        ),
    ];
    for (key, expected) in cases {
        assert_eq!(
            sqlstate(run(select(vec![column("k")], vec![key]), vec![], vec![])),
            expected
        );
    }
}

/// PostgreSQL parses an overflowing positive token as numeric even beneath unary minus.
#[test]
fn huge_grouping_ordinals_are_noninteger_constants() {
    for key in [
        integer(2147483648),
        json!({"tag":"unary","op":"negative","arg":integer(2147483648)}),
    ] {
        assert_eq!(
            sqlstate(run(select(vec![column("k")], vec![key]), vec![], vec![])),
            "42601"
        );
    }
}

/// HAVING may compare its aggregate with an independently scoped aggregate subquery.
#[test]
fn having_accepts_independent_aggregate_subquery() {
    let inner = report(
        select(vec![aggregate("sum", column("k"), false)], vec![]),
        vec![],
    )["result"]["ast"]
        .clone();
    let mut body = select(
        vec![column("k"), aggregate("sum", column("v"), false)],
        vec![column("k")],
    );
    body["having"] = json!({"tag":"binary","op":"gt","left":aggregate("sum",column("v"),false),
        "right":{"tag":"scalar_subquery","query":inner}});
    let input = vec![
        vec![Cell::Int32(1), Cell::Int32(5)],
        vec![Cell::Int32(2), Cell::Int32(1)],
    ];
    assert_eq!(
        success(run(body, vec![], input)).rows,
        vec![vec![Cell::Int32(1), Cell::Int64(5)]]
    );
}

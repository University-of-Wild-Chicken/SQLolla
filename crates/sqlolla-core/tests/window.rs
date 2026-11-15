//! Ranking windows preserve peer choices, repeated references and hidden projection positions.
use serde_json::{json, Value};
use sqlolla_core::*;

/// Build a complete syntax-only report for independent binder regressions.
fn report(items: Vec<Value>, orders: Vec<Value>) -> Value {
    json!({"protocol":"sqlolla.parser.v2","parser_commit":"ad15c585155ff7fc2c5fc4c9746905e4959ff5eb","assurance":"syntax-only","semantic_preservation":false,
        "result":{"parse_status":"parsed","export_status":"exported","ast":{"tag":"query","ctes":[],"recursive":false,"order_by":orders,"limit":null,"offset":null,"offset_first":false,"locking":[],
        "body":{"tag":"select","distinct":false,"distinct_on":[],"group_by":[],"having":null,"where":null,"items":items,"source":{"tag":"table","name":"t","schema":null,"alias":null,"columns":[]}}}}})
}

/// A single physical input position isolates ranking from unrelated SQL constructors.
fn catalog() -> Catalog {
    serde_json::from_value(json!({"search_path":["public"],"tables":{"t":{"schema":"public","name":"t","columns":[{"name":"x","ty":"int32","nullable":true}]}}})).unwrap()
}

/// Scalar input references retain their original unqualified name.
fn column() -> Value {
    json!({"tag":"column","name":"x","qualifier":null,"schema":null})
}

/// Default ascending order places NULL peers last.
fn order(expression: Value) -> Value {
    json!({"expression":expression,"direction":"asc","nulls":null})
}

/// Each ranking call preserves the complete explicit window specification.
fn call(name: &str, orders: Vec<Value>) -> Value {
    json!({"tag":"window_call","function":{"tag":"call","name":[name],"args":[]},"window":{"name":null,"frame":null,"partition_by":[],"order_by":orders}})
}

/// Keep output aliases absent so implicit builtin labels are exercised.
fn item(value: Value) -> Value {
    json!({"tag":"expression","value":value,"alias":null})
}

/// Evaluate every admitted result while making occurrence capacity explicit.
fn rows(report: Value, values: Vec<Cell>, max_outcomes: usize) -> Vec<Bag> {
    let plan = bind_statement(&report, &catalog(), &Parameters::new()).unwrap();
    let database = [(
        "t".into(),
        values.into_iter().map(|value| vec![value]).collect(),
    )]
    .into();
    let context = ExecutionContext {
        max_outcomes,
        ..Default::default()
    };
    evaluate_statement(&plan, &database, &context)
        .unwrap()
        .outcomes
        .into_iter()
        .map(|outcome| match outcome {
            StatementOutcome::Succeeded(value) => value.rows,
            other => panic!("{other:?}"),
        })
        .collect()
}

/// Distinct tied rows can receive either numbering, but repeated row_number references remain equal.
#[test]
fn tied_numbering_keeps_repeated_reference_correlated() {
    let rank = call("row_number", vec![]);
    let outcomes = rows(
        report(vec![item(column()), item(rank.clone()), item(rank)], vec![]),
        vec![Cell::Int32(1), Cell::Int32(2)],
        8,
    );
    assert_eq!(outcomes.len(), 2);
    assert!(outcomes.iter().flatten().all(|row| row[1] == row[2]));
    assert!(outcomes.iter().any(|rows| rows[0][1] == Cell::Int64(1)));
    assert!(outcomes.iter().any(|rows| rows[0][1] == Cell::Int64(2)));
}

/// Rank-only peers produce one complete bag even when their row permutations exceed the outcome cap.
#[test]
fn rank_only_avoids_irrelevant_factorial_orders() {
    let outcomes = rows(
        report(vec![item(column()), item(call("rank", vec![]))], vec![]),
        (1..=8).map(Cell::Int32).collect(),
        1,
    );
    assert_eq!(outcomes.len(), 1);
    assert!(outcomes[0].iter().all(|row| row[1] == Cell::Int64(1)));
}

/// RANK counts preceding occurrences while DENSE_RANK counts peer classes.
#[test]
fn ranking_peer_gaps_are_distinct() {
    let orders = vec![order(column())];
    let outcomes = rows(
        report(
            vec![
                item(column()),
                item(call("rank", orders.clone())),
                item(call("dense_rank", orders)),
            ],
            vec![],
        ),
        vec![Cell::Int32(1), Cell::Int32(1), Cell::Int32(3)],
        1,
    );
    assert_eq!(
        outcomes[0][2],
        vec![Cell::Int32(3), Cell::Int64(3), Cell::Int64(2)]
    );
}

/// Wildcards expose original positions only and never append compiler-private ranking slots twice.
#[test]
fn wildcard_hides_ranking_slots() {
    let ast = report(
        vec![
            json!({"tag":"wildcard","qualifier":null,"schema":null}),
            item(call("row_number", vec![])),
        ],
        vec![],
    );
    let plan = bind_statement(&ast, &catalog(), &Parameters::new()).unwrap();
    let StatementPlan::Query(plan) = plan else {
        panic!("query expected")
    };
    assert_eq!(
        plan.schema
            .iter()
            .map(|column| column.name.as_str())
            .collect::<Vec<_>>(),
        vec!["x", "row_number"]
    );
}

/// Different window sorts are refused instead of inventing independent ranking correlations.
#[test]
fn different_window_specifications_fail_closed() {
    let ast = report(
        vec![
            item(call("rank", vec![])),
            item(call("rank", vec![order(column())])),
        ],
        vec![],
    );
    assert_eq!(
        bind_statement(&ast, &catalog(), &Parameters::new())
            .unwrap_err()
            .code,
        "unsupported.window_specifications"
    );
}

/// An ORDER-only window may rank input rows without exposing its hidden column in SELECT output.
#[test]
fn order_only_window_is_bound_before_projection() {
    let ast = report(
        vec![item(column())],
        vec![order(call("row_number", vec![order(column())]))],
    );
    let outcomes = rows(ast, vec![Cell::Int32(2), Cell::Int32(1)], 8);
    assert_eq!(
        outcomes,
        vec![vec![vec![Cell::Int32(1)], vec![Cell::Int32(2)]]]
    );
}

/// A hidden builtin label cannot make an ordinary input column with that name ambiguous.
#[test]
fn hidden_slot_does_not_shadow_source_column() {
    let mut catalog = catalog();
    catalog.tables.get_mut("t").unwrap().columns[0].name = "rank".into();
    let mut value = column();
    value["name"] = json!("rank");
    let ast = report(vec![item(value), item(call("rank", vec![]))], vec![]);
    assert!(matches!(
        bind_statement(&ast, &catalog, &Parameters::new()).unwrap(),
        StatementPlan::Query(_)
    ));
}

/// Constant source values reach window-key planning even when WHERE removes every row.
#[test]
fn singleton_window_key_failure_survives_false_filter() {
    let division = json!({"tag":"binary","op":"divide","left":{"tag":"integer","magnitude":"1"},"right":column()});
    let mut ast = report(
        vec![item(call("row_number", vec![order(division)]))],
        vec![],
    );
    let mut query = ast["result"]["ast"].clone();
    query["body"] = json!({"tag":"values","rows":[[{"tag":"integer","magnitude":"0"}]]});
    ast["result"]["ast"]["body"]["source"] =
        json!({"tag":"derived","query":query,"alias":"t","columns":["x"],"lateral":false});
    ast["result"]["ast"]["body"]["where"] = json!({"tag":"boolean","value":false});
    let plan = bind_statement(&ast, &catalog(), &Parameters::new()).unwrap();
    assert!(
        matches!(plan,StatementPlan::Rejected{failure} if failure.sqlstate=="22012" && failure.phase==FailurePhase::Planning)
    );
}

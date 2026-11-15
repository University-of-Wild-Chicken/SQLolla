//! Conflict tests retain occurrence identity, proposed-row constraints and RETURNING counts.
use serde_json::{json, Value};
use sqlolla_core::*;

/// A complete two-column target permits conflicts on the nonnullable primary key.
fn setup() -> (Catalog, ExecutionContext) {
    let catalog = serde_json::from_value(json!({"search_path":["public"],"tables":{"t":{
        "schema":"public","name":"t","columns":[{"name":"k","ty":"int32","nullable":false},
        {"name":"v","ty":"int32","nullable":false}]}}}))
    .unwrap();
    let metadata = serde_json::from_value(json!({"complete":true,
        "primary_key":{"name":"t_pkey","columns":[0]}}))
    .unwrap();
    let context = ExecutionContext {
        table_metadata: [("t".into(), metadata)].into(),
        ..Default::default()
    };
    (catalog, context)
}

/// The Advanced parser's target omits the CST-only optional inference predicate field.
fn statement(rows: &[(i32, i32)], action: Value) -> Value {
    let rows = rows
        .iter()
        .map(|(k, v)| vec![integer(*k), integer(*v)])
        .collect::<Vec<_>>();
    json!({"protocol":"sqlolla.parser.v2","parser_commit":"ad15c585155ff7fc2c5fc4c9746905e4959ff5eb",
        "assurance":"syntax-only","semantic_preservation":false,"result":{"parse_status":"parsed","export_status":"exported",
        "ast":{"tag":"insert","table":"t","alias":null,"columns":["k","v"],"source":{"tag":"values","rows":rows},
        "on_conflict":{"target":{"tag":"columns","names":["k"]},"action":action},"returning":[{"tag":"wildcard","qualifier":null,"schema":null}]}}})
}

/// Nonnegative fixture values avoid unrelated unary-literal syntax.
fn integer(value: i32) -> Value {
    json!({"tag":"integer","magnitude":value.to_string()})
}

/// UPDATE values read the separate EXCLUDED scope and an optional explicit predicate.
fn update(selected: bool) -> Value {
    json!({"tag":"update","assignments":[{"column":"v","value":{
        "tag":"column","name":"v","qualifier":"excluded","schema":null}}],
        "where":{"tag":"boolean","value":selected}})
}

/// Run the normal binder and pure evaluator with an unchanged caller-owned prestate.
fn execute(rows: &[(i32, i32)], before: Bag, action: Value) -> StatementOutcome {
    let (catalog, context) = setup();
    let plan = bind_statement(&statement(rows, action), &catalog, &Parameters::new()).unwrap();
    let outcomes = evaluate_statement(&plan, &[("t".into(), before)].into(), &context).unwrap();
    assert_eq!(outcomes.outcomes.len(), 1);
    outcomes.outcomes.into_iter().next().unwrap()
}

/// DO NOTHING preserves the first literal VALUES proposal and does not return skipped rows.
#[test]
fn nothing_preserves_literal_source_order() {
    let StatementOutcome::Succeeded(result) =
        execute(&[(1, 9), (1, 2)], vec![], json!({"tag":"nothing"}))
    else {
        panic!()
    };
    assert_eq!(result.affected, 1);
    assert_eq!(
        result.database["t"],
        vec![vec![Cell::Int32(1), Cell::Int32(9)]]
    );
    assert_eq!(result.rows, result.database["t"]);
}

/// An updated row returns its new values and counts as affected even when replacing a conflict.
#[test]
fn update_reads_excluded_and_returns_new_row() {
    let before = vec![vec![Cell::Int32(1), Cell::Int32(0)]];
    let StatementOutcome::Succeeded(result) = execute(&[(1, 7)], before, update(true)) else {
        panic!()
    };
    assert_eq!(result.command_tag, "INSERT 0 1");
    assert_eq!(result.rows, vec![vec![Cell::Int32(1), Cell::Int32(7)]]);
}

/// PostgreSQL detects a same-command inserted tuple before evaluating conflict WHERE false.
#[test]
fn same_command_conflict_rejects_before_false_predicate() {
    let StatementOutcome::SqlError(failure) = execute(&[(1, 1), (1, 2)], vec![], update(false))
    else {
        panic!()
    };
    assert_eq!(failure.sqlstate, "21000");
    assert!(failure.exposed_rows.is_empty());
}

/// Repeated skipped conflicts on an untouched old tuple do not invent cardinality failures.
#[test]
fn skipped_old_tuple_may_conflict_repeatedly() {
    let before = vec![vec![Cell::Int32(1), Cell::Int32(0)]];
    let StatementOutcome::Succeeded(result) =
        execute(&[(1, 1), (1, 2)], before.clone(), update(false))
    else {
        panic!()
    };
    assert_eq!(result.affected, 0);
    assert_eq!(result.database["t"], before);
    assert!(result.rows.is_empty());
}

/// A second selected update to the same old occurrence rejects atomically.
#[test]
fn repeated_selected_update_rejects_atomically() {
    let before = vec![vec![Cell::Int32(1), Cell::Int32(0)]];
    let StatementOutcome::SqlError(failure) = execute(&[(1, 1), (1, 2)], before, update(true))
    else {
        panic!()
    };
    assert_eq!(failure.sqlstate, "21000");
    assert!(failure.exposed_rows.is_empty());
}

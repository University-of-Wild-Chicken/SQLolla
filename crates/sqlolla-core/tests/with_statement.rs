//! Same-command CTE tests distinguish original snapshots, RETURNING relations and unused writes.
use serde_json::{json, Value};
use sqlolla_core::*;

/// Two independent tables permit explicit write-frame and joined-mutation fixtures.
fn setup() -> (Catalog, Database, ExecutionContext) {
    let table = json!({"schema":"public","name":"t","columns":[{"name":"v","ty":"int32","nullable":false}]});
    let mut other = table.clone();
    other["name"] = json!("s");
    let catalog =
        serde_json::from_value(json!({"search_path":["public"],"tables":{"t":table,"s":other}}))
            .unwrap();
    let db = [
        ("t".into(), vec![vec![Cell::Int32(0)]]),
        ("s".into(), vec![vec![Cell::Int32(7)]]),
    ]
    .into();
    let metadata = TableSemantics {
        complete: true,
        ..Default::default()
    };
    let context = ExecutionContext {
        table_metadata: [("t".into(), metadata.clone()), ("s".into(), metadata)].into(),
        ..Default::default()
    };
    (catalog, db, context)
}

/// Wrap one complete deterministic extended AST.
fn report(ast: Value) -> Value {
    json!({"protocol":"sqlolla.parser.v2","parser_commit":"ad15c585155ff7fc2c5fc4c9746905e4959ff5eb",
    "assurance":"syntax-only","semantic_preservation":false,"result":{"parse_status":"parsed","export_status":"exported","ast":ast}})
}

/// Produce a physical or virtual table reference with its exact optional AST fields.
fn table(name: &str) -> Value {
    json!({"tag":"table","name":name,"schema":null,"alias":null,"columns":[]})
}

/// Query wrappers preserve all modifiers rather than omitting fields during test construction.
fn query(source: Value, expression: Value) -> Value {
    json!({"tag":"query","body":{"tag":"select","items":[{"tag":"expression","value":expression,"alias":null}],
        "source":source,"where":null,"distinct":false,"group_by":[],"having":null,"distinct_on":[]},
        "ctes":[],"recursive":false,"order_by":[],"limit":null,"offset":null,"offset_first":false,"locking":[]})
}

/// Resolve column references through the normal binder.
fn column(qualifier: Option<&str>) -> Value {
    json!({"tag":"column","name":"v","qualifier":qualifier,"schema":null})
}

/// A write CTE exposes its new value but may not replace later base-table reads.
fn update() -> Value {
    json!({"tag":"update","table":"t","alias":null,"assignments":[{"column":"v","value":{"tag":"integer","magnitude":"9"}}],
        "from":null,"where":null,"returning":[{"tag":"wildcard","qualifier":null,"schema":null}]})
}

/// One nonrecursive binding supplies a reviewable same-command visibility boundary.
fn with(statement: Value, body: Value) -> Value {
    json!({"tag":"with_statement","recursive":false,"ctes":[{"name":"u","columns":[],"materialized":null,"statement":statement}],"body":body})
}

/// Execute through the public APIs and require a single admitted behavior.
fn execute(ast: Value) -> StatementSuccess {
    let (catalog, db, context) = setup();
    let plan = bind_statement(&report(ast), &catalog, &Parameters::new()).unwrap();
    let values = evaluate_statement(&plan, &db, &context).unwrap();
    assert_eq!(values.outcomes.len(), 1);
    let StatementOutcome::Succeeded(value) = values.outcomes.into_iter().next().unwrap() else {
        panic!()
    };
    value
}

/// Base scans see the original row even though the unreferenced write CTE executes fully.
#[test]
fn base_reads_retain_original_snapshot_and_unused_writes_execute() {
    let value = execute(with(update(), query(table("t"), column(None))));
    assert_eq!(value.rows, vec![vec![Cell::Int32(0)]]);
    assert_eq!(value.database["t"], vec![vec![Cell::Int32(9)]]);
    assert_eq!(value.database["s"], vec![vec![Cell::Int32(7)]]);
    assert_eq!(value.database.len(), 2);
}

/// RETURNING is the explicit channel by which another substatement observes changed values.
#[test]
fn returning_relation_exposes_new_value_without_publishing_virtual_table() {
    let value = execute(with(update(), query(table("u"), column(None))));
    assert_eq!(value.rows, vec![vec![Cell::Int32(9)]]);
    assert_eq!(value.database.len(), 2);
}

/// An unused read CTE is typechecked but its unreachable constant evaluation does not run.
#[test]
fn unused_read_does_not_force_constant_failure() {
    let division = json!({"tag":"binary","op":"divide","left":{"tag":"integer","magnitude":"1"},"right":{"tag":"integer","magnitude":"0"}});
    let value = execute(with(
        query(Value::Null, division),
        query(table("t"), column(None)),
    ));
    assert_eq!(value.rows, vec![vec![Cell::Int32(0)]]);
}

/// UPDATE FROM assignments resolve the source scope while returning the new target prefix.
#[test]
fn joined_update_reads_source_and_frames_untargeted_table() {
    let mut ast = update();
    ast["from"] = table("s");
    ast["assignments"][0]["value"] = column(Some("s"));
    ast["returning"] = json!([{"tag":"wildcard","qualifier":"t","schema":null}]);
    let value = execute(ast);
    assert_eq!(value.database["t"], vec![vec![Cell::Int32(7)]]);
    assert_eq!(value.rows, vec![vec![Cell::Int32(7)]]);
    assert_eq!(value.affected, 1);
}

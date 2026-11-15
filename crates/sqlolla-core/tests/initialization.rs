//! Compiler-owned relation opening survives dead query work without creating tuple demand.
use serde_json::{json, Value};
use sqlolla_core::*;

/// Match the independently observed physical one-row table and an explicit complete lock snapshot.
fn setup() -> (Catalog, Database, ExecutionContext) {
    let catalog = serde_json::from_value(json!({"search_path":["public"],"tables":{"t":{
        "schema":"public","name":"t","columns":[{"name":"k","ty":"int32","nullable":false}]}}}))
    .unwrap();
    let context = ExecutionContext {
        locks: Some(LockContext {
            complete: true,
            transaction: "self".into(),
            held_rows: vec![],
            row_share_conflicts: Default::default(),
        }),
        ..Default::default()
    };
    (
        catalog,
        [("t".into(), vec![vec![Cell::Int32(1)]])].into(),
        context,
    )
}

/// Parser query nodes preserve locking separately from their SELECT body.
fn query(source: Value, value: Value, locked: bool) -> Value {
    let locking = if locked {
        json!([{"strength":"update","targets":[],"wait_policy":"block"}])
    } else {
        json!([])
    };
    json!({"tag":"query","body":{"tag":"select","items":[{"tag":"expression","value":value,"alias":null}],
        "source":source,"where":null,"distinct":false,"group_by":[],"having":null,"distinct_on":[]},
        "ctes":[],"recursive":false,"order_by":[],"limit":null,"offset":null,"offset_first":false,"locking":locking})
}

/// Closed physical references cannot be confused with compiler-reserved CTE result identities.
fn table() -> Value {
    json!({"tag":"table","name":"t","alias":null,"columns":[],"schema":null})
}

/// Bind the pinned syntax-only protocol rather than constructing an initialization result directly.
fn bind(ast: Value) -> sqlolla_core::Result<StatementPlan> {
    bind_statement(&report(ast), &setup().0, &Parameters::new())
}

/// Produce the full parser provenance envelope for both statements and stored definitions.
fn report(ast: Value) -> Value {
    json!({"protocol":"sqlolla.parser.v2","parser_commit":"ad15c585155ff7fc2c5fc4c9746905e4959ff5eb",
        "assurance":"syntax-only","semantic_preservation":false,
        "result":{"parse_status":"parsed","export_status":"exported","ast":ast}})
}

/// An unused read CTE is bound but its divide-by-zero target is never planned or executed.
fn unused() -> Value {
    let bad = json!({"tag":"binary","op":"divide","left":{"tag":"integer","magnitude":"1"},"right":{"tag":"integer","magnitude":"0"}});
    let mut ast = query(Value::Null, json!({"tag":"integer","magnitude":"1"}), false);
    ast["ctes"] = json!([{"name":"q","columns":[],"query":query(table(),bad,true)}]);
    ast
}

/// Pruning unused rows preserves ROW SHARE and never adds a tuple lock or target-expression error.
#[test]
fn unused_locking_cte_retains_only_initialization() {
    let plan = bind(unused()).unwrap();
    let StatementPlan::Initialized { relations, .. } = &plan else {
        panic!()
    };
    assert_eq!(relations, &["t"]);
    let (_, database, context) = setup();
    let result = evaluate_statement(&plan, &database, &context).unwrap();
    let [StatementOutcome::Succeeded(value)] = result.outcomes.as_slice() else {
        panic!()
    };
    assert_eq!(value.rows, vec![vec![Cell::Int32(1)]]);
    assert_eq!(
        value.effects,
        vec![EffectEvent::TableLock {
            table: "t".into(),
            mode: "row_share".into()
        }]
    );
}

/// A fixed relation conflict cannot be converted into success just because all tuple demand vanished.
#[test]
fn unused_relation_conflict_is_still_an_unresolved_wait() {
    let (_, database, mut context) = setup();
    context
        .locks
        .as_mut()
        .unwrap()
        .row_share_conflicts
        .insert("t".into());
    assert_eq!(
        evaluate_statement(&bind(unused()).unwrap(), &database, &context)
            .unwrap_err()
            .code,
        "unsupported.lock_wait"
    );
}

/// Opening precedes constant planning failure, but atomic SQL failure releases newly acquired ownership.
#[test]
fn planning_failure_retains_opening_precedence_and_rolls_back() {
    let bad = json!({"tag":"binary","op":"divide","left":{"tag":"integer","magnitude":"1"},"right":{"tag":"integer","magnitude":"0"}});
    let plan = bind(query(table(), bad, true)).unwrap();
    let (_, database, mut context) = setup();
    let result = evaluate_statement(&plan, &database, &context).unwrap();
    let [StatementOutcome::SqlError(failure)] = result.outcomes.as_slice() else {
        panic!()
    };
    assert_eq!(failure.sqlstate, "22012");
    assert!(failure.effects.is_empty());
    context
        .locks
        .as_mut()
        .unwrap()
        .row_share_conflicts
        .insert("t".into());
    assert_eq!(
        evaluate_statement(&plan, &database, &context)
            .unwrap_err()
            .code,
        "unsupported.lock_wait"
    );
}

/// A failed bind cannot pretend to establish the same source-opening order as successful analysis.
#[test]
fn binding_error_precedence_is_explicitly_unmodeled() {
    let bad = json!({"tag":"cast","arg":{"tag":"text","value":"bad"},"type":{"name":{"tag":"scalar_type","name":"int"},"modifiers":[]}});
    let error = bind(query(table(), bad, true)).unwrap_err();
    assert_eq!(error.code, "unsupported.initialization_binding");
}

/// Constant CASE pruning removes scalar row work, while the resolved relation remains opened.
#[test]
fn dead_scalar_sublink_preserves_relation_ownership() {
    let column = json!({"tag":"column","name":"k","qualifier":null,"schema":null});
    let scalar = json!({"tag":"scalar_subquery","query":query(table(), column, true)});
    let value = json!({"tag":"case","operand":null,"branches":[{"when":{"tag":"boolean","value":false},"then":scalar}],
        "else":{"tag":"integer","magnitude":"0"}});
    let plan = bind(query(Value::Null, value, false)).unwrap();
    let (_, database, context) = setup();
    let result = evaluate_statement(&plan, &database, &context).unwrap();
    let [StatementOutcome::Succeeded(value)] = result.outcomes.as_slice() else {
        panic!()
    };
    assert_eq!(value.rows, vec![vec![Cell::Int32(0)]]);
    assert_eq!(
        value.effects,
        vec![EffectEvent::TableLock {
            table: "t".into(),
            mode: "row_share".into()
        }]
    );
}

/// An expanded view cannot silently transfer outer row marks through erased query boundaries.
#[test]
fn outer_view_lock_requires_preserved_boundaries() {
    let (mut catalog, _, _) = setup();
    let mut view: ViewDefinition = serde_json::from_value(json!({"schema":"public","name":"v",
        "columns":["k"],"search_path":["public"],"resolved_relations":[["public","t"]],
        "source_sql":"SELECT k FROM t"}))
    .unwrap();
    let column = json!({"tag":"column","name":"k","qualifier":null,"schema":null});
    view.prepared = Some(report(query(table(), column.clone(), false)));
    catalog.views.insert("v".into(), view);
    let mut source = table();
    source["name"] = json!("v");
    let error = bind_statement(
        &report(query(source, column, true)),
        &catalog,
        &Parameters::new(),
    )
    .unwrap_err();
    assert_eq!(error.code, "unsupported.lock_view_origin");
}

//! Header-returning integration keeps physical version identity out of ordinary wildcard rows.
use serde_json::{json, Value};
use sqlolla_core::*;

/// One declared integer target has complete key metadata and a pristine input header.
fn setup() -> (Catalog, Database, ExecutionContext) {
    let catalog = serde_json::from_value(json!({"search_path":["public"],"tables":{"t":{
        "schema":"public","name":"t","columns":[{"name":"k","ty":"int32","nullable":false},
        {"name":"v","ty":"int32","nullable":false}]}}}))
    .unwrap();
    let metadata = serde_json::from_value(
        json!({"complete":true,"primary_key":{"name":"t_pkey","columns":[0]}}),
    )
    .unwrap();
    let tuples = serde_json::from_value(json!({"complete":true,"exclusive_snapshot":true,
        "top_level_no_savepoints":true,"write_xid":20,"tables":{"t":[{"xmin":10,"raw_xmax":0,"state":"invalid"}]}})).unwrap();
    (
        catalog,
        [("t".into(), vec![vec![Cell::Int32(1), Cell::Int32(10)]])].into(),
        ExecutionContext {
            tuples: Some(tuples),
            table_metadata: [("t".into(), metadata)].into(),
            ..Default::default()
        },
    )
}

/// System names remain ordinary scoped parser identifiers until the target binder resolves them.
fn column(name: &str) -> Value {
    json!({"tag":"column","name":name,"qualifier":null,"schema":null})
}

/// Explicit two-value proposals exercise both new slots and existing-key conflict updates.
fn report(key: u32, selected: bool, returning: Value) -> Value {
    json!({"protocol":"sqlolla.parser.v2","parser_commit":"ad15c585155ff7fc2c5fc4c9746905e4959ff5eb",
        "assurance":"syntax-only","semantic_preservation":false,"result":{"parse_status":"parsed","export_status":"exported",
        "ast":{"tag":"insert","table":"t","alias":null,"columns":["k","v"],"source":{"tag":"values","rows":[[
        {"tag":"integer","magnitude":key.to_string()},{"tag":"integer","magnitude":"20"}]]},
        "on_conflict":{"target":{"tag":"columns","names":["k"]},"action":{"tag":"update",
        "assignments":[{"column":"v","value":{"tag":"column","name":"v","qualifier":"excluded","schema":null}}],
        "where":{"tag":"boolean","value":selected}}},"returning":returning}}})
}

/// Wildcard arity stays physical; explicit system fields retain nominal xid and Boolean types.
fn output() -> Value {
    json!([{"tag":"wildcard","qualifier":null,"schema":null},
        {"tag":"expression","value":column("xmin"),"alias":null},
        {"tag":"expression","value":{"tag":"binary","op":"ne","left":column("xmax"),
        "right":{"tag":"integer","magnitude":"0"}},"alias":"changed"}])
}

/// Standard binding and evaluation use the caller's unchanged row/header snapshots.
fn run(report: Value) -> StatementOutcome {
    let (catalog, database, context) = setup();
    let plan = bind_statement(&report, &catalog, &Parameters::new()).unwrap();
    evaluate_statement(&plan, &database, &context)
        .unwrap()
        .outcomes
        .remove(0)
}

/// The same RETURNING expression distinguishes actual fresh and locked new versions.
#[test]
fn returned_header_comes_from_version_lineage() {
    for (key, changed) in [(1, true), (2, false)] {
        let StatementOutcome::Succeeded(value) = run(report(key, true, output())) else {
            panic!()
        };
        assert_eq!(value.rows[0].len(), 4);
        assert_eq!(
            value.rows[0][2],
            scalar::input(&SqlType::Xid, "20").unwrap()
        );
        assert_eq!(value.rows[0][3], Cell::Bool(changed));
        assert!(value.database["t"].iter().all(|row| row.len() == 2));
    }
}

/// A rejected conflict WHERE retains the physical lock even though no row was returned or changed.
#[test]
fn skipped_update_still_records_header_lock() {
    let StatementOutcome::Succeeded(value) = run(report(1, false, output())) else {
        panic!()
    };
    assert_eq!(value.affected, 0);
    assert!(value.rows.is_empty());
    let [EffectEvent::TupleVersion { observation }] = value.effects.as_slice() else {
        panic!()
    };
    assert_eq!(observation.transition.after.xmin, 10);
    assert_eq!(observation.transition.after.raw_xmax, 20);
    assert_eq!(
        observation.before_row.as_ref(),
        Some(&observation.after_row)
    );
}

/// Missing context is a coverage refusal rather than a fabricated zero xmax.
#[test]
fn missing_header_snapshot_cannot_execute_system_returning() {
    let (catalog, database, mut context) = setup();
    context.tuples = None;
    let plan = bind_statement(&report(1, true, output()), &catalog, &Parameters::new()).unwrap();
    assert_eq!(
        evaluate_statement(&plan, &database, &context)
            .unwrap_err()
            .code,
        "unsupported.tuple_context"
    );
}

/// A correlated system field inside a nested SELECT cannot bypass the tuple-scope admission guard.
#[test]
fn nested_system_returning_is_explicitly_refused() {
    let query = json!({"tag":"query","body":{"tag":"select","items":[{"tag":"expression","value":column("xmax"),"alias":null}],
        "source":null,"where":null,"distinct":false,"group_by":[],"having":null,"distinct_on":[]},
        "ctes":[],"recursive":false,"order_by":[],"limit":null,"offset":null,"offset_first":false,"locking":[]});
    let output =
        json!([{"tag":"expression","value":{"tag":"scalar_subquery","query":query},"alias":null}]);
    let (catalog, _, _) = setup();
    let error = bind_statement(&report(1, true, output), &catalog, &Parameters::new()).unwrap_err();
    assert_eq!(error.code, "unsupported.returning_system_subquery");
}

/// Generic array NULL propagation precedes the missing xid element ordering at runtime.
#[test]
fn xid_array_order_rejects_nonnull_but_preserves_null() {
    let ty = SqlType::Array(Box::new(SqlType::Xid));
    for value in [Cell::Null(ty.clone()), scalar::input(&ty, "{}").unwrap()] {
        let literal = BoundExpr {
            ty: ty.clone(),
            nullable: value.is_null(),
            node: ExprNode::Literal(value.clone()),
        };
        let expression = BoundExpr {
            ty: SqlType::Bool,
            nullable: true,
            node: ExprNode::Binary {
                op: BinaryOp::Lt,
                left: Box::new(literal.clone()),
                right: Box::new(literal),
            },
        };
        let result = eval_expr(&expression, &vec![]);
        if value.is_null() {
            assert_eq!(result.unwrap(), Cell::Null(SqlType::Bool));
        } else {
            assert_eq!(result.unwrap_err().code, "sql.42883");
        }
    }
}

/// Unimplemented mixed composite overloads remain coverage errors, never invented PostgreSQL errors.
#[test]
fn mixed_nullif_is_not_mislabeled_as_sql_type_failure() {
    let output = json!([{"tag":"expression","alias":null,"value":{"tag":"null_if",
        "left":column("xmax"),"right":{"tag":"integer","magnitude":"0"}}}]);
    let (catalog, _, _) = setup();
    let error = bind_statement(&report(1, true, output), &catalog, &Parameters::new()).unwrap_err();
    assert_eq!(error.code, "unsupported.xid_composite_overload");
    assert!(error.sql.is_none());
}

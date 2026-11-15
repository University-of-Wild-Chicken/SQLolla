//! Ordered diagnostics retain physical index identity across constraint categories.
use super::*;

/// A unique constraint was created before a later primary key, reversing kind-based order.
fn metadata() -> TableSemantics {
    let key = |name: &str, column| KeyConstraint {
        name: name.into(),
        columns: vec![column],
        nulls_not_distinct: false,
        deferrable: false,
        initially_deferred: false,
    };
    TableSemantics {
        primary_key: Some(key("late_primary", 1)),
        unique: vec![key("early_unique", 0)],
        unique_enforcement_order: vec!["early_unique".into(), "late_primary".into()],
        ..Default::default()
    }
}

/// The reported name follows authentic executor order, not PK-before-UNIQUE grouping.
#[test]
fn first_physical_index_rejects() {
    let rows = vec![vec![Cell::Int32(1), Cell::Int32(1)]; 2];
    let failure = immediate(&rows, &metadata(), &[], &ExecutionContext::default()).unwrap_err();
    assert_eq!(
        failure.sql.unwrap().constraint_name.as_deref(),
        Some("early_unique")
    );
}

/// Missing order cannot justify choosing among different observable constraint names.
#[test]
fn competing_unordered_rejections_refuse() {
    let mut metadata = metadata();
    metadata.unique_enforcement_order.clear();
    let rows = vec![vec![Cell::Int32(1), Cell::Int32(1)]; 2];
    assert_eq!(
        immediate(&rows, &metadata, &[], &ExecutionContext::default())
            .unwrap_err()
            .code,
        "unsupported.constraint_order"
    );
}

/// Incomplete, duplicate and unknown identities cannot pretend to be physical traversal evidence.
#[test]
fn malformed_order_rejected() {
    for order in [
        vec!["early_unique"],
        vec!["early_unique", "early_unique"],
        vec!["early_unique", "unknown"],
    ] {
        let mut metadata = metadata();
        metadata.unique_enforcement_order = order.into_iter().map(str::to_owned).collect();
        assert!(rules(&metadata).is_err());
    }
}

/// Statement execution cannot move an initially deferred uniqueness error to an earlier boundary.
#[test]
fn deferred_timing_requires_transaction_interface() {
    let mut metadata = metadata();
    let key = metadata.primary_key.as_mut().unwrap();
    key.deferrable = true;
    key.initially_deferred = true;
    let mut context = ExecutionContext::default();
    context.table_metadata.insert("t".into(), metadata);
    assert_eq!(
        crate::constraints::require_statement_timing(&context)
            .unwrap_err()
            .code,
        "unsupported.deferred_constraints"
    );
    context
        .table_metadata
        .get_mut("t")
        .unwrap()
        .primary_key
        .as_mut()
        .unwrap()
        .initially_deferred = false;
    crate::constraints::require_statement_timing(&context).unwrap();
}

//! Adversarial mutation tests distinguish assignment, constraint rejection and atomic publication.
use serde_json::{json, Value};
use sqlolla_core::*;

/// The fixture has a short character column to distinguish assignment and explicit CAST.
fn catalog() -> Catalog {
    serde_json::from_value(json!({"search_path":["public"],"tables":{"t":{
        "schema":"public","name":"t","columns":[{"name":"v","ty":{"varchar":{"length":1}},"nullable":false}]}}})).unwrap()
}

/// Wrap a complete INSERT AST in the pinned syntax-only v2 protocol.
fn insert(value: Value) -> Value {
    json!({"protocol":"sqlolla.parser.v2","parser_commit":"ad15c585155ff7fc2c5fc4c9746905e4959ff5eb",
        "assurance":"syntax-only","semantic_preservation":false,"result":{"parse_status":"parsed","export_status":"exported",
        "ast":{"tag":"insert","table":"t","alias":null,"columns":["v"],"source":{"tag":"values","rows":[[value]]},
            "on_conflict":null,"returning":[]}}})
}

/// Writes require affirmative, complete schema semantics even when there are no named constraints.
fn context() -> ExecutionContext {
    ExecutionContext {
        table_metadata: std::collections::BTreeMap::from([(
            "t".into(),
            TableSemantics {
                complete: true,
                ..Default::default()
            },
        )]),
        ..Default::default()
    }
}

/// Typed character assignment must reject overlength data instead of explicit-cast truncation.
#[test]
fn assignment_does_not_truncate_like_explicit_cast() {
    let ast = insert(json!({"tag":"numbered_parameter","index":1}));
    let plan = bind_statement(
        &ast,
        &catalog(),
        &Parameters::from([(1, Cell::Text("long".into()))]),
    )
    .unwrap();
    let outcomes =
        evaluate_statement(&plan, &Database::from([("t".into(), vec![])]), &context()).unwrap();
    let StatementOutcome::SqlError(failure) = &outcomes.outcomes[0] else {
        panic!("Assignment must fail");
    };
    assert_eq!(failure.sqlstate, "22001");
    let cast = json!({"tag":"cast","arg":{"tag":"text","value":"long"},
        "type":{"name":{"tag":"scalar_type","name":"varchar"},"modifiers":[1]}});
    let plan = bind_statement(&insert(cast), &catalog(), &Parameters::new()).unwrap();
    let outcomes =
        evaluate_statement(&plan, &Database::from([("t".into(), vec![])]), &context()).unwrap();
    let StatementOutcome::Succeeded(value) = &outcomes.outcomes[0] else {
        panic!("Explicit cast must succeed");
    };
    assert_eq!(scalar::output(&value.database["t"][0][0]).unwrap(), "l");
}

/// NOT NULL failures retain PostgreSQL's column field independently from constraint identity.
#[test]
fn not_null_diagnostic_names_column() {
    let plan = bind_statement(
        &insert(json!({"tag":"sql_null"})),
        &catalog(),
        &Parameters::new(),
    )
    .unwrap();
    let before = Database::from([("t".into(), vec![])]);
    let outcomes = evaluate_statement(&plan, &before, &context()).unwrap();
    let StatementOutcome::SqlError(failure) = &outcomes.outcomes[0] else {
        panic!("NULL must fail");
    };
    assert_eq!(failure.sqlstate, "23502");
    assert_eq!(failure.column_name.as_deref(), Some("v"));
    assert_eq!(failure.constraint_name, None);
    assert!(before["t"].is_empty());
}

/// Explicit DEFAULT requests the declaration, whereas SQL NULL still violates NOT NULL.
#[test]
fn explicit_default_is_distinct_from_null() {
    let context = default_context();
    let plan = bind_statement(
        &insert(json!({"tag":"default_value"})),
        &catalog(),
        &Parameters::new(),
    )
    .unwrap();
    let before = Database::from([("t".into(), vec![])]);
    let outcome = evaluate_statement(&plan, &before, &context).unwrap();
    let StatementOutcome::Succeeded(result) = &outcome.outcomes[0] else {
        panic!()
    };
    assert_eq!(scalar::output(&result.database["t"][0][0]).unwrap(), "x");
    let plan = bind_statement(
        &insert(json!({"tag":"sql_null"})),
        &catalog(),
        &Parameters::new(),
    )
    .unwrap();
    let outcome = evaluate_statement(&plan, &before, &context).unwrap();
    let StatementOutcome::SqlError(failure) = &outcome.outcomes[0] else {
        panic!()
    };
    assert_eq!(failure.sqlstate, "23502");
}

/// A literal default fixture has no sequence, external clock or unmodeled function effects.
fn default_context() -> ExecutionContext {
    let mut context = context();
    let value = scalar::input(&SqlType::Varchar { length: Some(1) }, "x").unwrap();
    context
        .table_metadata
        .get_mut("t")
        .unwrap()
        .defaults
        .insert(0, DefaultSpec::Literal { value });
    context
}

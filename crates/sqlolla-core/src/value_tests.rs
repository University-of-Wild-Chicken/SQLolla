//! Text admission regressions cover direct Rust construction and every external-value category.
use crate::*;
use serde_json::json;

/// Untrusted JSON and direct cells cannot introduce PostgreSQL-impossible text.
#[test]
fn text_transport_rejects_nul_and_oversize() {
    assert!(serde_json::from_value::<Cell>(json!({"type":"text","value":"bad\0text"})).is_err());
    assert_eq!(
        Cell::Text("bad\0text".into()).validate().unwrap_err().code,
        "invalid.text_nul"
    );
    assert_eq!(
        Cell::Text("x".repeat(MAX_TEXT_BYTES + 1))
            .validate()
            .unwrap_err()
            .code,
        "resource.scalar_bytes"
    );
    assert_eq!(
        scalar::input(&SqlType::Text, "bad\0text").unwrap_err().code,
        "sql.22021"
    );
}

/// Even unused invalid parameters fail before an unrelated parser or SQL verdict is considered.
#[test]
fn invalid_parameter_is_admission_failure() {
    let catalog: Catalog =
        serde_json::from_value(json!({"search_path":["public"],"tables":{}})).unwrap();
    let params = [(1, Cell::Text("bad\0text".into()))].into();
    assert_eq!(
        bind(&json!({}), &catalog, &params).unwrap_err().code,
        "invalid.text_nul"
    );
    assert_eq!(
        bind_statement(&json!({}), &catalog, &params)
            .unwrap_err()
            .code,
        "invalid.text_nul"
    );
}

/// A valid declared text column cannot hide invalid bytes in an unused input table.
#[test]
fn database_text_is_checked_before_queries() {
    let catalog: Catalog =
        serde_json::from_value(json!({"search_path":["public"],"tables":{"public.r":{
        "schema":"public","name":"r","columns":[{"name":"x","ty":"text","nullable":true}]}}}))
        .unwrap();
    let database = [(
        "public.r".into(),
        vec![vec![Cell::Text("bad\0text".into())]],
    )]
    .into();
    assert_eq!(
        validate_database(&catalog, &database).unwrap_err().code,
        "invalid.text_nul"
    );
}

/// Settings and unused literal defaults share the same prestate refusal rather than SQLSTATE.
#[test]
fn context_text_is_checked_before_execution() {
    let mut context = ExecutionContext::default();
    context
        .settings
        .insert("unused".into(), Cell::Text("bad\0text".into()));
    assert_eq!(
        crate::execution::validate_context(&context)
            .unwrap_err()
            .code,
        "invalid.text_nul"
    );
    context.settings.clear();
    let mut table = TableSemantics::default();
    table.defaults.insert(
        0,
        DefaultSpec::Literal {
            value: Cell::Text("bad\0text".into()),
        },
    );
    context.table_metadata.insert("unused".into(), table);
    assert_eq!(
        crate::execution::validate_context(&context)
            .unwrap_err()
            .code,
        "invalid.text_nul"
    );
}

/// Direct scalar constructors cannot return an oversized concatenation or propagate NUL text.
#[test]
fn scalar_text_construction_is_checked() {
    let invalid = Cell::Text("bad\0text".into());
    assert_eq!(
        scalar::function("length", &[invalid]).unwrap_err().code,
        "invalid.text_nul"
    );
    let text = Cell::Text("x".repeat(MAX_TEXT_BYTES / 2 + 1));
    assert_eq!(
        scalar::function("concat", &[text.clone(), text.clone()])
            .unwrap_err()
            .code,
        "resource.scalar_bytes"
    );
    assert_eq!(
        scalar::arithmetic("concat", &text, &text).unwrap_err().code,
        "resource.scalar_bytes"
    );
}

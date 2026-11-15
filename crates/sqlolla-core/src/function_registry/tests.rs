//! Registry regressions protect call identity and metadata immutability admission.
use super::*;

/// Session/effect calls never inherit the immutable default from scalar body metadata.
#[test]
fn effects_and_stability_are_explicit() {
    assert_eq!(builtin("nextval").unwrap().effect, Effect::Sequence);
    assert_eq!(builtin("pg_notify").unwrap().effect, Effect::Notification);
    assert_eq!(builtin("concat").unwrap().volatility, Volatility::Stable);
    assert!(builtin("imaginary_function").is_none());
    assert!(builtin_name("pg_read_file"));
    assert!(builtin_signatures("pg_read_file").is_err());
}

/// Implicit call conversion differs from explicit and assignment conversion.
#[test]
fn typed_arguments_cannot_narrow_or_parse_text() {
    assert!(!implicit_cast(&SqlType::Int64, &SqlType::Int32));
    assert!(!implicit_cast(&SqlType::Text, &SqlType::Int32));
    assert!(implicit_cast(&SqlType::Int32, &SqlType::Int64));
    assert!(!implicit_cast(
        &SqlType::Name,
        &SqlType::Varchar { length: None }
    ));
    assert!(implicit_cast(&SqlType::Text, &SqlType::Name));
    assert_eq!(
        search_path(&["public".into()]),
        vec!["pg_catalog", "public"]
    );
    assert_eq!(
        search_path(&["public".into(), "pg_catalog".into()]),
        vec!["public", "pg_catalog"]
    );
}

/// Even a lazy unreachable branch cannot confer immutable index admission on a stable call.
#[test]
fn local_body_purity_has_no_row_count_shortcut() {
    let expression = BoundExpr {
        ty: SqlType::Text,
        nullable: false,
        node: ExprNode::Context {
            key: "transaction_timestamp".into(),
            missing_ok: false,
        },
    };
    assert_eq!(
        require_immutable(&expression, "unsupported.index_immutability")
            .unwrap_err()
            .code,
        "unsupported.index_immutability"
    );
    let pure = BoundExpr {
        ty: SqlType::Int32,
        nullable: false,
        node: ExprNode::Literal(Cell::Int32(1)),
    };
    require_immutable(&pure, "unsupported.index_immutability").unwrap();
}

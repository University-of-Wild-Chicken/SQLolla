//! Signature selection tests do not execute or replace function bodies.
use super::*;

/// Construct one complete supported declaration without prebinding its unused body.
fn function(schema: &str, name: &str, ty: SqlType) -> ImmutableSqlFunction {
    ImmutableSqlFunction {
        profile: "sql_immutable_non_strict_invoker_v1".into(),
        schema: schema.into(),
        name: name.into(),
        can_execute: true,
        can_use_schema: true,
        parameters: vec![Column {
            name: "a".into(),
            ty,
            nullable: true,
            qualifier: None,
            schema: None,
        }],
        returns: SqlType::Int32,
        body_sql: "SELECT 7".into(),
        prepared: None,
    }
}

/// Resolve one typed parameter independently of runtime argument evaluation.
fn chosen<'a>(
    functions: &'a SqlFunctions,
    name: &str,
    value: Cell,
) -> Result<Option<&'a ImmutableSqlFunction>> {
    let parameters = Parameters::from([(1, value)]);
    let expressions = Expressions {
        functions: Some(functions),
        lookup: None,
        columns: &[],
        params: &parameters,
        extended: true,
        scope: None,
    };
    resolve(
        &expressions,
        functions,
        &json!({"tag":"call","name":[name],"args":[{"tag":"numbered_parameter","index":1}]}),
    )
}

/// Implicit pg_catalog beats an identical user signature, while explicit placement reverses it.
#[test]
fn catalog_precedence_is_signature_specific() {
    let mut functions = SqlFunctions {
        search_path: vec!["public".into()],
        definitions: vec![function("public", "abs", SqlType::Int32)],
    };
    assert!(chosen(&functions, "abs", Cell::Int32(1)).unwrap().is_none());
    functions.search_path.push("pg_catalog".into());
    assert!(chosen(&functions, "abs", Cell::Int32(1)).unwrap().is_some());
}

/// Builtin existence by name cannot displace an exact, differently typed declared signature.
#[test]
fn exact_declared_signature_wins_over_incompatible_builtin() {
    let functions = SqlFunctions {
        search_path: vec!["public".into()],
        definitions: vec![function("public", "abs", SqlType::Text)],
    };
    assert!(chosen(&functions, "abs", Cell::Text("abc".into()))
        .unwrap()
        .is_some());
    assert!(chosen(&functions, "abs", Cell::Int32(1)).unwrap().is_none());
}

/// Implicit narrowing and typed-text parsing cannot manufacture an applicable overload.
#[test]
fn incompatible_typed_actuals_are_sql_undefined_function() {
    let functions = SqlFunctions {
        search_path: vec!["public".into()],
        definitions: vec![function("public", "f", SqlType::Int32)],
    };
    for value in [
        Cell::Int64(1),
        Cell::Text("1".into()),
        Cell::Null(SqlType::Int64),
    ] {
        assert_eq!(
            chosen(&functions, "f", value).unwrap_err().code,
            "sql.42883"
        );
    }
}

/// Differing schema signatures compete by exact type before their relative schema position.
#[test]
fn later_exact_signature_beats_earlier_widening() {
    let functions = SqlFunctions {
        search_path: vec!["other".into(), "public".into()],
        definitions: vec![
            function("other", "f", SqlType::Int64),
            function("public", "f", SqlType::Int32),
        ],
    };
    assert_eq!(
        chosen(&functions, "f", Cell::Int32(1))
            .unwrap()
            .unwrap()
            .schema,
        "public"
    );
}

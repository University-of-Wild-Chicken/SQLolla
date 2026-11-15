//! Adversarial scalar regressions independently checked against PostgreSQL16.16.
use super::*;
use crate::{Cell, SqlType};
use std::cmp::Ordering;

/// Signature rejection precedes strict NULL folding and keeps numeric storage modifiers out of results.
#[test]
fn absolute_value_signatures() {
    for cell in [
        Cell::Bool(true),
        Cell::Null(SqlType::Bool),
        Cell::Text("-2".into()),
    ] {
        assert_eq!(function("abs", &[cell]).unwrap_err().code, "sql.42883");
    }
    assert_eq!(function("abs", &[]).unwrap_err().code, "sql.42883");
    assert_eq!(
        function("abs", &[Cell::Int32(1), Cell::Int32(2)])
            .unwrap_err()
            .code,
        "sql.42883"
    );
    assert_eq!(
        function("abs", &[Cell::Int32(i32::MIN)]).unwrap_err().code,
        "sql.22003"
    );
    let value = value(
        SqlType::Numeric {
            precision: Some(4),
            scale: Some(2),
        },
        "-2.10",
    );
    let result = function("abs", &[value]).unwrap();
    assert_eq!(result.sql_type(), numeric());
    assert_eq!(output(&result).unwrap(), "2.10");
}

/// Catalog name truncation respects UTF-8 byte boundaries rather than character count.
#[test]
fn name_and_text_comparison_profiles() {
    let name = value(SqlType::Name, &"é".repeat(40));
    assert_eq!(output(&name).unwrap().len(), 62);
    assert_eq!(output(&name).unwrap().chars().count(), 31);
    assert_eq!(
        output(&value(SqlType::Name, &"a".repeat(80)))
            .unwrap()
            .len(),
        63
    );
    let padded = value(SqlType::Char { length: 2 }, "a ");
    assert!(!duplicate_eq(&padded, &Cell::Text("a ".into())).unwrap());
}

/// Cast existence and strict concatenation are independent of textual formatting.
#[test]
fn rejected_cross_cast_and_strict_concatenation() {
    assert_eq!(
        cast(&Cell::Bool(true), &SqlType::Date).unwrap_err().code,
        "sql.42846"
    );
    let null = Cell::Null(SqlType::Text);
    let text = Cell::Text("x".into());
    assert_eq!(arithmetic("concat", &null, &text).unwrap(), null);
    assert_eq!(
        function("concat", &[Cell::Null(SqlType::Text), text.clone()]).unwrap(),
        text
    );
}

/// Construct an unconstrained PostgreSQL numeric type for arithmetic tests.
fn numeric() -> SqlType {
    SqlType::Numeric {
        precision: None,
        scale: None,
    }
}
/// Parse a typed test value, failing visibly if the fixture itself is invalid.
fn value(ty: SqlType, text: &str) -> Cell {
    input(&ty, text).unwrap()
}

/// Exact decimal arithmetic does not inherit binary floating rounding.
#[test]
fn decimal_arithmetic_and_division_scale() {
    let sum = arithmetic("add", &value(numeric(), "0.1"), &value(numeric(), "0.2")).unwrap();
    assert_eq!(output(&sum).unwrap(), "0.3");
    let division = arithmetic("divide", &value(numeric(), "1"), &value(numeric(), "3")).unwrap();
    assert_eq!(output(&division).unwrap(), "0.33333333333333333333");
}

/// Numeric display-scale identity survives even when SQL considers values equal.
#[test]
fn transport_and_duplicate_equality_are_distinct() {
    let left = value(numeric(), "1.0");
    let right = value(numeric(), "1.00");
    assert_ne!(left, right);
    assert!(duplicate_eq(&left, &right).unwrap());
    assert_eq!(
        cast(&left, &SqlType::Text).unwrap(),
        Cell::Text("1.0".into())
    );
    assert_eq!(
        cast(&right, &SqlType::Text).unwrap(),
        Cell::Text("1.00".into())
    );
}

/// Numeric typmods round half away from zero before checking precision overflow.
#[test]
fn numeric_typmod_and_signed_ties() {
    let ty = SqlType::Numeric {
        precision: Some(3),
        scale: Some(1),
    };
    assert_eq!(output(&value(ty.clone(), "-1.25")).unwrap(), "-1.3");
    assert_eq!(input(&ty, "99.95").unwrap_err().code, "sql.22003");
    assert_eq!(output(&value(numeric(), "-0.000")).unwrap(), "0.000");
}

/// Float NaN is equal to itself and sorts after infinity; zeros retain spelling.
#[test]
fn float_nan_and_negative_zero() {
    let nan = value(SqlType::Float64, "NaN");
    assert!(duplicate_eq(&nan, &nan).unwrap());
    assert_eq!(
        compare_sql(&nan, &value(SqlType::Float64, "Infinity")).unwrap(),
        Ordering::Greater
    );
    let minus = value(SqlType::Float64, "-0");
    let plus = value(SqlType::Float64, "0");
    assert_ne!(minus, plus);
    assert!(duplicate_eq(&minus, &plus).unwrap());
    assert_eq!(output(&minus).unwrap(), "-0");
}

/// Float output switches to PostgreSQL scientific notation at its actual thresholds.
#[test]
fn float_text_and_underflow() {
    for (source, expected) in [
        ("1e15", "1e+15"),
        ("1e14", "100000000000000"),
        ("1e-5", "1e-05"),
        ("1e-4", "0.0001"),
    ] {
        assert_eq!(output(&value(SqlType::Float64, source)).unwrap(), expected);
    }
    assert!(input(&SqlType::Float64, "0e-999").is_ok());
    assert_eq!(
        input(&SqlType::Float64, "1e-999").unwrap_err().code,
        "sql.22003"
    );
}

/// Explicit character casts truncate; assignment rejects significant excess input.
#[test]
fn character_cast_and_assignment_differ() {
    let text = Cell::Text("abcd".into());
    let ty = SqlType::Varchar { length: Some(2) };
    assert_eq!(output(&cast(&text, &ty).unwrap()).unwrap(), "ab");
    assert_eq!(assignment_cast(&text, &ty).unwrap_err().code, "sql.22001");
    let left = value(SqlType::Char { length: 2 }, "a ");
    let right = value(ty, "a ");
    assert_ne!(left, right);
    assert!(duplicate_eq(&left, &right).unwrap());
    assert_eq!(cast(&left, &SqlType::Text).unwrap(), Cell::Text("a".into()));
}

/// Enum labels compare in declaration order and retain nominal enum identity.
#[test]
fn enum_identity_and_order() {
    let ty = SqlType::Enum {
        name: "public.priority".into(),
        labels: vec!["high".into(), "low".into()],
    };
    assert_eq!(
        compare_sql(&value(ty.clone(), "high"), &value(ty.clone(), "low")).unwrap(),
        Ordering::Less
    );
    assert_eq!(input(&ty, "missing").unwrap_err().code, "sql.22P02");
    let other = SqlType::Enum {
        name: "public.other".into(),
        labels: vec!["high".into(), "low".into()],
    };
    assert!(compare_sql(&value(ty, "high"), &value(other, "high")).is_err());
}

/// Arrays distinguish NULL from quoted NULL and preserve nondefault lower bounds.
#[test]
fn array_nulls_dimensions_and_numeric_equality() {
    let ty = SqlType::Array(Box::new(SqlType::Int32));
    let bounded = value(ty.clone(), "[0:1]={1,NULL}");
    assert_eq!(output(&bounded).unwrap(), "[0:1]={1,NULL}");
    assert_ne!(
        compare_sql(&bounded, &value(ty, "{1,NULL}")).unwrap(),
        Ordering::Equal
    );
    let numeric_array = SqlType::Array(Box::new(numeric()));
    assert!(duplicate_eq(
        &value(numeric_array.clone(), "{1.0,NULL}"),
        &value(numeric_array, "{1.00,NULL}")
    )
    .unwrap());
}

/// Nonrectangular arrays are errors instead of flattened data with altered meaning.
#[test]
fn array_rectangularity_and_quoted_null() {
    let ty = SqlType::Array(Box::new(SqlType::Int32));
    assert_eq!(input(&ty, "{{1},{2,3}}").unwrap_err().code, "sql.22P02");
    assert_eq!(
        output(&value(ty, "{{1,NULL},{2,3}}")).unwrap(),
        "{{1,NULL},{2,3}}"
    );
    let text = SqlType::Array(Box::new(SqlType::Text));
    assert!(!duplicate_eq(&value(text.clone(), "{NULL}"), &value(text, "{\"NULL\"}")).unwrap());
}

/// JSONB uses last duplicate keys, exact numeric equality and its empty-array rule.
#[test]
fn jsonb_canonical_semantics() {
    let left = value(SqlType::Jsonb, "{\"a\":0,\"a\":1.0}");
    let right = value(SqlType::Jsonb, "{\"a\":1.00}");
    assert!(duplicate_eq(&left, &right).unwrap());
    assert_eq!(
        compare_sql(&value(SqlType::Jsonb, "[]"), &value(SqlType::Jsonb, "null")).unwrap(),
        Ordering::Less
    );
    assert_eq!(
        output(&value(SqlType::Jsonb, "{\"aa\":1,\"b\":2}")).unwrap(),
        "{\"b\": 2, \"aa\": 1}"
    );
}

/// Binary values preserve octets independently of Unicode or textual backslashes.
#[test]
fn bytea_and_bit_input() {
    assert_eq!(
        output(&value(SqlType::Bytea, "\\000\\377A")).unwrap(),
        "\\x00ff41"
    );
    assert_eq!(
        output(&cast(&Cell::Int32(-1), &SqlType::Bit { length: 8 }).unwrap()).unwrap(),
        "11111111"
    );
    assert_eq!(
        input(&SqlType::Bit { length: 8 }, "1").unwrap_err().code,
        "sql.22026"
    );
}

/// Date arithmetic clips calendar months; timestamptz compares UTC instants.
#[test]
fn temporal_calendar_and_utc() {
    let date = value(SqlType::Date, "2020-01-31");
    let month = value(SqlType::Interval, "1 month");
    assert_eq!(
        output(&arithmetic("add", &date, &month).unwrap()).unwrap(),
        "2020-02-29 00:00:00"
    );
    let left = value(SqlType::Timestamptz, "2000-01-01 01:00:00+01");
    let right = value(SqlType::Timestamptz, "2000-01-01 00:00:00+00");
    assert!(duplicate_eq(&left, &right).unwrap());
    assert!(duplicate_eq(
        &value(SqlType::Interval, "1 month"),
        &value(SqlType::Interval, "30 days")
    )
    .unwrap());
}

/// Scalar wire validation rejects malformed values before they become bag atoms.
#[test]
fn malformed_scalar_wire_is_rejected() {
    let json = r#"{"type":"scalar","value":{"ty":{"numeric":{"precision":null,"scale":null}},"value":"not numeric"}}"#;
    assert!(serde_json::from_str::<Cell>(json).is_err());
    let cell = value(SqlType::Array(Box::new(SqlType::Int32)), "{1,NULL}");
    assert_eq!(
        serde_json::from_str::<Cell>(&serde_json::to_string(&cell).unwrap()).unwrap(),
        cell
    );
}

/// Strict operators preserve typed NULL even when a non-NULL divisor is zero.
#[test]
fn strict_null_and_checked_errors() {
    assert_eq!(
        arithmetic("divide", &Cell::Null(numeric()), &value(numeric(), "0")).unwrap(),
        Cell::Null(numeric())
    );
    assert_eq!(
        arithmetic("divide", &Cell::Int64(i64::MIN), &Cell::Int64(-1))
            .unwrap_err()
            .code,
        "sql.22003"
    );
    assert_eq!(
        arithmetic("divide", &value(numeric(), "1"), &value(numeric(), "0"))
            .unwrap_err()
            .code,
        "sql.22012"
    );
}

/// PostgreSQL ARRAY constructors retain shape, and all-empty children yield an empty array.
#[test]
fn array_constructor_shape_and_empty_children() {
    let ty = SqlType::Array(Box::new(SqlType::Int32));
    let a = value(ty.clone(), "[0:1]={1,NULL}");
    let b = value(ty.clone(), "[0:1]={2,3}");
    let result = array_constructor(&ty, &[a, b]).unwrap();
    assert_eq!(result.sql_type(), ty);
    assert_eq!(output(&result).unwrap(), "[1:2][0:1]={{1,NULL},{2,3}}");
    assert_eq!(
        output(
            &array_constructor(&ty, &[Cell::Null(ty.clone()), value(ty.clone(), "{}")]).unwrap()
        )
        .unwrap(),
        "{}"
    );
    assert_eq!(
        array_constructor(&ty, &[value(ty.clone(), "{1}"), Cell::Null(ty.clone())])
            .unwrap_err()
            .code,
        "sql.2202E"
    );
    assert_eq!(
        array_constructor(&ty, &[value(ty.clone(), "{1}"), value(ty.clone(), "{1,2}")])
            .unwrap_err()
            .code,
        "sql.2202E"
    );
}

/// Quantified predicates distinguish a NULL array from an empty array and preserve NULL elements.
#[test]
fn array_elements_retain_nulls_and_row_major_order() {
    let ty = SqlType::Array(Box::new(SqlType::Int32));
    assert_eq!(array_elements(&Cell::Null(ty.clone())).unwrap(), None);
    assert_eq!(
        array_elements(&value(ty.clone(), "{}")).unwrap(),
        Some(vec![])
    );
    assert_eq!(
        array_elements(&value(ty, "[0:1][2:3]={{1,NULL},{2,3}}")).unwrap(),
        Some(vec![
            Cell::Int32(1),
            Cell::Null(SqlType::Int32),
            Cell::Int32(2),
            Cell::Int32(3)
        ])
    );
}

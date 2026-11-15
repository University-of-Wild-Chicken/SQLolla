//! Catalog scalar regressions independently checked against PostgreSQL16 input and cast behavior.
use super::{cast, compare_sql, input, output};
use crate::{Cell, SqlType};
use std::cmp::Ordering;

/// Unsigned identifiers retain nominal type, full width and the strtoul radix profile.
#[test]
fn oid_input_and_identity() {
    for (source, expected) in [
        ("4294967295", "4294967295"),
        ("-1", "4294967295"),
        ("0x10", "16"),
        ("010", "8"),
    ] {
        let value = input(&SqlType::Oid, source).unwrap();
        assert_eq!(output(&value).unwrap(), expected);
        assert_eq!(value.sql_type(), SqlType::Oid);
        assert_ne!(value, Cell::Int64(expected.parse().unwrap()));
    }
    assert_eq!(
        input(&SqlType::Oid, "4294967296").unwrap_err().code,
        "sql.22003"
    );
    assert_eq!(input(&SqlType::Oid, "1_000").unwrap_err().code, "sql.22P02");
    assert_eq!(
        input(&SqlType::Oid, "-2147483649").unwrap_err().code,
        "unsupported.oid_negative_profile"
    );
}

/// Integer casts preserve signed32 bits while bigint conversions check the unsigned range.
#[test]
fn oid_integer_casts() {
    let value = input(&SqlType::Oid, "4294967295").unwrap();
    assert_eq!(cast(&value, &SqlType::Int32).unwrap(), Cell::Int32(-1));
    assert_eq!(
        cast(&value, &SqlType::Int64).unwrap(),
        Cell::Int64(4294967295)
    );
    assert_eq!(cast(&Cell::Int32(-1), &SqlType::Oid).unwrap(), value);
    assert_eq!(
        cast(&Cell::Int64(-1), &SqlType::Oid).unwrap_err().code,
        "sql.22003"
    );
    assert_eq!(
        compare_sql(&value, &input(&SqlType::Oid, "2147483647").unwrap()).unwrap(),
        Ordering::Greater
    );
}

/// Internal char keeps a byte, not a Unicode character or blank-padded SQL char value.
#[test]
fn catalog_char_bytes_and_casts() {
    for (source, expected) in [
        ("", ""),
        ("abc", "a"),
        ("é", "\\303"),
        ("\\377", "\\377"),
        ("\\000", ""),
    ] {
        assert_eq!(
            output(&input(&SqlType::InternalChar, source).unwrap()).unwrap(),
            expected
        );
    }
    let last = input(&SqlType::InternalChar, "\\377").unwrap();
    assert_eq!(cast(&last, &SqlType::Int32).unwrap(), Cell::Int32(-1));
    assert_eq!(
        cast(&Cell::Int32(-1), &SqlType::InternalChar).unwrap(),
        last
    );
    assert_eq!(
        cast(&Cell::Int32(128), &SqlType::InternalChar)
            .unwrap_err()
            .code,
        "sql.22003"
    );
    assert_eq!(
        compare_sql(&last, &input(&SqlType::InternalChar, "a").unwrap()).unwrap(),
        Ordering::Greater
    );
}

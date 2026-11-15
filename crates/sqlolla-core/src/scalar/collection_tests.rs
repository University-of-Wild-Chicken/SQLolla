//! Adversarial collection regressions independent of the external PostgreSQL fixture.
use super::*;
use crate::{Cell, SqlType};

/// JSONB concatenation replaces a whole nested object and preserves array duplicates.
#[test]
fn jsonb_concat_is_top_level() {
    let left = input(&SqlType::Jsonb, r#"{"a":{"left":1},"x":2}"#).unwrap();
    let right = input(&SqlType::Jsonb, r#"{"a":{"right":3}}"#).unwrap();
    assert_eq!(
        output(&arithmetic("concat", &left, &right).unwrap()).unwrap(),
        r#"{"a": {"right": 3}, "x": 2}"#
    );
    let array = input(&SqlType::Jsonb, "[1,1]").unwrap();
    assert_eq!(
        output(&arithmetic("concat", &array, &array).unwrap()).unwrap(),
        "[1, 1, 1, 1]"
    );
}

/// JSON preserves spelling and duplicate keys; JSONB normalizes only after explicit conversion.
#[test]
fn json_text_is_distinct_from_jsonb() {
    let value = input(&SqlType::Json, "{ \"a\":1,\"a\":2 }").unwrap();
    assert_eq!(output(&value).unwrap(), "{ \"a\":1,\"a\":2 }");
    assert_eq!(
        output(&cast(&value, &SqlType::Jsonb).unwrap()).unwrap(),
        "{\"a\": 2}"
    );
    assert_eq!(
        comparison_type(&SqlType::Json, &SqlType::Json)
            .unwrap_err()
            .code,
        "sql.42883"
    );
}

/// Nullable constructor values are retained while a NULL key has family-specific SQLSTATE.
#[test]
fn json_constructors_preserve_null_values() {
    let args = [Cell::Text("a".into()), Cell::Null(SqlType::Text)];
    assert_eq!(
        output(&function("json_build_object", &args).unwrap()).unwrap(),
        "{\"a\" : null}"
    );
    let args = [Cell::Null(SqlType::Text), Cell::Int32(1)];
    assert_eq!(
        function("json_build_object", &args).unwrap_err().code,
        "sql.22004"
    );
    assert_eq!(
        function("jsonb_build_object", &args).unwrap_err().code,
        "sql.22023"
    );
}

/// Existence is top-level string matching and ignores NULL keys in any/all arrays.
#[test]
fn jsonb_existence_ignores_null_keys() {
    let object = input(&SqlType::Jsonb, r#"[{"a":1},"b"]"#).unwrap();
    assert_eq!(
        function("jsonb_exists", &[object.clone(), Cell::Text("a".into())]).unwrap(),
        Cell::Bool(false)
    );
    let keys = input(&SqlType::Array(Box::new(SqlType::Text)), "{NULL}").unwrap();
    assert_eq!(
        function("jsonb_exists_all", &[object, keys]).unwrap(),
        Cell::Bool(true)
    );
}

/// Array indexing honors non-one lower bounds and rejects the wrong dimension count as NULL.
#[test]
fn array_subscript_preserves_lower_bounds() {
    let array = input(
        &SqlType::Array(Box::new(SqlType::Int32)),
        "[0:1][2:3]={{10,20},{30,40}}",
    )
    .unwrap();
    assert_eq!(
        function(
            "__sqlolla_array_element",
            &[array.clone(), Cell::Int32(1), Cell::Int32(2)]
        )
        .unwrap(),
        Cell::Int32(30)
    );
    assert_eq!(
        function("__sqlolla_array_element", &[array, Cell::Int32(0)]).unwrap(),
        Cell::Null(SqlType::Int32)
    );
}

/// get_bit uses zero-based left-to-right indexing, with SQL errors beyond either end.
#[test]
fn bit_index_is_not_an_array_index() {
    let bits = input(&SqlType::Bit { length: 8 }, "10000000").unwrap();
    assert_eq!(
        function("get_bit", &[bits.clone(), Cell::Int32(0)]).unwrap(),
        Cell::Int32(1)
    );
    assert_eq!(
        function("get_bit", &[bits, Cell::Int32(-1)])
            .unwrap_err()
            .code,
        "sql.2202E"
    );
}

/// Append preserves NULL elements and measurement distinguishes empty arrays from missing dimensions.
#[test]
fn array_append_and_measure() {
    let ty = SqlType::Array(Box::new(SqlType::Int32));
    let result = function(
        "array_append",
        &[Cell::Null(ty.clone()), Cell::Null(SqlType::Int32)],
    )
    .unwrap();
    assert_eq!(output(&result).unwrap(), "{NULL}");
    assert_eq!(function("cardinality", &[result]).unwrap(), Cell::Int32(1));
    let empty = input(&ty, "{}").unwrap();
    assert_eq!(
        function("array_length", &[empty, Cell::Int32(1)]).unwrap(),
        Cell::Null(SqlType::Int32)
    );
}

/// JSONB updates never synthesize intermediate containers and skip unreachable NULL path suffixes.
#[test]
fn jsonb_set_missing_parent_and_null_path() {
    let json = input(&SqlType::Jsonb, "{}").unwrap();
    let path = input(&SqlType::Array(Box::new(SqlType::Text)), "{a,NULL}").unwrap();
    let value = input(&SqlType::Jsonb, "1").unwrap();
    assert_eq!(
        function("jsonb_set", &[json, path.clone(), value.clone()]).unwrap(),
        input(&SqlType::Jsonb, "{}").unwrap()
    );
    let json = input(&SqlType::Jsonb, "{\"a\":1}").unwrap();
    assert_eq!(
        function("jsonb_set", &[json, path, value])
            .unwrap_err()
            .code,
        "sql.22004"
    );
}

/// Custom NULL delimiter handling must not be replaced by the generic strict fast path.
#[test]
fn string_split_null_policy() {
    let text = Cell::Text("éx".into());
    let result = function("string_to_array", &[text, Cell::Null(SqlType::Text)]).unwrap();
    assert_eq!(
        array_elements(&result).unwrap().unwrap(),
        vec![Cell::Text("é".into()), Cell::Text("x".into())]
    );
    let result = function("jsonb_typeof", &[input(&SqlType::Jsonb, "null").unwrap()]).unwrap();
    assert_eq!(result, Cell::Text("null".into()));
    assert_eq!(
        function("split_part", &[Cell::Null(SqlType::Text)])
            .unwrap_err()
            .code,
        "unsupported.function"
    );
}

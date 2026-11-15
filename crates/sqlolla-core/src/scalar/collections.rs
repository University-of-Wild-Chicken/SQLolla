//! Checked top-level JSONB collection rules and lower-bound-aware array lookup.
use super::{json_value::JsonValue, rich::Key, ScalarValue};
use crate::json::error;
use crate::{Cell, Result, SqlType};

/// Merge only the top level, retaining right object values and repeated array elements.
pub(crate) fn json_concat(left: &Cell, right: &Cell) -> Result<Cell> {
    let (left, right) = (json(left)?, json(right)?);
    let value = match (left, right) {
        (JsonValue::Object(left), JsonValue::Object(right)) => {
            let mut values = left
                .iter()
                .cloned()
                .collect::<std::collections::BTreeMap<_, _>>();
            values.extend(right.iter().cloned());
            JsonValue::Object(values.into_iter().collect())
        }
        _ => JsonValue::Array(
            json_elements(left)
                .into_iter()
                .chain(json_elements(right))
                .collect(),
        ),
    };
    super::input(&SqlType::Jsonb, &value.text())
}

/// Arrays contribute their elements; every other JSON value contributes itself once.
fn json_elements(value: &JsonValue) -> Vec<JsonValue> {
    match value {
        JsonValue::Array(values) => values.clone(),
        value => vec![value.clone()],
    }
}

/// Borrow a validated JSONB tree without reparsing or confusing SQL NULL with JSON null.
fn json(value: &Cell) -> Result<&JsonValue> {
    match value {
        Cell::Scalar(ScalarValue {
            key: Key::Json(value),
            ..
        }) => Ok(value),
        _ => Err(error(
            "invalid.jsonb_operand",
            "Expected a non-NULL JSONB value",
        )),
    }
}

/// Match top-level keys/strings; nested containers and numeric elements are not strings.
fn exists(value: &JsonValue, key: &str) -> bool {
    match value {
        JsonValue::Object(values) => values.iter().any(|entry| entry.0 == key),
        JsonValue::Array(values) => values
            .iter()
            .any(|value| matches!(value, JsonValue::String(text) if text == key)),
        JsonValue::String(value) => value == key,
        _ => false,
    }
}

/// Evaluate strict existence functions; NULL search-array elements are ignored.
pub(crate) fn json_exists(name: &str, args: &[Cell]) -> Result<Cell> {
    if args.len() != 2 {
        return Err(error("sql.42883", "JSONB existence requires two arguments"));
    }
    if args.iter().any(Cell::is_null) {
        return Ok(Cell::Null(SqlType::Bool));
    }
    let value = json(&args[0])?;
    if name == "jsonb_exists" {
        return Ok(Cell::Bool(exists(value, &super::output(&args[1])?)));
    }
    let keys = super::array_elements(&args[1])?.unwrap_or_default();
    let keys = keys
        .iter()
        .filter(|key| !key.is_null())
        .map(super::output)
        .collect::<Result<Vec<_>>>()?;
    Ok(Cell::Bool(if name == "jsonb_exists_all" {
        keys.iter().all(|key| exists(value, key))
    } else {
        keys.iter().any(|key| exists(value, key))
    }))
}

/// Evaluate an internal element-access node, respecting every declared dimension/bound.
pub(crate) fn array_element(args: &[Cell]) -> Result<Cell> {
    let base = args
        .first()
        .ok_or_else(|| error("invalid.subscript", "Missing array operand"))?;
    let SqlType::Array(element) = base.sql_type() else {
        return Err(error("sql.42804", "Subscripted value is not an array"));
    };
    if args.iter().any(Cell::is_null) {
        return Ok(Cell::Null(*element));
    }
    let Cell::Scalar(ScalarValue {
        key: Key::Array(array),
        ..
    }) = base
    else {
        unreachable!()
    };
    if args.len() - 1 != array.dimensions.len() {
        return Ok(Cell::Null(*element));
    }
    let Some(offset) = array_offset(array, &args[1..])? else {
        return Ok(Cell::Null(*element));
    };
    Ok(array
        .values
        .get(offset)
        .cloned()
        .unwrap_or(Cell::Null(*element)))
}

/// Compute a row-major offset only when every index lies inside its actual dimension.
fn array_offset(array: &super::arrays::ArrayValue, indices: &[Cell]) -> Result<Option<usize>> {
    let mut offset = 0usize;
    for ((index, width), lower) in indices.iter().zip(&array.dimensions).zip(&array.lower) {
        let Cell::Int32(index) = index else {
            return Err(error("invalid.subscript", "Index must be int32"));
        };
        let relative = i64::from(*index) - i64::from(*lower);
        if relative < 0 || relative >= *width as i64 {
            return Ok(None);
        }
        offset = offset * width + relative as usize;
    }
    Ok(Some(offset))
}

/// Read a fixed bit string from its leftmost zero-based position with SQL range errors.
pub(crate) fn get_bit(args: &[Cell]) -> Result<Cell> {
    if args.len() != 2 {
        return Err(error("sql.42883", "get_bit requires two arguments"));
    }
    if args.iter().any(Cell::is_null) {
        return Ok(Cell::Null(SqlType::Int32));
    }
    let Cell::Int32(index) = args[1] else {
        return Err(error("sql.42883", "Bit index must be integer"));
    };
    let Cell::Scalar(ScalarValue {
        key: Key::Bits(bits),
        ..
    }) = &args[0]
    else {
        return Err(error(
            "unsupported.get_bit_family",
            "Only fixed bit strings are admitted",
        ));
    };
    let value = usize::try_from(index)
        .ok()
        .and_then(|index| bits.get(index));
    value
        .map(|value| Cell::Int32(i32::from(*value)))
        .ok_or_else(|| error("sql.2202E", "Bit index out of valid range"))
}

//! Structural JSONB extraction keeps missing values, JSON null and SQL NULL distinct.
use super::{json_value::JsonValue, rich::Key, ScalarValue};
use crate::{json::error, Cell, Result, SqlType};

/// Validate the compiler-owned operation before handling SQL NULL or structural absence.
pub(super) fn evaluate(args: &[Cell]) -> Result<Cell> {
    let [base, key, Cell::Text(mode)] = args else {
        return Err(error(
            "invalid.json_access",
            "Expected base, selector and extraction mode",
        ));
    };
    let ty = signature(base, key, mode)?;
    if base.is_null() || key.is_null() {
        return Ok(Cell::Null(ty));
    }
    let Cell::Scalar(ScalarValue {
        key: Key::Json(value),
        ..
    }) = base
    else {
        return Err(error(
            "invalid.json_access",
            "Expected validated JSONB tree",
        ));
    };
    let selected = if mode.starts_with("path") {
        path(value, key)?
    } else {
        field(value, key)
    };
    output(selected, &ty)
}

/// A typed selector cannot silently change between an object field and an array index.
fn signature(base: &Cell, key: &Cell, mode: &str) -> Result<SqlType> {
    let valid = match mode {
        "field" | "field_text" => matches!(key.sql_type(), SqlType::Text | SqlType::Int32),
        "path" | "path_text" => key.sql_type() == SqlType::Array(Box::new(SqlType::Text)),
        _ => false,
    };
    if base.sql_type() != SqlType::Jsonb || !valid {
        return Err(error(
            "invalid.json_access",
            "Malformed internal extraction signature",
        ));
    }
    Ok(if mode.ends_with("_text") {
        SqlType::Text
    } else {
        SqlType::Jsonb
    })
}

/// Field operators match only their own structural overload, including negative array positions.
fn field<'a>(value: &'a JsonValue, key: &Cell) -> Option<&'a JsonValue> {
    match (value, key) {
        (JsonValue::Object(fields), Cell::Text(key)) => fields
            .iter()
            .find(|entry| entry.0 == *key)
            .map(|entry| &entry.1),
        (JsonValue::Array(values), Cell::Int32(index)) => element(values, *index),
        (JsonValue::Object(_), Cell::Int32(_)) => None,
        (scalar, Cell::Int32(0 | -1)) => Some(scalar),
        _ => None,
    }
}

/// Array indexes are zero-based; negative positions count from the end without saturation.
fn element(values: &[JsonValue], index: i32) -> Option<&JsonValue> {
    let index = if index < 0 {
        values.len() as i64 + i64::from(index)
    } else {
        i64::from(index)
    };
    usize::try_from(index)
        .ok()
        .and_then(|index| values.get(index))
}

/// Each path component follows the current container, and missing/wrong shapes produce SQL NULL.
fn path<'a>(mut value: &'a JsonValue, keys: &Cell) -> Result<Option<&'a JsonValue>> {
    for key in super::array_elements(keys)?.unwrap_or_default() {
        let Cell::Text(key) = key else {
            return Ok(None);
        };
        let next = match value {
            JsonValue::Object(fields) => fields
                .iter()
                .find(|entry| entry.0 == key)
                .map(|entry| &entry.1),
            JsonValue::Array(values) => key
                .trim_start_matches(|c: char| c.is_ascii_whitespace())
                .parse::<i32>()
                .ok()
                .and_then(|index| element(values, index)),
            _ => None,
        };
        let Some(next) = next else { return Ok(None) };
        value = next;
    }
    Ok(Some(value))
}

/// Text extraction unquotes strings and converts JSON null to SQL NULL; JSON extraction preserves it.
fn output(value: Option<&JsonValue>, ty: &SqlType) -> Result<Cell> {
    let Some(value) = value else {
        return Ok(Cell::Null(ty.clone()));
    };
    if *ty == SqlType::Text {
        return Ok(match value {
            JsonValue::Null => Cell::Null(SqlType::Text),
            JsonValue::String(text) => Cell::Text(text.clone()),
            value => Cell::Text(value.text()),
        });
    }
    super::input(ty, &value.text())
}

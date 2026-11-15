//! JSONB path updates visit existing parents and only create a missing final slot.
use super::{json_value::JsonValue, rich::Key, ScalarValue};
use crate::json::error;
use crate::{Cell, Result, SqlType};

/// Apply strict jsonb_set after validating its path dimensions and root shape.
pub(crate) fn set(args: &[Cell]) -> Result<Cell> {
    if args.iter().any(Cell::is_null) {
        return Ok(Cell::Null(SqlType::Jsonb));
    }
    let target = json(&args[0])?;
    let path = super::array_functions::array(&args[1])?;
    if path.dimensions.len() > 1 {
        return Err(error("sql.2202E", "Wrong number of array subscripts"));
    }
    if !matches!(target, JsonValue::Object(_) | JsonValue::Array(_)) {
        return Err(error("sql.22023", "Cannot set path in scalar"));
    }
    let create = args
        .get(3)
        .map(|arg| *arg == Cell::Bool(true))
        .unwrap_or(true);
    let empty = matches!(target, JsonValue::Object(values) if values.is_empty())
        || matches!(target, JsonValue::Array(values) if values.is_empty());
    if path.values.is_empty() || (empty && !create) {
        return Ok(args[0].clone());
    }
    let result = update(target, &path.values, json(&args[2])?, create)?;
    super::input(&SqlType::Jsonb, &result.text())
}

/// Borrow JSONB semantics rather than round-tripping through lossy generic JSON numbers.
fn json(value: &Cell) -> Result<&JsonValue> {
    match value {
        Cell::Scalar(ScalarValue {
            key: Key::Json(value),
            ..
        }) => Ok(value),
        _ => Err(error("invalid.jsonb_operand", "Expected JSONB")),
    }
}

/// Validate each reached path component lazily; missing parents skip unreachable suffixes.
fn update(target: &JsonValue, path: &[Cell], value: &JsonValue, create: bool) -> Result<JsonValue> {
    let Some(first) = path.first() else {
        return Ok(target.clone());
    };
    if first.is_null() {
        return Err(error("sql.22004", "JSONB path element must not be NULL"));
    }
    let key = super::output(first)?;
    match target {
        JsonValue::Object(fields) => object(fields, &key, &path[1..], value, create),
        JsonValue::Array(values) => array(values, &key, &path[1..], value, create),
        value => Ok(value.clone()),
    }
}

/// An object update preserves other fields and inserts only at the final path component.
fn object(
    fields: &[(String, JsonValue)],
    key: &str,
    rest: &[Cell],
    value: &JsonValue,
    create: bool,
) -> Result<JsonValue> {
    let mut fields = fields.to_vec();
    if let Some((_, current)) = fields.iter_mut().find(|entry| entry.0 == key) {
        *current = if rest.is_empty() {
            value.clone()
        } else {
            update(current, rest, value, create)?
        };
    } else if rest.is_empty() && create {
        fields.push((key.into(), value.clone()));
    }
    Ok(JsonValue::Object(fields))
}

/// Array path indexes are signed decimal int32; out-of-range final creation prepends/appends.
fn array(
    values: &[JsonValue],
    key: &str,
    rest: &[Cell],
    value: &JsonValue,
    create: bool,
) -> Result<JsonValue> {
    let index = array_index(key, values.len())?;
    let mut values = values.to_vec();
    if index >= 0 && index < values.len() as i64 {
        let current = &mut values[index as usize];
        *current = if rest.is_empty() {
            value.clone()
        } else {
            update(current, rest, value, create)?
        };
    } else if rest.is_empty() && create {
        if index < 0 {
            values.insert(0, value.clone());
        } else {
            values.push(value.clone());
        }
    }
    Ok(JsonValue::Array(values))
}

/// Decode a decimal int32 path component and resolve negative positions from the end.
fn array_index(key: &str, length: usize) -> Result<i64> {
    let index = key
        .trim_start_matches(|c: char| c.is_ascii_whitespace())
        .parse::<i32>()
        .map_err(|_| error("sql.22P02", "JSONB path array index is not an integer"))?;
    Ok(if index < 0 {
        length as i64 + i64::from(index)
    } else {
        i64::from(index)
    })
}

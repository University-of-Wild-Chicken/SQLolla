//! Nullable text/array conversions and JSONB top-level type inspection.
use super::{json_value::JsonValue, rich::Key, ScalarValue};
use crate::json::error;
use crate::{Cell, Result, SqlType};

/// Resolve admitted signatures before NULL handling can bypass an invalid overload.
pub(crate) fn signature(name: &str, args: &[SqlType]) -> Result<SqlType> {
    match (name, args) {
        ("string_to_array", [SqlType::Text, SqlType::Text])
        | ("string_to_array", [SqlType::Text, SqlType::Text, SqlType::Text]) => {
            Ok(SqlType::Array(Box::new(SqlType::Text)))
        }
        ("array_to_string", [SqlType::Array(_), SqlType::Text])
        | ("array_to_string", [SqlType::Array(_), SqlType::Text, SqlType::Text])
        | ("jsonb_typeof", [SqlType::Jsonb]) => Ok(SqlType::Text),
        _ => Err(error(
            "sql.42883",
            "No string/collection function with these argument types",
        )),
    }
}

/// Convert nullable text without confusing NULL and empty delimiters or dropping NULL fields.
pub(crate) fn split(args: &[Cell]) -> Result<Cell> {
    let ty = SqlType::Array(Box::new(SqlType::Text));
    if args[0].is_null() {
        return Ok(Cell::Null(ty));
    }
    let text = super::output(&args[0])?;
    let marker = args
        .get(2)
        .filter(|value| !value.is_null())
        .map(super::output)
        .transpose()?;
    let fields = fields(&text, &args[1])?;
    if fields.len() > 65536 {
        return Err(error("resource.array", "Split field budget exceeded"));
    }
    let values = fields
        .into_iter()
        .map(|field| {
            if marker.as_ref() == Some(&field) {
                Cell::Null(SqlType::Text)
            } else {
                Cell::Text(field)
            }
        })
        .collect::<Vec<_>>();
    super::array_constructor(&SqlType::Text, &values)
}

/// Exact delimiter matching preserves adjacent/trailing empty fields and Unicode characters.
fn fields(text: &str, delimiter: &Cell) -> Result<Vec<String>> {
    if text.is_empty() {
        return Ok(Vec::new());
    }
    if delimiter.is_null() {
        return collect_fields(text.chars().map(|value| value.to_string()));
    }
    let delimiter = super::output(delimiter)?;
    if delimiter.is_empty() {
        return Ok(vec![text.into()]);
    }
    collect_fields(text.split(&delimiter).map(str::to_owned))
}

/// Reject an oversized split before allocating an unbounded vector of individual fields.
fn collect_fields(values: impl Iterator<Item = String>) -> Result<Vec<String>> {
    let values: Vec<_> = values.take(65537).collect();
    if values.len() > 65536 {
        return Err(error("resource.array", "Split field budget exceeded"));
    }
    Ok(values)
}

/// Flatten arrays in row-major order, skipping NULL elements unless a replacement is supplied.
pub(crate) fn join(args: &[Cell]) -> Result<Cell> {
    if args[0].is_null() || args[1].is_null() {
        return Ok(Cell::Null(SqlType::Text));
    }
    let array = super::array_functions::array(&args[0])?;
    let marker = args
        .get(2)
        .filter(|value| !value.is_null())
        .map(super::output)
        .transpose()?;
    let mut fields = Vec::new();
    for value in &array.values {
        if value.is_null() {
            if let Some(marker) = &marker {
                fields.push(marker.clone());
            }
        } else {
            fields.push(element_output(value)?);
        }
    }
    super::input(&SqlType::Text, &fields.join(&super::output(&args[1])?))
}

/// Array string conversion uses PostgreSQL's type output function, not Boolean-to-text casts.
fn element_output(value: &Cell) -> Result<String> {
    match value {
        Cell::Bool(value) => Ok(if *value { "t" } else { "f" }.into()),
        _ => super::output(value),
    }
}

/// Distinguish SQL NULL from JSON null while inspecting only the top-level JSON value.
pub(crate) fn json_type(value: &Cell) -> Result<Cell> {
    if value.is_null() {
        return Ok(Cell::Null(SqlType::Text));
    }
    let Cell::Scalar(ScalarValue {
        key: Key::Json(json),
        ..
    }) = value
    else {
        return Err(error("sql.42883", "jsonb_typeof requires JSONB"));
    };
    Ok(Cell::Text(
        match json {
            JsonValue::Null => "null",
            JsonValue::Bool(_) => "boolean",
            JsonValue::Number(_) => "number",
            JsonValue::String(_) => "string",
            JsonValue::Array(_) => "array",
            JsonValue::Object(_) => "object",
        }
        .into(),
    ))
}

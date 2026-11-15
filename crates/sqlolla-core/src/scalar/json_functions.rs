//! Plain JSON preserves spelling; constructors explicitly convert SQL values and NULLs.
use super::{rich::Key, ScalarValue};
use crate::json::error;
use crate::{Cell, Result, SqlType};

/// Retain exact plain-JSON source after validating the admitted Unicode input profile.
pub(crate) fn text_key(value: &str) -> Result<(Key, String)> {
    serde_json::from_str::<serde_json::Value>(value).map_err(|_| {
        error(
            "unsupported.json_input_profile",
            "JSON syntax or unpaired Unicode escapes exceed the checked text profile",
        )
    })?;
    Ok((Key::JsonText(value.into()), value.into()))
}

/// Build an object or array without strict-NULL propagation; JSONB conversion is explicit.
pub(crate) fn construct(name: &str, args: &[Cell]) -> Result<Cell> {
    let text = if name.ends_with("object") {
        object(args, name.starts_with("jsonb_"))?
    } else {
        "[".to_owned()
            + &args
                .iter()
                .map(convert)
                .collect::<Result<Vec<_>>>()?
                .join(", ")
            + "]"
    };
    super::input(
        &if name.starts_with("jsonb_") {
            SqlType::Jsonb
        } else {
            SqlType::Json
        },
        &text,
    )
}

/// Plain JSON objects preserve input key order and repeated keys; NULL keys are SQL errors.
fn object(args: &[Cell], binary: bool) -> Result<String> {
    if !args.len().is_multiple_of(2) {
        return Err(error(
            "sql.22023",
            "Object constructor requires key/value pairs",
        ));
    }
    let mut fields = Vec::new();
    for pair in args.as_chunks::<2>().0 {
        if pair[0].is_null() {
            return Err(error(
                if binary { "sql.22023" } else { "sql.22004" },
                "JSON object key must not be NULL",
            ));
        }
        if matches!(
            pair[0].sql_type(),
            SqlType::Array(_) | SqlType::Json | SqlType::Jsonb
        ) {
            return Err(error("sql.22023", "JSON object keys must be scalar values"));
        }
        let key = quote(&super::output(&pair[0])?)?;
        fields.push(key + " : " + &convert(&pair[1])?);
    }
    Ok("{".to_owned() + &fields.join(", ") + "}")
}

/// Convert SQL primitive values, JSON and arrays without interpreting strings as JSON syntax.
fn convert(value: &Cell) -> Result<String> {
    if value.is_null() {
        return Ok("null".into());
    }
    let text = super::output(value)?;
    Ok(match value.sql_type() {
        SqlType::Json | SqlType::Jsonb => text,
        SqlType::Int16 | SqlType::Int32 | SqlType::Int64 | SqlType::Numeric { .. } => text,
        SqlType::Float64 if !matches!(text.as_str(), "NaN" | "Infinity" | "-Infinity") => text,
        SqlType::Bool => text,
        SqlType::Array(_) => array(value)?,
        SqlType::Timestamp | SqlType::Timestamptz => {
            quote(&timestamp(&text, value.sql_type() == SqlType::Timestamptz))?
        }
        _ => quote(&text)?,
    })
}

/// JSON timestamps use ISO T and UTC +00:00 while ordinary SQL transport uses a space.
fn timestamp(value: &str, zoned: bool) -> String {
    let mut value = value.replacen(' ', "T", 1);
    if zoned && value.ends_with("+00") {
        value.push_str(":00");
    }
    value
}

/// Encode text with the JSON escaping rules and no locale-dependent formatting.
fn quote(value: &str) -> Result<String> {
    serde_json::to_string(value)
        .map_err(|_| error("invalid.json_encoding", "Cannot encode JSON text"))
}

/// Preserve multidimensional array structure instead of flattening it into one dimension.
fn array(value: &Cell) -> Result<String> {
    let Cell::Scalar(ScalarValue {
        key: Key::Array(array),
        ..
    }) = value
    else {
        unreachable!()
    };
    let mut cursor = 0;
    array_dimension(&array.dimensions, &array.values, &mut cursor)
}

/// Recursively render a validated rectangular array in row-major order.
fn array_dimension(dimensions: &[usize], values: &[Cell], cursor: &mut usize) -> Result<String> {
    let Some(width) = dimensions.first() else {
        return Ok("[]".into());
    };
    let mut output = Vec::new();
    for _ in 0..*width {
        output.push(if dimensions.len() == 1 {
            let value = convert(&values[*cursor])?;
            *cursor += 1;
            value
        } else {
            array_dimension(&dimensions[1..], values, cursor)?
        });
    }
    Ok("[".to_owned() + &output.join(",") + "]")
}

//! Explicit input profiles and canonical validation for every admitted scalar family.
use super::{
    arrays::ArrayValue,
    json_value::JsonValue,
    rich::{Key, ScalarValue},
    temporal, Decimal, PgFloat,
};
use crate::json::error;
use crate::{Cell, Result, SqlType};

/// Convert PostgreSQL text transport into a validated typed value.
pub fn input(ty: &SqlType, value: &str) -> Result<Cell> {
    if value.len() > crate::MAX_TEXT_BYTES {
        return Err(error(
            "resource.scalar_bytes",
            "Scalar input exceeds byte budget",
        ));
    }
    match ty {
        SqlType::Int32 => parse_integer(value).and_then(|value| {
            i32::try_from(value)
                .map(Cell::Int32)
                .map_err(|_| error("sql.22003", "Integer out of range"))
        }),
        SqlType::Int64 => parse_integer(value).map(Cell::Int64),
        SqlType::Bool => boolean(value).map(Cell::Bool),
        SqlType::Text if !value.contains('\0') => Ok(Cell::Text(value.into())),
        SqlType::Text => Err(error("sql.22021", "Text cannot contain NUL")),
        _ => rich(ty, value).map(Cell::Scalar),
    }
}

/// Construct semantic keys after enforcing actual type and modifier constraints.
pub(crate) fn rich(ty: &SqlType, value: &str) -> Result<ScalarValue> {
    if value.len() > 1_048_576 {
        return Err(error(
            "resource.scalar_bytes",
            "Scalar input exceeds byte budget",
        ));
    }
    let (key, value) = parse_key(ty, value)?;
    Ok(ScalarValue {
        ty: ty.clone(),
        value,
        key,
    })
}

/// Dispatch rich input by semantic family without mixing validation algorithms.
fn parse_key(ty: &SqlType, value: &str) -> Result<(Key, String)> {
    match ty {
        SqlType::Int16 | SqlType::Numeric { .. } | SqlType::Float64 => number_key(ty, value),
        SqlType::Varchar { length } => character(value, *length, false),
        SqlType::Char { length } => character(value, Some(*length), true),
        SqlType::Bpchar => character(value, None, true),
        SqlType::Name => name_key(value),
        SqlType::Xid => super::xid::input_key(value),
        SqlType::Oid | SqlType::Regclass | SqlType::InternalChar => {
            super::catalog::input_key(ty, value)
        }
        SqlType::Bytea => byte_key(value),
        SqlType::Date | SqlType::Timestamp | SqlType::Timestamptz | SqlType::Interval => {
            temporal_key(ty, value)
        }
        SqlType::Enum { name, labels } => enumeration(name, labels, value),
        SqlType::Array(element) => array_key(element, value),
        SqlType::Jsonb => json_key(value),
        SqlType::Json => super::json_functions::text_key(value),
        SqlType::Void => void_key(value),
        SqlType::Bit { length } => bits(value, *length),
        _ => Err(error(
            "invalid.scalar_transport",
            "Use the dedicated primitive Cell variant",
        )),
    }
}

/// Void result transport has one canonical non-NULL representation and no general input function.
fn void_key(value: &str) -> Result<(Key, String)> {
    if value.is_empty() {
        Ok((Key::Void, String::new()))
    } else {
        Err(error(
            "unsupported.void_input",
            "Only canonical void result transport is admitted",
        ))
    }
}

/// Validate exact and approximate rich numeric families.
fn number_key(ty: &SqlType, value: &str) -> Result<(Key, String)> {
    Ok(match ty {
        SqlType::Int16 => {
            let value = i16::try_from(parse_integer(value)?)
                .map_err(|_| error("sql.22003", "Smallint out of range"))?;
            (Key::Integer(i64::from(value)), value.to_string())
        }
        SqlType::Numeric { precision, scale } => {
            let value = Decimal::parse(value)?.constrained(*precision, *scale)?;
            (Key::Numeric(value.clone()), value.to_string())
        }
        SqlType::Float64 => {
            let value = PgFloat::parse(value)?;
            (Key::Float(value), value.text())
        }
        _ => return Err(error("invalid.numeric_type", "Numeric family required")),
    })
}

/// Canonicalize date, timestamp and calendar interval input.
fn temporal_key(ty: &SqlType, value: &str) -> Result<(Key, String)> {
    Ok(match ty {
        SqlType::Date => {
            let days = temporal::date(value)?;
            (Key::Date(days), temporal::display(days, ty))
        }
        SqlType::Timestamp | SqlType::Timestamptz => timestamp(ty, value)?,
        SqlType::Interval => {
            let interval = super::interval_parse::parse(value, super::IntervalFields::Full)?;
            let text = super::interval_format::format(&interval);
            (Key::Interval(interval), text)
        }
        _ => return Err(error("invalid.temporal_type", "Temporal family required")),
    })
}

/// Preserve every byte while producing lowercase hexadecimal text transport.
fn byte_key(value: &str) -> Result<(Key, String)> {
    let bytes = bytes(value)?;
    let text = "\\x".to_owned()
        + &bytes
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect::<String>();
    Ok((Key::Bytes(bytes), text))
}
/// Validate dimensions and derive canonical array transport.
fn array_key(element: &SqlType, value: &str) -> Result<(Key, String)> {
    let array = ArrayValue::parse(element, value)?;
    let text = array.text()?;
    Ok((Key::Array(array), text))
}
/// Validate JSONB and retain both its key semantics and canonical display.
fn json_key(value: &str) -> Result<(Key, String)> {
    let json = JsonValue::parse(value)?;
    let text = json.text();
    Ok((Key::Json(json), text))
}

/// Clip PostgreSQL name storage at a complete UTF-8 character before byte 64.
fn name_key(value: &str) -> Result<(Key, String)> {
    if value.contains('\0') {
        return Err(error("sql.22021", "Name input contains NUL"));
    }
    let mut length = value.len().min(63);
    while !value.is_char_boundary(length) {
        length -= 1;
    }
    let value = value[..length].to_owned();
    Ok((Key::Text(value.clone()), value))
}

/// Parse signed integer input, including PostgreSQL16 underscore and radix forms.
fn parse_integer(value: &str) -> Result<i64> {
    let value = value.trim_matches(|c: char| c.is_ascii_whitespace());
    let negative = value.starts_with('-');
    let magnitude = value.strip_prefix(['+', '-']).unwrap_or(value);
    let (radix, digits) = radix(magnitude);
    if digits.is_empty()
        || digits.ends_with('_')
        || digits.contains("__")
        || (radix == 10 && digits.starts_with('_'))
    {
        return Err(error("sql.22P02", "Invalid integer input"));
    }
    let parsed = i128::from_str_radix(&digits.replace('_', ""), radix).map_err(integer_error)?;
    i64::try_from(if negative { -parsed } else { parsed })
        .map_err(|_| error("sql.22003", "Integer out of range"))
}

/// Recognize PostgreSQL16 explicit integer radix prefixes.
fn radix(magnitude: &str) -> (u32, &str) {
    if magnitude.starts_with("0x") || magnitude.starts_with("0X") {
        (16, &magnitude[2..])
    } else if magnitude.starts_with("0o") || magnitude.starts_with("0O") {
        (8, &magnitude[2..])
    } else if magnitude.starts_with("0b") || magnitude.starts_with("0B") {
        (2, &magnitude[2..])
    } else {
        (10, magnitude)
    }
}

/// Distinguish malformed integer syntax from exact host-wide overflow.
fn integer_error(failure: std::num::ParseIntError) -> crate::Error {
    error(
        if matches!(
            failure.kind(),
            std::num::IntErrorKind::PosOverflow | std::num::IntErrorKind::NegOverflow
        ) {
            "sql.22003"
        } else {
            "sql.22P02"
        },
        "Invalid integer input",
    )
}

/// PostgreSQL Boolean input accepts unambiguous ASCII-case-insensitive prefixes.
fn boolean(value: &str) -> Result<bool> {
    let value = value
        .trim_matches(|c: char| c.is_ascii_whitespace())
        .to_ascii_lowercase();
    if value == "1" {
        return Ok(true);
    }
    if value == "0" {
        return Ok(false);
    }
    let yes = ["true", "yes", "on"]
        .iter()
        .any(|word| word.starts_with(&value));
    let no = ["false", "no", "off"]
        .iter()
        .any(|word| word.starts_with(&value));
    if !value.is_empty() && yes != no {
        Ok(yes)
    } else {
        Err(error("sql.22P02", "Invalid Boolean input"))
    }
}

/// Enforce character counts and bpchar's space-insensitive comparison key.
fn character(value: &str, length: Option<u32>, padded: bool) -> Result<(Key, String)> {
    if value.contains('\0') {
        return Err(error("sql.22021", "Character input contains NUL"));
    }
    let mut characters: Vec<_> = value.chars().collect();
    if let Some(length) = length {
        limit_characters(&mut characters, length, padded)?;
    }
    let value: String = characters.into_iter().collect();
    let key = if padded {
        value.trim_end_matches(' ').to_string()
    } else {
        value.clone()
    };
    Ok((Key::Text(key), value))
}

/// Enforce character count without allocating an attacker-controlled padding length.
fn limit_characters(characters: &mut Vec<char>, length: u32, padded: bool) -> Result<()> {
    if length > 1_048_576 {
        return Err(error(
            "resource.character_width",
            "Character width exceeds scalar budget",
        ));
    }
    let length = length as usize;
    if characters
        .get(length..)
        .is_some_and(|tail| tail.iter().any(|character| *character != ' '))
    {
        return Err(error(
            "sql.22001",
            "Character value exceeds declared length",
        ));
    }
    characters.truncate(length);
    if padded {
        characters.resize(length, ' ');
    }
    Ok(())
}

/// Parse bytea hex or PostgreSQL's historical backslash/octal format.
fn bytes(value: &str) -> Result<Vec<u8>> {
    if let Some(hex) = value.strip_prefix("\\x") {
        return hex_bytes(hex);
    }
    let bytes = value.as_bytes();
    let mut result = Vec::new();
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] != b'\\' {
            result.push(bytes[index]);
            index += 1;
            continue;
        }
        if bytes.get(index + 1) == Some(&b'\\') {
            result.push(b'\\');
            index += 2;
            continue;
        }
        let part = bytes
            .get(index + 1..index + 4)
            .ok_or_else(|| error("sql.22P02", "Invalid bytea escape"))?;
        let octal = std::str::from_utf8(part)
            .ok()
            .and_then(|text| u8::from_str_radix(text, 8).ok())
            .ok_or_else(|| error("sql.22P02", "Invalid bytea octal escape"))?;
        result.push(octal);
        index += 4;
    }
    Ok(result)
}

/// Hex input permits whitespace between complete bytes, never between their nibbles.
fn hex_bytes(hex: &str) -> Result<Vec<u8>> {
    let bytes = hex.as_bytes();
    let mut index = 0;
    let mut result = Vec::new();
    while index < bytes.len() {
        if bytes[index].is_ascii_whitespace() {
            index += 1;
            continue;
        }
        let pair = bytes
            .get(index..index + 2)
            .ok_or_else(|| error("sql.22023", "Odd bytea hexadecimal length"))?;
        let byte = std::str::from_utf8(pair)
            .ok()
            .and_then(|pair| u8::from_str_radix(pair, 16).ok())
            .ok_or_else(|| error("sql.22023", "Invalid bytea hexadecimal digit"))?;
        result.push(byte);
        index += 2;
    }
    Ok(result)
}

/// Retain nominal enum identity and validate declaration-order membership.
fn enumeration(name: &str, labels: &[String], value: &str) -> Result<(Key, String)> {
    let ordinal = labels
        .iter()
        .position(|label| label == value)
        .ok_or_else(|| error("sql.22P02", "Unknown enum label"))?;
    Ok((Key::Enum(name.into(), ordinal), value.into()))
}
/// Canonicalize finite date/time values independently of input timezone spelling.
fn timestamp(ty: &SqlType, value: &str) -> Result<(Key, String)> {
    let micros = temporal::timestamp(value, *ty == SqlType::Timestamptz)?;
    let key = if *ty == SqlType::Timestamptz {
        Key::Timestamptz(micros)
    } else {
        Key::Timestamp(micros)
    };
    Ok((key, temporal::display(micros, ty)))
}
/// Validate a fixed-width bit value without truncation or integer reinterpretation.
fn bits(value: &str, length: u32) -> Result<(Key, String)> {
    if !value.bytes().all(|byte| matches!(byte, b'0' | b'1')) {
        return Err(error("sql.22P02", "Invalid bit input"));
    }
    if value.len() != length as usize {
        return Err(error("sql.22026", "Bit string length mismatch"));
    }
    Ok((
        Key::Bits(value.bytes().map(|byte| byte == b'1').collect()),
        value.into(),
    ))
}

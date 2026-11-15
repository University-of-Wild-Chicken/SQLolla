//! Nominal catalog scalar input with explicit unsigned OID and single-byte char semantics.
use super::rich::Key;
use crate::json::error;
use crate::{Cell, Result, SqlType};

/// Validate catalog scalar transport without flattening either nominal family to text.
pub(crate) fn input_key(ty: &SqlType, text: &str) -> Result<(Key, String)> {
    match ty {
        SqlType::Oid | SqlType::Regclass => {
            if *ty == SqlType::Regclass && !text.bytes().all(|value| value.is_ascii_digit()) {
                return Err(error(
                    "unsupported.regclass_input",
                    "Named regclass input needs catalog context",
                ));
            }
            let value = oid(text)?;
            Ok((Key::Integer(i64::from(value)), value.to_string()))
        }
        SqlType::InternalChar => {
            let value = internal_char(text);
            let display = match value {
                0 => String::new(),
                1..=127 => char::from(value).to_string(),
                _ => format!("\\{value:03o}"),
            };
            Ok((Key::Integer(i64::from(value)), display))
        }
        _ => Err(error(
            "invalid.catalog_type",
            "Catalog scalar family required",
        )),
    }
}

/// Match the portable strtoul input profile; refuse wider negative platform-dependent cases.
pub(super) fn oid(text: &str) -> Result<u32> {
    let text = text.trim_matches(|value: char| value.is_ascii_whitespace());
    let negative = text.starts_with('-');
    let unsigned = text.strip_prefix(['+', '-']).unwrap_or(text);
    let (radix, digits) = radix_digits(unsigned);
    if overflowing_suffix(radix, digits) {
        return Err(error(
            "unsupported.catalog_overflow_profile",
            "Overflow before invalid trailing input depends on C unsigned-long width",
        ));
    }
    if digits.is_empty() || !digits.chars().all(|value| value.is_digit(radix)) {
        return Err(error("sql.22P02", "Invalid OID input"));
    }
    let value =
        u32::from_str_radix(digits, radix).map_err(|_| error("sql.22003", "OID out of range"))?;
    if negative && value > 2_147_483_648 {
        return Err(error(
            "unsupported.oid_negative_profile",
            "Wide negative OID input is platform-dependent",
        ));
    }
    Ok(if negative {
        value.wrapping_neg()
    } else {
        value
    })
}

/// Base-zero catalog input retains hexadecimal and legacy octal syntax.
fn radix_digits(unsigned: &str) -> (u32, &str) {
    if unsigned.starts_with("0x") || unsigned.starts_with("0X") {
        (16, &unsigned[2..])
    } else if unsigned.starts_with('0') && unsigned.len() > 1 {
        (8, &unsigned[1..])
    } else {
        (10, unsigned)
    }
}

/// A suffix after unsigned32 overflow has platform-dependent SQLSTATE precedence in strtoul.
fn overflowing_suffix(radix: u32, digits: &str) -> bool {
    let Some(end) = digits.find(|value: char| !value.is_digit(radix)) else {
        return false;
    };
    end != 0 && u64::from_str_radix(&digits[..end], radix).unwrap_or(u64::MAX) > u64::from(u32::MAX)
}

/// Octal escapes are one byte; other input keeps only its first UTF-8 byte as PostgreSQL does.
fn internal_char(text: &str) -> u8 {
    let bytes = text.as_bytes();
    if bytes.len() == 4
        && bytes[0] == b'\\'
        && bytes[1..].iter().all(|value| (b'0'..=b'7').contains(value))
    {
        return ((u16::from(bytes[1] - b'0') << 6)
            + (u16::from(bytes[2] - b'0') << 3)
            + u16::from(bytes[3] - b'0')) as u8;
    }
    bytes.first().copied().unwrap_or(0)
}

/// Resolve only the explicit numeric catalog casts supported by PostgreSQL's type catalog.
pub(crate) fn cast_allowed(source: &SqlType, target: &SqlType) -> bool {
    matches!(
        (source, target),
        (
            SqlType::Int16 | SqlType::Int32 | SqlType::Int64,
            SqlType::Oid | SqlType::Regclass
        ) | (
            SqlType::Oid | SqlType::Regclass,
            SqlType::Int32 | SqlType::Int64
        ) | (SqlType::Oid, SqlType::Regclass)
            | (SqlType::Regclass, SqlType::Oid)
            | (SqlType::Int32, SqlType::InternalChar)
            | (SqlType::InternalChar, SqlType::Int32)
    )
}

/// Numeric catalog casts use declared widths instead of reparsing their display spelling.
pub(crate) fn cast(value: &Cell, target: &SqlType) -> Result<Option<Cell>> {
    if !cast_allowed(&value.sql_type(), target) {
        return Ok(None);
    }
    let number = integer_value(value);
    let result = match target {
        SqlType::Oid | SqlType::Regclass => oid_cast(number, &value.sql_type(), target)?,
        SqlType::InternalChar => {
            let byte = i8::try_from(number)
                .map_err(|_| error("sql.22003", "Internal char out of range"))?;
            super::input(target, &format!("\\{:03o}", byte as u8))?
        }
        SqlType::Int32 if value.sql_type() == SqlType::InternalChar => {
            Cell::Int32(i32::from(number as i8))
        }
        SqlType::Int32 => Cell::Int32(number as i32),
        SqlType::Int64 => Cell::Int64(number),
        _ => unreachable!(),
    };
    Ok(Some(result))
}

/// Extract an integral key only after cast-family validation has established its representation.
fn integer_value(value: &Cell) -> i64 {
    match value {
        Cell::Int32(value) => i64::from(*value),
        Cell::Int64(value) => *value,
        Cell::Scalar(value) => match value.key {
            Key::Integer(value) => value,
            _ => unreachable!(),
        },
        _ => unreachable!(),
    }
}

/// Bigint-to-OID checks range, while signed32 input preserves its stored bit pattern.
fn oid_cast(number: i64, source: &SqlType, target: &SqlType) -> Result<Cell> {
    let oid = if *source == SqlType::Int64 {
        u32::try_from(number).map_err(|_| error("sql.22003", "OID out of range"))?
    } else {
        number as u32
    };
    super::input(target, &oid.to_string())
}

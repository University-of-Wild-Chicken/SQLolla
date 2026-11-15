//! Pure scalar casts, comparison and arithmetic over validated values.
use super::{input, rich::Key, temporal, typing, Decimal, PgFloat, ScalarValue};
use crate::json::error;
use crate::{Cell, Result, SqlType};
use std::cmp::Ordering;

/// Return portable PostgreSQL text transport for a non-NULL scalar.
pub fn output(value: &Cell) -> Result<String> {
    value.validate()?;
    match value {
        Cell::Int32(value) => Ok(value.to_string()),
        Cell::Int64(value) => Ok(value.to_string()),
        Cell::Bool(value) => Ok(if *value { "true" } else { "false" }.into()),
        Cell::Text(value) => Ok(value.clone()),
        Cell::Scalar(value) if value.sql_type() == &SqlType::Regclass => Err(error(
            "unsupported.regclass_display",
            "Regclass symbolic display requires a catalog rendering contract",
        )),
        Cell::Scalar(value) => Ok(value.text().into()),
        Cell::Null(_) => Err(error(
            "invalid.null_text",
            "NULL requires explicit null transport",
        )),
    }
}

/// Apply explicit SQL casts while preserving NULL type and checked numeric conversion.
pub fn cast(value: &Cell, target: &SqlType) -> Result<Cell> {
    value.validate()?;
    if !cast_allowed(&value.sql_type(), target) {
        return Err(error("sql.42846", "Cannot cast between these SQL types"));
    }
    if value.is_null() {
        return Ok(Cell::Null(target.clone()));
    }
    if value.sql_type() == *target {
        return Ok(value.clone());
    }
    if let Some(result) = super::catalog::cast(value, target)? {
        return Ok(result);
    }
    if matches!(target, SqlType::Int16 | SqlType::Int32 | SqlType::Int64) {
        return integer_cast(value, target);
    }
    if let Some(result) = array_conversion(value, target) {
        return result;
    }
    if *target == SqlType::Bool && typing::numeric(&value.sql_type()) {
        return Ok(Cell::Bool(!decimal(value)?.is_zero()));
    }
    input(target, &cast_text(value, target)?)
}

/// Recognize an array cast without mixing element conversion into scalar dispatch.
fn array_conversion(value: &Cell, target: &SqlType) -> Option<Result<Cell>> {
    match (value, target) {
        (
            Cell::Scalar(ScalarValue {
                key: Key::Array(array),
                ..
            }),
            SqlType::Array(element),
        ) => Some(array_cast(array, element, target)),
        _ => None,
    }
}

/// Cast array elements semantically while preserving dimensions and lower bounds.
fn array_cast(
    array: &super::arrays::ArrayValue,
    element: &SqlType,
    target: &SqlType,
) -> Result<Cell> {
    let mut array = array.clone();
    array.values = array
        .values
        .iter()
        .map(|value| cast(value, element))
        .collect::<Result<_>>()?;
    array.element = element.clone();
    input(target, &array.text()?)
}

/// Permit PostgreSQL cast families explicitly instead of arbitrary formatting/reparsing.
fn cast_allowed(source: &SqlType, target: &SqlType) -> bool {
    if source == target || typing::character(source) || typing::character(target) {
        return true;
    }
    if typing::numeric(source) && typing::numeric(target) {
        return true;
    }
    if super::catalog::cast_allowed(source, target) {
        return true;
    }
    match (source, target) {
        (SqlType::Json, SqlType::Jsonb) | (SqlType::Jsonb, SqlType::Json) => true,
        (SqlType::Bool, SqlType::Int32) | (SqlType::Int32, SqlType::Bool) => true,
        (
            SqlType::Date | SqlType::Timestamp | SqlType::Timestamptz,
            SqlType::Date | SqlType::Timestamp | SqlType::Timestamptz,
        ) => true,
        (SqlType::Int32 | SqlType::Int64, SqlType::Bit { .. })
        | (SqlType::Bit { .. }, SqlType::Int32 | SqlType::Int64) => true,
        (SqlType::Array(left), SqlType::Array(right)) => cast_allowed(left, right),
        _ => false,
    }
}

/// Apply only the textual adjustments associated with an allowed explicit cast.
fn cast_text(value: &Cell, target: &SqlType) -> Result<String> {
    let mut text = output(value)?;
    if matches!(value.sql_type(), SqlType::Char { .. } | SqlType::Bpchar)
        && matches!(target, SqlType::Text | SqlType::Varchar { .. })
    {
        text = text.trim_end_matches(' ').into();
    }
    if let SqlType::Varchar {
        length: Some(length),
    }
    | SqlType::Char { length } = target
    {
        text = text.chars().take(*length as usize).collect();
    }
    if let SqlType::Bit { length } = target {
        return output(&bit_cast(value, *length)?);
    }
    if *target == SqlType::Date {
        text = date_cast_text(value, text);
    }
    Ok(text)
}

/// Date casts remove only time fields after applying the UTC timezone profile.
fn date_cast_text(value: &Cell, fallback: String) -> String {
    if let Cell::Scalar(ScalarValue {
        key: Key::Timestamp(time) | Key::Timestamptz(time),
        ..
    }) = value
    {
        temporal::display(
            if matches!(*time, i64::MIN | i64::MAX) {
                *time
            } else {
                time.div_euclid(temporal::DAY)
            },
            &SqlType::Date,
        )
    } else {
        fallback
    }
}

/// Round numeric/float casts with their different PostgreSQL tie policies.
fn integer_cast(value: &Cell, target: &SqlType) -> Result<Cell> {
    let text = if let Cell::Scalar(value) = value {
        match &value.key {
            Key::Bits(bits) => bit_integer(bits, target)?.to_string(),
            Key::Numeric(value) => value.rounded(0)?.to_string(),
            Key::Float(value) if value.0.is_finite() => format!("{:.0}", value.0.round_ties_even()),
            Key::Float(_) => return Err(error("sql.22003", "Nonfinite integer cast")),
            _ => output(&Cell::Scalar(value.clone()))?,
        }
    } else if let Cell::Bool(value) = value {
        if *value { "1" } else { "0" }.into()
    } else {
        output(value)?
    };
    input(target, &text)
}

/// Interpret bit strings at integer width, retaining signed full-width high bits.
fn bit_integer(bits: &[bool], target: &SqlType) -> Result<i64> {
    let width = if *target == SqlType::Int64 { 64 } else { 32 };
    if bits.len() > width {
        return Err(error("sql.22003", "Bit string too long for integer"));
    }
    let value = bits
        .iter()
        .fold(0u64, |value, bit| (value << 1) | u64::from(*bit));
    Ok(if width == 32 && bits.len() == 32 {
        i64::from(value as i32)
    } else {
        value as i64
    })
}

/// SQL integer-to-bit uses low-order two's-complement bits; string casts pad/truncate.
fn bit_cast(value: &Cell, length: u32) -> Result<Cell> {
    if length > 65536 {
        return Err(error(
            "resource.bit_length",
            "Bit width exceeds scalar budget",
        ));
    }
    let text = match value {
        Cell::Int32(value) => integer_bits(i64::from(*value), length),
        Cell::Int64(value) => integer_bits(*value, length),
        _ => {
            let value = output(value)?;
            format!("{value:0<width$}", width = length as usize)
                .chars()
                .take(length as usize)
                .collect()
        }
    };
    input(&SqlType::Bit { length }, &text)
}
/// Render sign-extended integer bits without host shifts beyond their width.
fn integer_bits(value: i64, length: u32) -> String {
    (0..length)
        .rev()
        .map(|bit| {
            if if bit >= 64 {
                value < 0
            } else {
                (value as u64) & (1u64 << bit) != 0
            } {
                '1'
            } else {
                '0'
            }
        })
        .collect()
}

/// Assignment enforces character widths; explicit casts may truncate by SQL rules.
pub fn assignment_cast(value: &Cell, target: &SqlType) -> Result<Cell> {
    if value.is_null() {
        return Ok(Cell::Null(target.clone()));
    }
    if matches!(target, SqlType::Char { .. } | SqlType::Varchar { .. }) {
        return input(target, &output(value)?);
    }
    cast(value, target)
}

/// SQL duplicate equivalence treats two NULLs alike and numeric scales semantically.
pub fn duplicate_eq(left: &Cell, right: &Cell) -> Result<bool> {
    if left.is_null() || right.is_null() {
        return Ok(left.is_null()
            && right.is_null()
            && typing::compatible(&left.sql_type(), &right.sql_type()));
    }
    Ok(compare_sql(left, right)? == Ordering::Equal)
}

/// Compare compatible non-NULL SQL values using their actual semantic family.
pub fn compare_sql(left: &Cell, right: &Cell) -> Result<Ordering> {
    if let Some(ordering) = super::xid::mixed_comparison(left, right)? {
        return Ok(ordering);
    }
    crate::comparison::require(&left.sql_type())?;
    crate::comparison::require(&right.sql_type())?;
    if left.is_null() || right.is_null() {
        return Err(error(
            "invalid.null_comparison",
            "Caller must apply SQL NULL semantics",
        ));
    }
    let ty = typing::comparison_type(&left.sql_type(), &right.sql_type())?;
    if ty == SqlType::Bpchar {
        return Ok(output(left)?
            .trim_end_matches(' ')
            .cmp(output(right)?.trim_end_matches(' ')));
    }
    let (left, right) = (cast(left, &ty)?, cast(right, &ty)?);
    Ok(match (&left, &right) {
        (Cell::Scalar(left), Cell::Scalar(right)) => left.key.cmp(&right.key),
        _ => left.cmp(&right),
    })
}

/// Evaluate strict numeric/calendar arithmetic after operands have been evaluated.
pub fn arithmetic(op: &str, left: &Cell, right: &Cell) -> Result<Cell> {
    let ty = typing::operation_type(op, &left.sql_type(), &right.sql_type())?;
    if left.is_null() || right.is_null() {
        return Ok(Cell::Null(ty));
    }
    if matches!(op, "concat" | "||") {
        return concatenate(left, right, &ty);
    }
    if !typing::numeric(&ty) {
        return calendar(op, left, right, &ty);
    }
    match ty {
        SqlType::Int16 | SqlType::Int32 | SqlType::Int64 => {
            integer_arithmetic(op, left, right, &ty)
        }
        SqlType::Float64 => {
            let left = PgFloat::parse(&output(left)?)?;
            let right = PgFloat::parse(&output(right)?)?;
            input(&ty, &left.arithmetic(op, right)?.text())
        }
        SqlType::Numeric { .. } => input(
            &SqlType::Numeric {
                precision: None,
                scale: None,
            },
            &decimal(left)?.arithmetic(op, &decimal(right)?)?.to_string(),
        ),
        _ => unreachable!(),
    }
}

/// Strict text concatenation differs from concat(), whose NULL arguments are ignored.
fn concatenate(left: &Cell, right: &Cell, ty: &SqlType) -> Result<Cell> {
    if *ty == SqlType::Jsonb {
        return super::collections::json_concat(left, right);
    }
    if *ty == SqlType::Text {
        return input(&SqlType::Text, &(output(left)? + &output(right)?));
    }
    Err(error(
        "unsupported.concat_family",
        "Array/JSONB concatenation requires its collection rule",
    ))
}

/// Use a wider exact intermediate before checking the resolved SQL integer width.
fn integer_arithmetic(op: &str, left: &Cell, right: &Cell, ty: &SqlType) -> Result<Cell> {
    let left = output(left)?
        .parse::<i128>()
        .map_err(|_| error("sql.22003", "Integer out of range"))?;
    let right = output(right)?
        .parse::<i128>()
        .map_err(|_| error("sql.22003", "Integer out of range"))?;
    if matches!(op, "divide" | "/" | "modulo" | "%") && right == 0 {
        return Err(error("sql.22012", "Division by zero"));
    }
    let value = match op {
        "add" | "+" => left + right,
        "subtract" | "-" => left - right,
        "multiply" | "*" => left * right,
        "divide" | "/" => left / right,
        "modulo" | "%" => left % right,
        _ => return Err(error("unsupported.arithmetic", op)),
    };
    input(ty, &value.to_string())
}

/// Convert integer or exact numeric cells to exact decimal operation operands.
pub(crate) fn decimal(value: &Cell) -> Result<Decimal> {
    if let Cell::Scalar(ScalarValue {
        key: Key::Numeric(value),
        ..
    }) = value
    {
        return Ok(value.clone());
    }
    Decimal::parse(&output(value)?)
}

/// Apply calendar arithmetic under the explicitly admitted UTC session profile.
fn calendar(op: &str, left: &Cell, right: &Cell, ty: &SqlType) -> Result<Cell> {
    if left.sql_type() == SqlType::Interval && right.sql_type() == SqlType::Interval {
        return Err(error(
            "unsupported.interval_arithmetic",
            "Interval arithmetic requires its component-overflow contract",
        ));
    }
    if left.sql_type() == SqlType::Interval && matches!(op, "add" | "+") {
        return calendar(op, right, left, ty);
    }
    let interval = if let Cell::Scalar(ScalarValue {
        key: Key::Interval(value),
        ..
    }) = right
    {
        value
    } else {
        return Err(error("unsupported.temporal_operation", op));
    };
    let interval = signed_interval(op, interval)?;
    let value = match left {
        Cell::Scalar(ScalarValue {
            key: Key::Date(value) | Key::Timestamp(value) | Key::Timestamptz(value),
            ..
        }) => *value,
        _ => return Err(error("unsupported.temporal_operation", op)),
    };
    let value = temporal::add(value, &interval, left.sql_type() == SqlType::Date)?;
    input(ty, &temporal::display(value, ty))
}

/// Negate all calendar components with checked signed boundaries.
fn signed_interval(op: &str, value: &temporal::Interval) -> Result<temporal::Interval> {
    let mut value = value.clone();
    if matches!(op, "subtract" | "-") {
        value.months = value
            .months
            .checked_neg()
            .ok_or_else(|| error("sql.22015", "Interval overflow"))?;
        value.days = value
            .days
            .checked_neg()
            .ok_or_else(|| error("sql.22015", "Interval overflow"))?;
        value.micros = value
            .micros
            .checked_neg()
            .ok_or_else(|| error("sql.22015", "Interval overflow"))?;
    }
    Ok(value)
}

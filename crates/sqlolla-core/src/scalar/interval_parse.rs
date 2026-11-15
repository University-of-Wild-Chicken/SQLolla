//! Finite PostgreSQL verbose and SQL-style interval input; unsupported formats fail explicitly.
use super::{
    interval::{components, IntervalFields},
    temporal::Interval,
};
use crate::json::error;
use crate::Result;

#[derive(Clone, Copy)]
enum Unit {
    Year(i64),
    Month,
    Week,
    Day,
    Hour,
    Minute,
    Second(i64),
}

#[derive(Default)]
struct Parts {
    months: i128,
    days: i128,
    micros: i128,
    seen: u16,
}

/// Parse independently signed fields, retaining the original storage-component distinction.
pub(crate) fn parse(text: &str, fields: IntervalFields) -> Result<Interval> {
    let text = normalized_text(text)?;
    let (tokens, sign) = tokens(&text)?;
    let mut parts = Parts::default();
    let mut index = 0;
    while index < tokens.len() {
        index += parse_field(&tokens[index..], fields, &mut parts)?;
    }
    components(
        parts.months * sign,
        parts.days * sign,
        parts.micros * sign,
        "sql.22015",
    )
}

/// Bound input allocation and refuse formats outside the declared text profile.
fn normalized_text(text: &str) -> Result<String> {
    if text.len() > 1_048_576 {
        return Err(error(
            "resource.scalar_bytes",
            "Interval text exceeds byte budget",
        ));
    }
    let text = text
        .trim_matches(|c: char| c.is_ascii_whitespace())
        .to_ascii_lowercase();
    if text.starts_with('p') {
        return Err(error(
            "unsupported.interval_iso_input",
            "ISO designator input requires its separate parsing contract",
        ));
    }
    Ok(text)
}

/// Remove explicit verbose noise/sign markers before examining interval fields.
fn tokens(text: &str) -> Result<(Vec<&str>, i128)> {
    let mut tokens: Vec<_> = text.split_ascii_whitespace().collect();
    if tokens.first() == Some(&"@") {
        tokens.remove(0);
    }
    let ago = tokens.last() == Some(&"ago");
    if ago {
        tokens.pop();
    }
    if tokens.is_empty() {
        return invalid();
    }
    if tokens.len() > 64 {
        return Err(error(
            "resource.interval_fields",
            "Interval token budget exceeded",
        ));
    }
    Ok((tokens, if ago { -1 } else { 1 }))
}

/// Distinguish explicit units, a day-before-clock form and qualifier-defaulted quantities.
fn parse_field(tokens: &[&str], fields: IntervalFields, parts: &mut Parts) -> Result<usize> {
    if tokens[0].contains(':') {
        mark(parts, 8 | 16 | 32)?;
        parts.micros += clock(tokens[0], fields)?;
        return Ok(1);
    }
    if tokens[0].trim_start_matches(['+', '-']).contains('-') {
        mark(parts, 1 | 2)?;
        parts.months += year_month(tokens[0])?;
        return Ok(1);
    }
    let (unit, consumed) = if let Some(unit) = tokens.get(1).and_then(|name| unit(name)) {
        (unit, 2)
    } else if tokens.get(1).is_some_and(|value| value.contains(':')) {
        (Unit::Day, 1)
    } else {
        (default_unit(fields), 1)
    };
    let (whole, fraction) = quantity(tokens[0])?;
    let mask = if matches!(unit, Unit::Second(1_000_000)) && fraction != 0.0 {
        32 | 64 | 128
    } else {
        mask(unit)
    };
    mark(parts, mask)?;
    add(parts, unit, whole, fraction);
    Ok(consumed)
}

/// PostgreSQL rejects repeated fields rather than silently taking the last occurrence.
fn mark(parts: &mut Parts, mask: u16) -> Result<()> {
    if parts.seen & mask != 0 {
        return invalid();
    }
    parts.seen |= mask;
    Ok(())
}

/// Resolve the explicit supported unit spellings used by canonical PostgreSQL output.
fn unit(text: &str) -> Option<Unit> {
    Some(match text.trim_end_matches('s') {
        "year" | "yr" => Unit::Year(12),
        "decade" => Unit::Year(120),
        "century" | "centurie" => Unit::Year(1200),
        "millennium" | "millennia" => Unit::Year(12000),
        "mon" | "month" => Unit::Month,
        "week" => Unit::Week,
        "day" => Unit::Day,
        "hour" | "hr" => Unit::Hour,
        "minute" | "min" => Unit::Minute,
        "second" | "sec" => Unit::Second(1_000_000),
        "millisecond" | "msec" => Unit::Second(1000),
        "microsecond" | "usec" => Unit::Second(1),
        _ => return None,
    })
}

/// Unit aliases share duplicate-detection fields; subsecond units retain their distinct slots.
fn mask(unit: Unit) -> u16 {
    match unit {
        Unit::Year(120) => 512,
        Unit::Year(1200) => 1024,
        Unit::Year(12000) => 2048,
        Unit::Year(_) => 1,
        Unit::Month => 2,
        Unit::Week => 256,
        Unit::Day => 4,
        Unit::Hour => 8,
        Unit::Minute => 16,
        Unit::Second(1000) => 64,
        Unit::Second(1) => 128,
        Unit::Second(_) => 32,
    }
}

/// Use the least declared field for an unmarked literal quantity.
fn default_unit(fields: IntervalFields) -> Unit {
    match fields.least() {
        IntervalFields::Year => Unit::Year(12),
        IntervalFields::Month => Unit::Month,
        IntervalFields::Day => Unit::Day,
        IntervalFields::Hour => Unit::Hour,
        IntervalFields::Minute => Unit::Minute,
        _ => Unit::Second(1_000_000),
    }
}

/// Keep integer magnitudes exact; the fractional part follows PostgreSQL's finite double path.
fn quantity(text: &str) -> Result<(i64, f64)> {
    let (negative, integer, fraction) = quantity_parts(text)?;
    let integer = if integer.is_empty() { "0" } else { integer };
    let signed = format!("{}{integer}", if negative { "-" } else { "" });
    let whole = signed
        .parse()
        .map_err(|_| error("sql.22015", "Interval quantity out of range"))?;
    let fraction = format!("0.{fraction}")
        .parse::<f64>()
        .map_err(|_| error("sql.22007", "Invalid interval fraction"))?;
    Ok((whole, if negative { -fraction } else { fraction }))
}

/// Unsupported numeric spellings cannot be mistaken for independently established SQL errors.
fn quantity_parts(text: &str) -> Result<(bool, &str, &str)> {
    let negative = text.starts_with('-');
    let unsigned = text.strip_prefix(['+', '-']).unwrap_or(text);
    let (integer, fraction) = unsigned.split_once('.').unwrap_or((unsigned, ""));
    if (integer.is_empty() && fraction.is_empty())
        || !integer
            .bytes()
            .chain(fraction.bytes())
            .all(|byte| byte.is_ascii_digit())
    {
        return Err(error(
            "unsupported.interval_input_profile",
            "Quantity spelling is outside the admitted interval profile",
        ));
    }
    if fraction.len() > 18 {
        return Err(error(
            "unsupported.interval_fraction_precision",
            "Fractional input profile admits at most 18 digits",
        ));
    }
    Ok((negative, integer, fraction))
}

/// Convert finite fractional units into smaller components without merging stored months/days.
fn add(parts: &mut Parts, unit: Unit, whole: i64, fraction: f64) {
    match unit {
        Unit::Year(scale) => {
            parts.months += i128::from(whole) * i128::from(scale)
                + (fraction * (scale / 12) as f64 * 12.0).round_ties_even() as i128
        }
        Unit::Month => {
            parts.months += i128::from(whole);
            fractional_days(parts, fraction * 30.0);
        }
        Unit::Week => {
            parts.days += i128::from(whole) * 7;
            fractional_days(parts, fraction * 7.0);
        }
        Unit::Day => {
            parts.days += i128::from(whole);
            fractional_days(parts, fraction);
        }
        Unit::Hour => parts.micros += time_quantity(whole, fraction, 3_600_000_000),
        Unit::Minute => parts.micros += time_quantity(whole, fraction, 60_000_000),
        Unit::Second(scale) => parts.micros += time_quantity(whole, fraction, scale),
    }
}

/// Split fractional days toward zero and round only the remaining fractional time.
fn fractional_days(parts: &mut Parts, days: f64) {
    let whole = days.trunc();
    parts.days += whole as i128;
    parts.micros += literal_fraction((days - whole) * 86_400_000_000.0);
}

/// Exact integer contributions are combined with fractional units whose exact ties truncate.
fn time_quantity(whole: i64, fraction: f64, scale: i64) -> i128 {
    i128::from(whole) * i128::from(scale) + literal_fraction(fraction * scale as f64)
}

/// Match AdjustFractMicroseconds, distinct from the clock and make_interval tie rules.
fn literal_fraction(product: f64) -> i128 {
    let whole = product.trunc();
    let remainder = product - whole;
    whole as i128
        + if remainder > 0.5 {
            1
        } else if remainder < -0.5 {
            -1
        } else {
            0
        }
}

/// Parse a signed SQL clock, allowing unbounded first fields and bounded later fields.
fn clock(text: &str, fields: IntervalFields) -> Result<i128> {
    let sign = if text.starts_with('-') { -1 } else { 1 };
    let pieces: Vec<_> = text
        .strip_prefix(['+', '-'])
        .unwrap_or(text)
        .split(':')
        .collect();
    if !(2..=3).contains(&pieces.len()) {
        return invalid();
    }
    let first = pieces[0]
        .parse::<i64>()
        .map_err(|_| error("sql.22007", "Invalid interval clock"))?;
    if first < 0 {
        return invalid();
    }
    let last = pieces.len() - 1;
    let (whole, fraction) = quantity(pieces[last])?;
    let minute_second = pieces.len() == 2
        && (fields == IntervalFields::MinuteToSecond || pieces[last].contains('.'));
    let limit = if pieces.len() == 2 && !minute_second {
        59
    } else {
        60
    };
    if !(0..=limit).contains(&whole) || fraction < 0.0 {
        return Err(error("sql.22015", "Interval clock field out of range"));
    }
    Ok(clock_value(&pieces, first, whole, fraction, minute_second)? * sign)
}

/// Resolve two-field clock ambiguity without changing the meaning of a three-field clock.
fn clock_value(
    pieces: &[&str],
    first: i64,
    whole: i64,
    fraction: f64,
    minute_second: bool,
) -> Result<i128> {
    Ok(if pieces.len() == 2 && !minute_second {
        if fraction != 0.0 {
            return invalid();
        }
        i128::from(first) * 3_600_000_000 + i128::from(whole) * 60_000_000
    } else {
        let minutes = if pieces.len() == 3 {
            pieces[1]
                .parse::<i64>()
                .map_err(|_| error("sql.22007", "Invalid minutes"))?
        } else {
            first
        };
        if pieces.len() == 3 && !(0..=59).contains(&minutes) {
            return Err(error("sql.22015", "Interval minute field out of range"));
        }
        let hours = if pieces.len() == 3 { first } else { 0 };
        i128::from(hours) * 3_600_000_000
            + i128::from(minutes) * 60_000_000
            + i128::from(whole) * 1_000_000
            + (fraction * 1_000_000.0).round_ties_even() as i128
    })
}

/// SQL year-month input applies its leading sign to both fields.
fn year_month(text: &str) -> Result<i128> {
    let sign = if text.starts_with('-') { -1 } else { 1 };
    let Some((year, month)) = text
        .strip_prefix(['+', '-'])
        .unwrap_or(text)
        .split_once('-')
    else {
        return invalid();
    };
    let year = year
        .parse::<i64>()
        .map_err(|_| error("sql.22007", "Invalid year-month input"))?;
    let month = month
        .parse::<i64>()
        .map_err(|_| error("sql.22007", "Invalid year-month input"))?;
    if year < 0 || !(0..12).contains(&month) {
        return Err(error("sql.22015", "Interval year-month field out of range"));
    }
    Ok((i128::from(year) * 12 + i128::from(month)) * sign)
}

/// Malformed admitted input remains an explicit PostgreSQL date/time syntax failure.
fn invalid<T>() -> Result<T> {
    Err(error("sql.22007", "Invalid interval input syntax"))
}

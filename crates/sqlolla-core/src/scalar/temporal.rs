//! ISO Gregorian dates and UTC timestamps; no host locale or clock is consulted.
use crate::json::error;
use crate::{Result, SqlType};

pub(crate) const DAY: i64 = 86_400_000_000;

/// Calendar interval components cannot be flattened to seconds for date arithmetic.
#[derive(Clone, Debug)]
pub(crate) struct Interval {
    pub months: i32,
    pub days: i32,
    pub micros: i64,
}

impl Interval {
    /// PostgreSQL comparison treats each month as thirty days, unlike calendar addition.
    fn comparison_value(&self) -> i128 {
        (i128::from(self.months) * 30 + i128::from(self.days)) * i128::from(DAY)
            + i128::from(self.micros)
    }
}
impl PartialEq for Interval {
    /// Interval comparison does not erase the separately stored calendar components.
    fn eq(&self, right: &Self) -> bool {
        self.comparison_value() == right.comparison_value()
    }
}
impl Eq for Interval {}
impl PartialOrd for Interval {
    /// Use PostgreSQL's thirty-day-month interval comparison.
    fn partial_cmp(&self, right: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(right))
    }
}
impl Ord for Interval {
    /// Compare exact microseconds under the documented interval ordering convention.
    fn cmp(&self, right: &Self) -> std::cmp::Ordering {
        self.comparison_value().cmp(&right.comparison_value())
    }
}

/// Parse a PostgreSQL-epoch date under the explicit ISO/AD input profile.
pub(crate) fn date(value: &str) -> Result<i64> {
    if value.eq_ignore_ascii_case("infinity") {
        return Ok(i64::MAX);
    }
    if value.eq_ignore_ascii_case("-infinity") {
        return Ok(i64::MIN);
    }
    let pieces: Vec<_> = value.split('-').collect();
    if pieces.len() != 3 {
        return Err(error(
            "unsupported.date_input_profile",
            "ISO AD date required",
        ));
    }
    let year = number(pieces[0])?;
    let month = number(pieces[1])?;
    let day = number(pieces[2])?;
    if !(1..=5_874_897).contains(&year)
        || !(1..=12).contains(&month)
        || day < 1
        || day > month_length(year, month)
    {
        return Err(error("sql.22008", "Date field out of range"));
    }
    let result = days_from_civil(year, month, day);
    if result >= 2_145_031_949 {
        return Err(error("sql.22008", "Date out of range"));
    }
    Ok(result)
}

/// Convert a valid civil date to days since 2000-01-01 using Gregorian eras.
fn days_from_civil(year: i64, month: i64, day: i64) -> i64 {
    let year = year - i64::from(month <= 2);
    let era = year.div_euclid(400);
    let within = year - era * 400;
    let shifted_month = month + if month > 2 { -3 } else { 9 };
    let ordinal = (153 * shifted_month + 2) / 5 + day - 1;
    era * 146097 + within * 365 + within / 4 - within / 100 + ordinal - 730425
}

/// Recover civil fields from a PostgreSQL-epoch finite day.
pub(crate) fn civil(days: i64) -> (i64, i64, i64) {
    let shifted = days + 730425;
    let era = shifted.div_euclid(146097);
    let within = shifted - era * 146097;
    let year_of_era = (within - within / 1460 + within / 36524 - within / 146096) / 365;
    let year = year_of_era + era * 400;
    let ordinal = within - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let shifted_month = (5 * ordinal + 2) / 153;
    let day = ordinal - (153 * shifted_month + 2) / 5 + 1;
    let month = shifted_month + if shifted_month < 10 { 3 } else { -9 };
    (year + i64::from(month <= 2), month, day)
}

/// Return the Gregorian month's maximum valid day.
fn month_length(year: i64, month: i64) -> i64 {
    match month {
        4 | 6 | 9 | 11 => 30,
        2 if year % 4 == 0 && (year % 100 != 0 || year % 400 == 0) => 29,
        2 => 28,
        _ => 31,
    }
}

/// Parse local/offset timestamps to exact microseconds; timezone profile is UTC.
pub(crate) fn timestamp(value: &str, timezone: bool) -> Result<i64> {
    if value.eq_ignore_ascii_case("infinity") {
        return Ok(i64::MAX);
    }
    if value.eq_ignore_ascii_case("-infinity") {
        return Ok(i64::MIN);
    }
    let value = value.trim();
    let (day, time) = value.split_once([' ', 'T']).unwrap_or((value, "00:00:00"));
    let day = date(day)?;
    if matches!(day, i64::MIN | i64::MAX) {
        return Ok(day);
    }
    let (time, offset) = split_timezone(time.trim())?;
    let offset = if timezone { offset } else { 0 };
    let result = i128::from(day) * i128::from(DAY) + i128::from(clock(&time)?)
        - i128::from(offset) * 1_000_000;
    if !(-211813488000000000..9_223_371_331_200_000_000).contains(&result) {
        return Err(error("sql.22008", "Timestamp out of range"));
    }
    i64::try_from(result).map_err(|_| error("sql.22008", "Timestamp out of range"))
}

/// Separate ISO numeric offsets, retaining local timestamp fields when requested.
fn split_timezone(value: &str) -> Result<(String, i64)> {
    if let Some(time) = value.strip_suffix(['Z', 'z']) {
        return Ok((time.trim().into(), 0));
    }
    let position = value
        .char_indices()
        .find(|(_, c)| matches!(c, '+' | '-'))
        .map(|(position, _)| position);
    let Some(position) = position else {
        return Ok((value.into(), 0));
    };
    let (time, offset) = value.split_at(position);
    Ok((time.trim().into(), timezone_offset(offset)?))
}

/// Parse signed timezone displacement only after validating complete ASCII digits.
fn timezone_offset(offset: &str) -> Result<i64> {
    let sign = if offset.starts_with('-') { -1 } else { 1 };
    let digits = offset[1..].replace(':', "");
    if !digits.bytes().all(|byte| byte.is_ascii_digit()) {
        return Err(error("sql.22009", "Invalid timezone offset"));
    }
    if !matches!(digits.len(), 2 | 4 | 6) {
        return Err(error("sql.22009", "Invalid timezone offset"));
    }
    let hours = number(&digits[..2])?;
    let minutes = if digits.len() >= 4 {
        number(&digits[2..4])?
    } else {
        0
    };
    let seconds = if digits.len() == 6 {
        number(&digits[4..])?
    } else {
        0
    };
    if hours > 15 || minutes > 59 || seconds > 59 {
        return Err(error("sql.22009", "Timezone displacement out of range"));
    }
    Ok(sign * (hours * 3600 + minutes * 60 + seconds))
}

/// Parse an ISO clock and round fractional seconds to microsecond precision.
fn clock(value: &str) -> Result<i64> {
    let parts: Vec<_> = value.split(':').collect();
    if parts.len() != 3 {
        return Err(error("sql.22007", "Invalid ISO time"));
    }
    let hour = number(parts[0])?;
    let minute = number(parts[1])?;
    let (second, fraction) = parts[2].split_once('.').unwrap_or((parts[2], ""));
    let second = number(second)?;
    if !(0..=24).contains(&hour) || !(0..=59).contains(&minute) || !(0..=60).contains(&second) {
        return Err(error("sql.22008", "Time field out of range"));
    }
    let micros = fraction_micros(fraction)?;
    let result = (hour * 3600 + minute * 60 + second) * 1_000_000 + micros;
    if hour == 24 && result != DAY {
        return Err(error("sql.22008", "Invalid 24-hour time"));
    }
    Ok(result)
}

/// Round fractional seconds to microseconds with nearest-even ties.
fn fraction_micros(fraction: &str) -> Result<i64> {
    if !fraction.bytes().all(|byte| byte.is_ascii_digit()) {
        return Err(error("sql.22007", "Invalid fractional seconds"));
    }
    let padded = format!("{fraction:0<6}");
    let mut micros = number(&padded[..6])?;
    if fraction.len() > 6 {
        let tail = &fraction[6..];
        if tail.as_bytes()[0] > b'5'
            || (tail.as_bytes()[0] == b'5'
                && (tail[1..].bytes().any(|c| c != b'0') || micros % 2 == 1))
        {
            micros += 1;
        }
    }
    Ok(micros)
}

/// Add calendar months by clipping the day, then days and exact microseconds.
pub(crate) fn add(value: i64, interval: &Interval, date_only: bool) -> Result<i64> {
    if matches!(value, i64::MIN | i64::MAX) {
        return Ok(value);
    }
    let days = if date_only {
        value
    } else {
        value.div_euclid(DAY)
    };
    let remainder = if date_only { 0 } else { value.rem_euclid(DAY) };
    let (year, month, day) = civil(days);
    let months = year * 12 + month - 1 + i64::from(interval.months);
    let year = months.div_euclid(12);
    let month = months.rem_euclid(12) + 1;
    let day = day.min(month_length(year, month));
    let days = days_from_civil(year, month, day) + i64::from(interval.days);
    let result =
        i128::from(days) * i128::from(DAY) + i128::from(remainder) + i128::from(interval.micros);
    i64::try_from(result).map_err(|_| error("sql.22008", "Timestamp overflow"))
}

/// Format canonical date/timestamp text without consulting platform timezone data.
pub(crate) fn display(value: i64, ty: &SqlType) -> String {
    if value == i64::MAX {
        return "infinity".into();
    }
    if value == i64::MIN {
        return "-infinity".into();
    }
    let days = if *ty == SqlType::Date {
        value
    } else {
        value.div_euclid(DAY)
    };
    let (year, month, day) = civil(days);
    let date = format!("{year:04}-{month:02}-{day:02}");
    if *ty == SqlType::Date {
        return date;
    }
    let zone = if *ty == SqlType::Timestamptz {
        "+00"
    } else {
        ""
    };
    format!("{date} {}{zone}", display_clock(value.rem_euclid(DAY)))
}

/// Format a nonnegative clock component with microsecond precision.
fn display_clock(time: i64) -> String {
    let seconds = time / 1_000_000;
    let fraction = if time % 1_000_000 == 0 {
        String::new()
    } else {
        format!(".{:06}", time % 1_000_000)
            .trim_end_matches('0')
            .into()
    };
    format!(
        "{:02}:{:02}:{:02}{fraction}",
        seconds / 3600,
        seconds / 60 % 60,
        seconds % 60
    )
}

/// Parse a bounded integral date/time component.
fn number(value: &str) -> Result<i64> {
    value
        .parse()
        .map_err(|_| error("sql.22007", "Invalid date/time field"))
}

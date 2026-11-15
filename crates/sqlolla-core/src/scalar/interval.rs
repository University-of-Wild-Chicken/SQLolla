//! Checked interval qualifiers and finite numeric construction; see Intervals.lean.
use super::{interval_format, interval_parse, rich::Key, temporal::Interval, ScalarValue};
use crate::json::error;
use crate::{Cell, Result, SqlType};
use serde::{Deserialize, Serialize};

/// SQL interval ranges retain higher fields while truncating below the last field.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum IntervalFields {
    /// Unrestricted interval fields.
    #[default]
    Full,
    /// Whole years.
    Year,
    /// Months including complete years.
    Month,
    /// Calendar days and larger fields.
    Day,
    /// Whole hours and larger fields.
    Hour,
    /// Whole minutes and larger fields.
    Minute,
    /// Seconds and larger fields.
    Second,
    /// Years through months.
    YearToMonth,
    /// Days through hours.
    DayToHour,
    /// Days through minutes.
    DayToMinute,
    /// Days through seconds.
    DayToSecond,
    /// Hours through minutes.
    HourToMinute,
    /// Hours through seconds.
    HourToSecond,
    /// Minutes through seconds; two-field clock input uses minutes:seconds.
    MinuteToSecond,
}

/// Source typmods remain attached to the expression even though the value family is interval.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct IntervalQualifier {
    /// Declared range, or unrestricted fields.
    pub fields: IntervalFields,
    /// Fractional second precision admitted from zero through six.
    pub precision: Option<u8>,
}

impl IntervalFields {
    /// Resolve the default unmarked unit and the least retained field.
    pub(crate) fn least(self) -> Self {
        match self {
            Self::YearToMonth => Self::Month,
            Self::DayToHour => Self::Hour,
            Self::DayToMinute | Self::HourToMinute => Self::Minute,
            Self::Full | Self::DayToSecond | Self::HourToSecond | Self::MinuteToSecond => {
                Self::Second
            }
            value => value,
        }
    }
}

/// Parse an unknown SQL literal using its qualifier's default unit before truncation.
pub fn interval_input(text: &str, qualifier: &IntervalQualifier) -> Result<Cell> {
    let value = interval_parse::parse(text, qualifier.fields)?;
    qualify(value, qualifier).map(cell)
}

/// Cast typed text using default seconds first; existing intervals retain their stored fields.
pub fn interval_cast(value: &Cell, qualifier: &IntervalQualifier) -> Result<Cell> {
    let ty = value.sql_type();
    if ty != SqlType::Interval && !super::typing::character(&ty) {
        return Err(error("sql.42846", "Cannot cast this type to interval"));
    }
    if value.is_null() {
        validate(qualifier)?;
        return Ok(Cell::Null(SqlType::Interval));
    }
    let interval = match value {
        Cell::Scalar(ScalarValue {
            key: Key::Interval(value),
            ..
        }) => value.clone(),
        _ => interval_parse::parse(&super::output(value)?, IntervalFields::Full)?,
    };
    qualify(interval, qualifier).map(cell)
}

/// Preserve high fields and apply the declared lower precision only after parsing.
fn qualify(mut value: Interval, qualifier: &IntervalQualifier) -> Result<Interval> {
    validate(qualifier)?;
    match qualifier.fields.least() {
        IntervalFields::Year => {
            value.months = value.months / 12 * 12;
            value.days = 0;
            value.micros = 0;
        }
        IntervalFields::Month => {
            value.days = 0;
            value.micros = 0;
        }
        IntervalFields::Day => value.micros = 0,
        IntervalFields::Hour => value.micros = value.micros / 3_600_000_000 * 3_600_000_000,
        IntervalFields::Minute => value.micros = value.micros / 60_000_000 * 60_000_000,
        _ => (),
    }
    if let Some(precision) = qualifier.precision {
        let scale = 10_i128.pow(u32::from(6 - precision));
        let micros = i128::from(value.micros);
        let rounded = (micros.abs() + scale / 2) / scale * scale * micros.signum();
        value.micros = i64::try_from(rounded).map_err(|_| {
            error(
                "unsupported.interval_precision_overflow",
                "Precision rounding exceeds the finite interval profile",
            )
        })?;
    }
    Ok(value)
}

/// Out-of-profile modifiers are refusals, not fabricated PostgreSQL SQLSTATE failures.
fn validate(qualifier: &IntervalQualifier) -> Result<()> {
    if qualifier.precision.is_some_and(|value| value > 6)
        || (qualifier.precision.is_some() && qualifier.fields.least() != IntervalFields::Second)
    {
        return Err(error(
            "unsupported.interval_precision",
            "Precision 0..6 requires a SECOND range",
        ));
    }
    Ok(())
}

/// Require the real builtin signature before applying strict NULL/default behavior.
pub(crate) fn signature(args: &[SqlType]) -> Result<SqlType> {
    if args.len() > 7
        || args
            .iter()
            .take(6)
            .any(|ty| !matches!(ty, SqlType::Int16 | SqlType::Int32))
        || args.get(6).is_some_and(|ty| !super::typing::numeric(ty))
    {
        return Err(error(
            "sql.42883",
            "make_interval argument types do not match",
        ));
    }
    Ok(SqlType::Interval)
}

/// Construct named-lowered or positional arguments with exact zero defaults and explicit NULLs.
pub(crate) fn make(args: &[Cell]) -> Result<Cell> {
    signature(&args.iter().map(Cell::sql_type).collect::<Vec<_>>())?;
    if args.iter().any(Cell::is_null) {
        return Ok(Cell::Null(SqlType::Interval));
    }
    let mut values = [0_i128; 6];
    for (index, value) in args.iter().take(6).enumerate() {
        values[index] = super::output(value)?.parse().map_err(|_| {
            error(
                "invalid.interval_integer",
                "Bound integer argument required",
            )
        })?;
    }
    let seconds = args.get(6).map(seconds_micros).transpose()?.unwrap_or(0);
    components(
        values[0] * 12 + values[1],
        values[2] * 7 + values[3],
        values[4] * 3_600_000_000 + values[5] * 60_000_000 + i128::from(seconds),
        "unsupported.interval_constructor_overflow",
    )
    .map(cell)
}

/// Match finite float8 rint conversion while refusing architecture-dependent overflow domains.
fn seconds_micros(value: &Cell) -> Result<i64> {
    let value = super::cast(value, &SqlType::Float64)?;
    let Cell::Scalar(ScalarValue {
        key: Key::Float(value),
        ..
    }) = value
    else {
        unreachable!()
    };
    if !value.0.is_finite() {
        return Err(error(
            "unsupported.interval_nonfinite",
            "Nonfinite constructor seconds are outside the proved finite profile",
        ));
    }
    let scaled = value.0 * 1_000_000.0;
    if scaled.abs() > 9_007_199_254_740_992.0 {
        return Err(error(
            "unsupported.interval_float_range",
            "Constructor seconds exceed the exact integer float-product profile",
        ));
    }
    Ok(scaled.round_ties_even() as i64)
}

/// Check wide components together so no partial wrapped interval can escape.
pub(crate) fn components(months: i128, days: i128, micros: i128, code: &str) -> Result<Interval> {
    Ok(Interval {
        months: i32::try_from(months)
            .map_err(|_| error(code, "Interval month field outside admitted range"))?,
        days: i32::try_from(days)
            .map_err(|_| error(code, "Interval day field outside admitted range"))?,
        micros: i64::try_from(micros)
            .map_err(|_| error(code, "Interval time field outside admitted range"))?,
    })
}

/// Construct a validated canonical transport value without flattening calendar fields.
fn cell(value: Interval) -> Cell {
    Cell::Scalar(ScalarValue {
        ty: SqlType::Interval,
        value: interval_format::format(&value),
        key: Key::Interval(value),
    })
}

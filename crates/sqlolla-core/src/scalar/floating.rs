//! PostgreSQL float8 equality/order and checked arithmetic around IEEE binary64.
use crate::json::error;
use crate::Result;
use std::cmp::Ordering;

/// A binary64 value whose NaN and signed-zero behavior matches PostgreSQL ordering.
#[derive(Clone, Copy, Debug)]
pub struct PgFloat(pub(crate) f64);
impl PgFloat {
    /// Parse finite input or the explicit Infinity/NaN spellings.
    pub fn parse(value: &str) -> Result<Self> {
        let value = value.trim_matches(|character: char| character.is_ascii_whitespace());
        let parsed = value
            .parse::<f64>()
            .map_err(|_| error("sql.22P02", "Invalid float8 input"))?;
        let special = special_literal(value);
        if parsed.is_infinite() && !special {
            return Err(error("sql.22003", "Float8 overflow"));
        }
        if parsed == 0.0
            && value
                .split(['e', 'E'])
                .next()
                .unwrap_or("")
                .bytes()
                .any(|c| matches!(c, b'1'..=b'9'))
        {
            return Err(error("sql.22003", "Float8 underflow"));
        }
        Ok(Self(parsed))
    }
    /// Apply arithmetic with PostgreSQL overflow, underflow and zero-division errors.
    pub fn arithmetic(self, op: &str, right: Self) -> Result<Self> {
        arithmetic(self, op, right)
    }
    /// Render an exact round-trippable finite value or PostgreSQL special spelling.
    pub fn text(self) -> String {
        format_float(self.0)
    }
}

/// Recognize intentionally nonfinite float input rather than numeric overflow.
fn special_literal(value: &str) -> bool {
    [
        "inf",
        "+inf",
        "-inf",
        "infinity",
        "+infinity",
        "-infinity",
        "nan",
        "+nan",
        "-nan",
    ]
    .contains(&value.to_ascii_lowercase().as_str())
}

/// Apply checked binary64 arithmetic independently of its transport wrapper.
fn arithmetic(left: PgFloat, op: &str, right: PgFloat) -> Result<PgFloat> {
    if matches!(op, "divide" | "/") && right.0 == 0.0 && !left.0.is_nan() {
        return Err(error("sql.22012", "Division by zero"));
    }
    let value = match op {
        "add" | "+" => left.0 + right.0,
        "subtract" | "-" => left.0 - right.0,
        "multiply" | "*" => left.0 * right.0,
        "divide" | "/" => left.0 / right.0,
        _ => return Err(error("unsupported.float_operator", op)),
    };
    if value.is_infinite() && left.0.is_finite() && right.0.is_finite() {
        return Err(error("sql.22003", "Float8 overflow"));
    }
    if matches!(op, "multiply" | "*" | "divide" | "/")
        && value == 0.0
        && left.0 != 0.0
        && right.0 != 0.0
        && right.0.is_finite()
    {
        return Err(error("sql.22003", "Float8 underflow"));
    }
    Ok(PgFloat(value))
}

/// Render PostgreSQL's scientific thresholds while preserving negative zero.
fn format_float(value: f64) -> String {
    if value.is_nan() {
        return "NaN".into();
    }
    if value == f64::INFINITY {
        return "Infinity".into();
    }
    if value == f64::NEG_INFINITY {
        return "-Infinity".into();
    }
    if value == 0.0 {
        return if value.is_sign_negative() { "-0" } else { "0" }.into();
    }
    if value.abs() >= 1e15 || value.abs() < 1e-4 {
        let scientific = format!("{:e}", value);
        let (mantissa, exponent) = scientific.split_once('e').unwrap_or((&scientific, "0"));
        let exponent: i32 = exponent.parse().unwrap_or(0);
        return format!("{mantissa}e{exponent:+03}");
    }
    value.to_string()
}
impl PartialEq for PgFloat {
    /// PostgreSQL considers all NaNs equal and both zeros equal.
    fn eq(&self, right: &Self) -> bool {
        self.cmp(right) == Ordering::Equal
    }
}
impl Eq for PgFloat {}
impl PartialOrd for PgFloat {
    /// Return PostgreSQL's total float ordering.
    fn partial_cmp(&self, right: &Self) -> Option<Ordering> {
        Some(self.cmp(right))
    }
}
impl Ord for PgFloat {
    /// NaN sorts after positive infinity rather than using its IEEE payload bits.
    fn cmp(&self, right: &Self) -> Ordering {
        match (self.0.is_nan(), right.0.is_nan()) {
            (true, true) => Ordering::Equal,
            (true, false) => Ordering::Greater,
            (false, true) => Ordering::Less,
            _ => self.0.partial_cmp(&right.0).unwrap_or(Ordering::Equal),
        }
    }
}

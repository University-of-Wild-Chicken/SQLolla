//! Exact base-ten numeric values with PostgreSQL display scale and rounding.
use crate::json::error;
use crate::Result;
use num_bigint::BigInt;
use num_traits::{Signed, Zero};
use std::cmp::Ordering;
use std::fmt;

/// Exact decimal; equality ignores display scale while formatting preserves it.
#[derive(Clone, Debug)]
pub struct Decimal {
    pub(crate) coefficient: BigInt,
    pub(crate) scale: i32,
    special: i8,
}

impl Decimal {
    /// Parse an exact decimal or PostgreSQL numeric special value.
    pub fn parse(value: &str) -> Result<Self> {
        let value = value.trim_matches(|character: char| character.is_ascii_whitespace());
        let special = match value.to_ascii_lowercase().as_str() {
            "nan" => 2,
            "infinity" | "+infinity" | "inf" | "+inf" => 1,
            "-infinity" | "-inf" => -1,
            _ => 0,
        };
        if special != 0 {
            return Ok(Self {
                coefficient: BigInt::zero(),
                scale: 0,
                special,
            });
        }
        Self::parse_finite(value)
    }

    /// Parse finite digits and exponent without using a floating-point intermediary.
    fn parse_finite(value: &str) -> Result<Self> {
        let (mantissa, exponent) = split_exponent(value)?;
        let negative = mantissa.starts_with('-');
        let unsigned = mantissa.strip_prefix(['+', '-']).unwrap_or(mantissa);
        let parts: Vec<_> = unsigned.split('.').collect();
        if parts.len() > 2 || unsigned.chars().all(|c| c == '.') {
            return invalid();
        }
        let digits = parts.concat();
        if digits.is_empty() || !digits.bytes().all(|c| c.is_ascii_digit()) {
            return invalid();
        }
        let coefficient = digits
            .parse::<BigInt>()
            .map_err(|_| error("sql.22P02", "Invalid numeric input"))?;
        let scale = parts.get(1).map_or(0, |part| part.len() as i32) - exponent;
        let result = Self {
            coefficient: if negative { -coefficient } else { coefficient },
            scale,
            special: 0,
        };
        result.checked()
    }

    /// Check PostgreSQL's finite storage limits without silently losing digits.
    pub(crate) fn checked(mut self) -> Result<Self> {
        if self.scale < 0 {
            if -self.scale > 131072 {
                return overflow();
            }
            self.coefficient *= power(-self.scale);
            self.scale = 0;
        }
        let digits = self.coefficient.abs().to_string().len() as i32;
        if self.scale > 16383 || digits - self.scale > 131072 {
            return overflow();
        }
        Ok(self)
    }

    /// Round to the declared decimal scale, with ties away from zero.
    pub fn rounded(&self, scale: i32) -> Result<Self> {
        if self.special != 0 {
            return Ok(self.clone());
        }
        if !(-1000..=16383).contains(&scale) {
            return overflow();
        }
        let mut result = self.clone();
        if scale >= self.scale {
            result.coefficient *= power(scale - self.scale);
        } else {
            let divisor = power(self.scale - scale);
            let remainder = &self.coefficient % &divisor;
            result.coefficient /= &divisor;
            if remainder.abs() * 2 >= divisor {
                result.coefficient += self.coefficient.signum();
            }
        }
        result.scale = scale;
        result.checked()
    }

    /// Enforce numeric precision after PostgreSQL's scale rounding.
    pub fn constrained(&self, precision: Option<u16>, scale: Option<i16>) -> Result<Self> {
        let Some(precision) = precision else {
            return Ok(self.clone());
        };
        let scale = i32::from(scale.unwrap_or(0));
        if !(1..=1000).contains(&precision) || !(-1000..=1000).contains(&scale) {
            return Err(error("sql.22023", "Invalid numeric typmod"));
        }
        if self.special == 2 {
            return Ok(self.clone());
        }
        if self.special != 0 {
            return overflow();
        }
        let result = self.rounded(scale)?;
        let maximum = power(i32::from(precision) - scale + result.scale);
        if result.coefficient.abs() >= maximum {
            return overflow();
        }
        Ok(result)
    }

    /// Apply exact finite arithmetic; special values have explicit semantic rules.
    pub fn arithmetic(&self, op: &str, right: &Self) -> Result<Self> {
        if self.special != 0 || right.special != 0 {
            return special_arithmetic(self, op, right);
        }
        let scale = self.scale.max(right.scale);
        let left_int = &self.coefficient * power(scale - self.scale);
        let right_int = &right.coefficient * power(scale - right.scale);
        let result = match op {
            "add" | "+" => Self::finite(left_int + right_int, scale),
            "subtract" | "-" => Self::finite(left_int - right_int, scale),
            "multiply" | "*" => Self::finite(
                &self.coefficient * &right.coefficient,
                self.scale + right.scale,
            ),
            "divide" | "/" => return self.divide(right),
            "modulo" | "%" if !right_int.is_zero() => Self::finite(left_int % right_int, scale),
            "modulo" | "%" => return Err(error("sql.22012", "Division by zero")),
            _ => return Err(error("unsupported.arithmetic", op)),
        };
        if result.scale > 16383 {
            result.rounded(16383)
        } else {
            result.checked()
        }
    }

    /// Use PostgreSQL16's base-10000 division display-scale selection.
    fn divide(&self, right: &Self) -> Result<Self> {
        if right.coefficient.is_zero() {
            return Err(error("sql.22012", "Division by zero"));
        }
        let (left_weight, left_digit) = self.leading_group();
        let (right_weight, right_digit) = right.leading_group();
        let quotient_weight = left_weight - right_weight - i32::from(left_digit <= right_digit);
        let scale = (16 - 4 * quotient_weight)
            .max(self.scale)
            .max(right.scale)
            .clamp(0, 1000);
        let shift = scale + right.scale - self.scale;
        let numerator = &self.coefficient * power(shift.max(0));
        let denominator = &right.coefficient * power((-shift).max(0));
        let mut coefficient = &numerator / &denominator;
        if (&numerator % &denominator).abs() * 2 >= denominator.abs() {
            coefficient += numerator.signum() * denominator.signum();
        }
        Self::finite(coefficient, scale).checked()
    }

    /// Construct a finite decimal before a caller applies range validation.
    fn finite(coefficient: BigInt, scale: i32) -> Self {
        Self {
            coefficient,
            scale,
            special: 0,
        }
    }

    /// Find the first normalized base-10000 digit without floating-point conversion.
    fn leading_group(&self) -> (i32, u32) {
        if self.coefficient.is_zero() {
            return (0, 0);
        }
        let digits = self.coefficient.abs().to_string();
        let exponent = digits.len() as i32 - self.scale - 1;
        let width = (exponent.rem_euclid(4) + 1) as usize;
        let first = format!("{digits:0<width$}")[..width].parse().unwrap_or(0);
        (exponent.div_euclid(4), first)
    }

    /// Determine whether the value is finite and mathematically zero.
    pub fn is_zero(&self) -> bool {
        self.special == 0 && self.coefficient.is_zero()
    }
}

/// Parse decimal exponents with a bounded exact integer representation.
fn split_exponent(value: &str) -> Result<(&str, i32)> {
    let parts: Vec<_> = value.split(['e', 'E']).collect();
    if parts.len() > 2 || value.len() > 200000 {
        return invalid();
    }
    let exponent = parts
        .get(1)
        .map_or(Ok(0), |part| part.parse::<i32>())
        .map_err(|_| error("sql.22003", "Numeric exponent out of range"))?;
    if !(-16383..=131072).contains(&exponent) {
        return overflow();
    }
    Ok((parts[0], exponent))
}

/// Return an exact nonnegative power of ten used by bounded scalar operations.
fn power(exponent: i32) -> BigInt {
    BigInt::from(10u8).pow(exponent.max(0) as u32)
}
/// Classify malformed decimal inputs as PostgreSQL invalid-text errors.
fn invalid<T>() -> Result<T> {
    Err(error("sql.22P02", "Invalid numeric input"))
}
/// Classify values beyond numeric storage/typmod bounds as numeric overflow.
fn overflow<T>() -> Result<T> {
    Err(error("sql.22003", "Numeric value out of range"))
}

/// Apply arithmetic to numeric infinities and NaN without host float approximations.
fn special_arithmetic(left: &Decimal, op: &str, right: &Decimal) -> Result<Decimal> {
    if left.special == 2 || right.special == 2 {
        return Decimal::parse("NaN");
    }
    let (l, r) = (numeric_sign(left), numeric_sign(right));
    let sign = match op {
        "add" | "+" => infinite_sum(left.special, right.special, l, r),
        "subtract" | "-" => infinite_sum(left.special, right.special, l, -r),
        "multiply" | "*" if left.is_zero() || right.is_zero() => 2,
        "multiply" | "*" => l * r,
        "divide" | "/" if right.is_zero() => return Err(error("sql.22012", "Division by zero")),
        "divide" | "/" if left.special != 0 && right.special != 0 => 2,
        "divide" | "/" if right.special != 0 => return Decimal::parse("0"),
        "divide" | "/" => l * r,
        _ => return Err(error("unsupported.numeric_special", op)),
    };
    Decimal::parse(match sign {
        -1 => "-Infinity",
        1 => "Infinity",
        _ => "NaN",
    })
}

/// Determine the sign of finite/infinite numeric operands without approximate conversion.
fn numeric_sign(value: &Decimal) -> i8 {
    if value.special != 0 {
        value.special
    } else if value.coefficient.is_negative() {
        -1
    } else {
        1
    }
}
/// Oppositely signed infinities sum to NaN; finite terms cannot change infinity.
fn infinite_sum(left_special: i8, right_special: i8, left: i8, right: i8) -> i8 {
    if left_special != 0 && right_special != 0 && left != right {
        2
    } else if left_special != 0 {
        left
    } else {
        right
    }
}

impl PartialEq for Decimal {
    /// Numeric equality ignores scale and follows PostgreSQL NaN equality.
    fn eq(&self, right: &Self) -> bool {
        self.cmp(right) == Ordering::Equal
    }
}
impl Eq for Decimal {}
impl PartialOrd for Decimal {
    /// Use the same total ordering as exact numeric comparison.
    fn partial_cmp(&self, right: &Self) -> Option<Ordering> {
        Some(self.cmp(right))
    }
}
impl Ord for Decimal {
    /// Sort negative infinity, finite values, positive infinity, then NaN.
    fn cmp(&self, right: &Self) -> Ordering {
        if self.special != right.special {
            return self.special.cmp(&right.special);
        }
        if self.special != 0 {
            return Ordering::Equal;
        }
        let scale = self.scale.max(right.scale);
        (&self.coefficient * power(scale - self.scale))
            .cmp(&(&right.coefficient * power(scale - right.scale)))
    }
}
impl fmt::Display for Decimal {
    /// Preserve decimal display scale without negative zero or exponent ambiguity.
    fn fmt(&self, output: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.special != 0 {
            return output.write_str(match self.special {
                -1 => "-Infinity",
                1 => "Infinity",
                _ => "NaN",
            });
        }
        let digits = self.coefficient.abs().to_string();
        let width = (self.scale + 1).max(digits.len() as i32) as usize;
        let digits = format!("{digits:0>width$}");
        let sign = if self.coefficient.is_negative() {
            "-"
        } else {
            ""
        };
        if self.scale == 0 {
            write!(output, "{sign}{digits}")
        } else {
            let position = digits.len() - self.scale as usize;
            write!(
                output,
                "{sign}{}.{}",
                &digits[..position],
                &digits[position..]
            )
        }
    }
}

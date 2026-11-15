//! PostgreSQL postgres-style interval output with independent signed fields.
use super::temporal::Interval;

/// Render separate months, days and unbounded hours without justify-style normalization.
pub(crate) fn format(value: &Interval) -> String {
    let mut output = Vec::new();
    let mut negative = false;
    calendar(
        &mut output,
        &mut negative,
        i64::from(value.months / 12),
        "year",
    );
    calendar(
        &mut output,
        &mut negative,
        i64::from(value.months % 12),
        "mon",
    );
    calendar(&mut output, &mut negative, i64::from(value.days), "day");
    if value.micros != 0 || output.is_empty() {
        let sign = if value.micros < 0 {
            "-"
        } else if negative {
            "+"
        } else {
            ""
        };
        output.push(format!("{sign}{}", clock(value.micros.unsigned_abs())));
    }
    output.join(" ")
}

/// Explicit plus signs disambiguate a positive field after a negative calendar field.
fn calendar(output: &mut Vec<String>, negative: &mut bool, value: i64, unit: &str) {
    if value == 0 {
        return;
    }
    let sign = if value > 0 && *negative { "+" } else { "" };
    let plural = if value == 1 { "" } else { "s" };
    output.push(format!("{sign}{value} {unit}{plural}"));
    *negative = value < 0;
}

/// Hour magnitudes may exceed 24 because stored time does not become calendar days.
fn clock(micros: u64) -> String {
    let seconds = micros / 1_000_000;
    let fraction = if micros.is_multiple_of(1_000_000) {
        String::new()
    } else {
        format!(".{:06}", micros % 1_000_000)
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

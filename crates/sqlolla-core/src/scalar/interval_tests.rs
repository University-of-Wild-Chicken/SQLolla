//! Contract-backed interval regression boundaries independently probed on PostgreSQL16.
use super::*;
use crate::{Cell, SqlType};

/// Keep literal unit inference distinct from casting an already typed text value.
#[test]
fn interval_literal_and_typed_text_differ() {
    let qualifier = IntervalQualifier {
        fields: IntervalFields::Month,
        precision: None,
    };
    assert_eq!(
        output(&interval_input("3", &qualifier).unwrap()).unwrap(),
        "3 mons"
    );
    assert_eq!(
        output(&interval_cast(&Cell::Text("3".into()), &qualifier).unwrap()).unwrap(),
        "00:00:00"
    );
    assert_eq!(
        output(&interval_cast(&input(&SqlType::Interval, "3 days").unwrap(), &qualifier).unwrap())
            .unwrap(),
        "00:00:00"
    );
}

/// Every range preserves fields to its left and removes only fields to its right.
#[test]
fn qualifier_preserves_larger_components() {
    let source = "1 year 2 mons 3 days 04:05:06.789";
    for (fields, expected) in [
        (IntervalFields::Year, "1 year"),
        (IntervalFields::Month, "1 year 2 mons"),
        (IntervalFields::Day, "1 year 2 mons 3 days"),
        (IntervalFields::DayToHour, "1 year 2 mons 3 days 04:00:00"),
        (
            IntervalFields::HourToMinute,
            "1 year 2 mons 3 days 04:05:00",
        ),
        (IntervalFields::Second, source),
    ] {
        let qualifier = IntervalQualifier {
            fields,
            precision: None,
        };
        assert_eq!(
            output(&interval_input(source, &qualifier).unwrap()).unwrap(),
            expected
        );
    }
}

/// Declared precision rounds microseconds half away from zero for each supported scale.
#[test]
fn interval_precision_rounding() {
    for (precision, text) in [
        (0, "00:00:01"),
        (1, "00:00:01.2"),
        (2, "00:00:01.23"),
        (3, "00:00:01.235"),
        (4, "00:00:01.2345"),
        (5, "00:00:01.2345"),
        (6, "00:00:01.2345"),
    ] {
        let qualifier = IntervalQualifier {
            fields: IntervalFields::Second,
            precision: Some(precision),
        };
        assert_eq!(
            output(&interval_input("1.2345", &qualifier).unwrap()).unwrap(),
            text
        );
        assert_eq!(
            output(&interval_input("-1.2345", &qualifier).unwrap()).unwrap(),
            "-".to_owned() + text
        );
    }
}

/// Constructor seconds use ties-even before being combined with exact calendar components.
#[test]
fn make_interval_defaults_and_float_ties() {
    assert_eq!(
        output(&function("make_interval", &[]).unwrap()).unwrap(),
        "00:00:00"
    );
    for (seconds, expected) in [
        ("0.0000005", "00:00:00"),
        ("0.0000015", "00:00:00.000002"),
        ("0.0000025", "00:00:00.000002"),
    ] {
        let mut args = vec![Cell::Int32(0); 6];
        args.push(input(&SqlType::Float64, seconds).unwrap());
        assert_eq!(
            output(&function("make_interval", &args).unwrap()).unwrap(),
            expected
        );
    }
    assert_eq!(
        function("make_interval", &[Cell::Null(SqlType::Int32)]).unwrap(),
        Cell::Null(SqlType::Interval)
    );
}

/// Months, weeks/days and elapsed clock fields remain separate in constructor output.
#[test]
fn make_interval_component_storage() {
    let mut args = (1..=6).map(Cell::Int32).collect::<Vec<_>>();
    args.push(input(&SqlType::Float64, "7.89").unwrap());
    assert_eq!(
        output(&function("make_interval", &args).unwrap()).unwrap(),
        "1 year 2 mons 25 days 05:06:07.89"
    );
    assert!(function("make_interval", &[Cell::Int32(i32::MAX)])
        .unwrap_err()
        .code
        .starts_with("unsupported."));
    args[6] = input(&SqlType::Float64, "NaN").unwrap();
    assert_eq!(
        function("make_interval", &args).unwrap_err().code,
        "unsupported.interval_nonfinite"
    );
}

/// SQL two-field clocks depend on the exact range, while PostgreSQL signs stay independent.
#[test]
fn interval_clock_and_signs() {
    let qualifier = IntervalQualifier {
        fields: IntervalFields::MinuteToSecond,
        precision: None,
    };
    assert_eq!(
        output(&interval_input("1:2", &qualifier).unwrap()).unwrap(),
        "00:01:02"
    );
    for (source, expected) in [
        ("1:2", "01:02:00"),
        ("-1 02:03:04", "-1 days +02:03:04"),
        ("-1 mon 1 day 1 hour", "-1 mons +1 day 01:00:00"),
        ("1-2", "1 year 2 mons"),
    ] {
        assert_eq!(
            output(&input(&SqlType::Interval, source).unwrap()).unwrap(),
            expected
        );
    }
}

/// Fractional calendar units cascade into smaller fields before a qualifier truncates them.
#[test]
fn interval_fraction_and_refusals() {
    assert_eq!(
        output(&input(&SqlType::Interval, "1.75 months").unwrap()).unwrap(),
        "1 mon 22 days 12:00:00"
    );
    assert_eq!(
        input(&SqlType::Interval, "1 hour 2 hour").unwrap_err().code,
        "sql.22007"
    );
    let qualifier = IntervalQualifier {
        fields: IntervalFields::Full,
        precision: Some(7),
    };
    assert_eq!(
        interval_input("1", &qualifier).unwrap_err().code,
        "unsupported.interval_precision"
    );
    let value = input(&SqlType::Interval, "1 day").unwrap();
    assert_eq!(
        arithmetic("add", &value, &value).unwrap_err().code,
        "unsupported.interval_arithmetic"
    );
}

/// Distinct date units accumulate; leap-second clock input is normalized before output.
#[test]
fn interval_distinct_units_and_field_ranges() {
    assert_eq!(
        output(&input(&SqlType::Interval, "1 week 2 days").unwrap()).unwrap(),
        "9 days"
    );
    assert_eq!(
        output(&input(&SqlType::Interval, "1:00:60").unwrap()).unwrap(),
        "01:01:00"
    );
    assert_eq!(
        input(&SqlType::Interval, "25:60:00").unwrap_err().code,
        "sql.22015"
    );
    assert_eq!(
        input(&SqlType::Interval, "1-13").unwrap_err().code,
        "sql.22015"
    );
}

/// Literal fractional units truncate exact ties; clock fractions choose the even microsecond.
#[test]
fn interval_fraction_rounding_paths() {
    for (source, expected) in [
        ("0.0000015 seconds", "00:00:00.000001"),
        ("-0.0000015 seconds", "-00:00:00.000001"),
        ("1.5 microseconds", "00:00:00.000001"),
        ("0:00:00.0000015", "00:00:00.000002"),
        ("0:00.0000015", "00:00:00.000002"),
        ("1:02.5", "00:01:02.5"),
    ] {
        assert_eq!(
            output(&input(&SqlType::Interval, source).unwrap()).unwrap(),
            expected
        );
    }
    assert_eq!(
        input(&SqlType::Interval, "1.5 seconds 2 milliseconds")
            .unwrap_err()
            .code,
        "sql.22007"
    );
}

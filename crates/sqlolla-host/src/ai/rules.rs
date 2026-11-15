//! Redaction-safe rule outcomes from the exact replay-confirmed counterexample.
use super::context::Evidence;
use serde_json::{Value, json};
use sqlolla::alloy::{Property, violated};

/// Report only a supported, witnessed violation without transmitting the rule literal or row values.
pub(super) fn observe(evidence: &Evidence) -> Option<Value> {
    let instance = &evidence.instance;
    if instance["kind"] != "counterexample" || instance["validation"] != "RUST_REPLAY_CONFIRMED" {
        return None;
    }
    let property = serde_json::from_value(evidence.bundle["request"]["property"].clone()).ok()?;
    let semantic = &instance["semantic"];
    match &property {
        Property::NoSqlError => (semantic["outcome"]["status"] == "sql_error").then(|| {
            json!({"kind":"execution_error","expectation":"The statement must complete without an SQL error.",
                "observed":"The replayed execution raised an SQL error."})
        }),
        _ => read_observation(&property, semantic),
    }
}

/// Use the bag checked by the corresponding native or outcome-backed solver profile.
fn output_rows(semantic: &Value) -> Option<(&'static str, &[Value])> {
    if let Some(output) = semantic["output"].as_array() {
        return Some(("/semantic/output", output));
    }
    let outcome = &semantic["outcome"];
    let (path, field) = match outcome["status"].as_str()? {
        "succeeded" => ("/semantic/outcome/outcome/rows", "rows"),
        "sql_error" => ("/semantic/outcome/outcome/exposed_rows", "exposed_rows"),
        _ => return None,
    };
    Some((path, outcome["outcome"][field].as_array()?))
}

/// Disclose the checked output position or cardinality and one canonical witness pointer.
fn read_observation(property: &Property, semantic: &Value) -> Option<Value> {
    let (path, rows) = output_rows(semantic)?;
    let (column, kind, expectation, observed) = match property {
        Property::OnlyValue { column, .. } => (
            *column,
            "required_value_mismatch",
            "Every returned row must match the required value at the selected output position.",
            "The cited row differs from the required value.",
        ),
        Property::NoValue { column, .. } => (
            *column,
            "forbidden_value_returned",
            "The forbidden value must never appear at the selected output position.",
            "The cited row contains the forbidden value.",
        ),
        Property::MaxRows { max } => return row_limit(path, rows.len(), *max),
        _ => return None,
    };
    let index = rows
        .iter()
        .position(|row| violates_at(property, row, column))?;
    Some(
        json!({"kind":kind,"output_position":column.checked_add(1)?,"witness_reference":format!("{path}/{index}"),
        "expectation":expectation,"observed":observed}),
    )
}

/// Disclose a witnessed cardinality violation without including the rule's limit.
fn row_limit(path: &str, occurrences: usize, max: usize) -> Option<Value> {
    (occurrences > max).then(|| {
        json!({"kind":"row_limit_exceeded","output_occurrences":occurrences,"witness_reference":format!("{path}/0"),
            "expectation":"The result must stay within the row limit.","observed":"The replayed result exceeded the row limit."})
    })
}

/// Reuse the canonical typed equality check, including typed NULL and exact numeric representations.
fn violates_at(property: &Property, row: &Value, column: usize) -> bool {
    row.get(column).is_some()
        && serde_json::from_value(Value::Array(vec![row.clone()]))
            .is_ok_and(|bag| violated(property, &bag))
}

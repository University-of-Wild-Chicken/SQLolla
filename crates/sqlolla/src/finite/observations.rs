//! Exact observation payload atoms bind table state, retained effects and SQL diagnostics.
use super::{FiniteModel, Transition};
use serde_json::{Value, json};
use sqlolla_core::{Cell, StatementOutcome};
use std::collections::BTreeSet;

/// Preserve complete deterministic transport payloads independently of result-row membership.
pub(super) fn payloads(transition: &Transition) -> Vec<(&'static str, Cell)> {
    let (after, catalog, effects) = match &transition.outcome {
        StatementOutcome::Succeeded(value) => (
            &value.database,
            value
                .catalog
                .as_deref()
                .or(transition.input_catalog.as_ref()),
            &value.effects,
        ),
        StatementOutcome::SqlError(value) => (
            &transition.input,
            transition.input_catalog.as_ref(),
            &value.effects,
        ),
    };
    vec![
        (
            "prePayload",
            serialized(&json!({"database":transition.input,"catalog":transition.input_catalog})),
        ),
        (
            "postPayload",
            serialized(&json!({"database":after,"catalog":catalog})),
        ),
        ("eventPayload", serialized(effects)),
        ("detailPayload", serialized(&transition.outcome)),
    ]
}

/// Fixed payload identities are evaluator observations, not independently derived SQL equations.
pub(super) fn facts(data: &FiniteModel) -> String {
    let mut output = String::new();
    for (index, transition) in data.transitions.iter().enumerate() {
        for (field, value) in payloads(transition) {
            output.push_str(&format!("T{index}.{field} = {}\n", data.values[&value]));
        }
    }
    output
}

/// Reject a solver/API instance whose state, error details or retained trace has been altered.
pub(super) fn verify(data: &FiniteModel, response: &Value) -> Result<(), String> {
    for field in ["prePayload", "postPayload", "eventPayload", "detailPayload"] {
        let actual: BTreeSet<_> =
            super::verify::relation(response, &format!("Transition.{field}"), 2)?
                .into_iter()
                .collect();
        let mut expected = BTreeSet::new();
        for (index, transition) in data.transitions.iter().enumerate() {
            let payload = payloads(transition)
                .into_iter()
                .find(|(name, _)| *name == field)
                .ok_or("missing observation field")?
                .1;
            expected.insert(vec![
                super::verify::singleton(response, &format!("T{index}"))?,
                super::verify::singleton(response, &data.values[&payload])?,
            ]);
        }
        if actual != expected {
            return Err(format!(
                "finite observation {field} differs from complete payload"
            ));
        }
    }
    Ok(())
}

/// Canonical struct fields and ordered maps make observation atoms reproducible.
fn serialized<T: serde::Serialize>(value: &T) -> Cell {
    Cell::Text(serde_json::to_string(value).expect("typed finite observation serialization"))
}

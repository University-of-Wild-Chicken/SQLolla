//! Check the finite Alloy table inventory before resolving a solver-selected transition.
use super::FiniteModel;
use serde_json::Value;
use sqlolla_core::StatementOutcome;
use std::collections::{BTreeMap, BTreeSet};

/// Validate every fixed field and then decode exactly one legal selected transition.
pub(super) fn decode(data: &FiniteModel, response: &Value) -> Result<usize, String> {
    check_inventory(data, response)?;
    let selected = singleton(response, "Selected")?;
    let choices = relation(response, "Selected.choice", 2)?;
    let choices: Vec<_> = choices
        .iter()
        .filter(|tuple| tuple[0] == selected)
        .collect();
    if choices.len() != 1 {
        return Err("finite model must select exactly one transition".into());
    }
    check_rows(data, response)?;
    check_transitions(data, response)?;
    super::observations::verify(data, response)?;
    for index in 0..data.transitions.len() {
        if singleton(response, &format!("T{index}"))? == choices[0][1] {
            return Ok(index);
        }
    }
    Err("solver selected an unknown transition".into())
}

/// Distinct declared singletons cannot alias even when corrupt tuples reuse the same atom.
fn check_inventory(data: &FiniteModel, response: &Value) -> Result<(), String> {
    let mut atoms = BTreeSet::from([singleton(response, "Selected")?]);
    for (parent, children) in inventory(data) {
        let mut expected = BTreeSet::new();
        for child in children {
            let atom = singleton(response, &child)?;
            if !atoms.insert(atom.clone()) {
                return Err("distinct finite singletons alias one API atom".into());
            }
            expected.insert(vec![atom]);
        }
        if relation(response, parent, 1)?
            .into_iter()
            .collect::<BTreeSet<_>>()
            != expected
        {
            return Err(format!(
                "finite {parent} inventory differs from declared singletons"
            ));
        }
    }
    Ok(())
}

/// Enumerate exact declared child signatures without trusting API parent membership.
fn inventory(data: &FiniteModel) -> Vec<(&'static str, Vec<String>)> {
    vec![
        ("Value", data.values.values().cloned().collect::<Vec<_>>()),
        (
            "Col",
            (0..data.schema.len()).map(|i| format!("K{i}")).collect(),
        ),
        (
            "Row",
            (0..data.rows.len()).map(|i| format!("R{i}")).collect(),
        ),
        (
            "Transition",
            (0..data.transitions.len())
                .map(|i| format!("T{i}"))
                .collect(),
        ),
        ("Status", vec!["Success".into(), "Failure".into()]),
    ]
}

/// Require exact positional cell tuples for all occurrences, including unselected alternatives.
fn check_rows(data: &FiniteModel, response: &Value) -> Result<(), String> {
    let actual: BTreeSet<_> = relation(response, "Row.cells", 3)?.into_iter().collect();
    let values: BTreeMap<_, _> = data
        .values
        .iter()
        .map(|(value, name)| Ok((value, singleton(response, name)?)))
        .collect::<Result<_, String>>()?;
    let mut expected = BTreeSet::new();
    for (i, row) in data.rows.iter().enumerate() {
        let atom = singleton(response, &format!("R{i}"))?;
        for (column, value) in row.iter().enumerate() {
            expected.insert(vec![
                atom.clone(),
                singleton(response, &format!("K{column}"))?,
                values[value].clone(),
            ]);
        }
    }
    if actual != expected {
        return Err("finite model output cells differ from the complete table".into());
    }
    Ok(())
}

/// Verify status and row membership for every transition; no error outcome may disappear.
fn check_transitions(data: &FiniteModel, response: &Value) -> Result<(), String> {
    let actual_rows: BTreeSet<_> = relation(response, "Transition.output", 2)?
        .into_iter()
        .collect();
    let actual_status: BTreeSet<_> = relation(response, "Transition.status", 2)?
        .into_iter()
        .collect();
    let mut rows = BTreeSet::new();
    let mut statuses = BTreeSet::new();
    for (i, transition) in data.transitions.iter().enumerate() {
        let atom = singleton(response, &format!("T{i}"))?;
        for row in &data.output_rows[i] {
            rows.insert(vec![atom.clone(), singleton(response, &format!("R{row}"))?]);
        }
        let name = if matches!(transition.outcome, StatementOutcome::Succeeded(_)) {
            "Success"
        } else {
            "Failure"
        };
        statuses.insert(vec![atom, singleton(response, name)?]);
    }
    if rows != actual_rows || statuses != actual_status {
        return Err("finite model transition table differs from its complete inventory".into());
    }
    Ok(())
}

/// Require typed tuple arity before inspecting any API values.
pub(super) fn relation(
    response: &Value,
    name: &str,
    arity: usize,
) -> Result<Vec<Vec<String>>, String> {
    let value = response["relations"]
        .get(format!("this/{name}"))
        .ok_or("missing finite relation")?;
    let tuples: Vec<Vec<String>> =
        serde_json::from_value(value.clone()).map_err(|e| e.to_string())?;
    if tuples.iter().any(|tuple| tuple.len() != arity) {
        return Err("finite relation has unexpected arity".into());
    }
    Ok(tuples)
}

/// Resolve actual API atoms rather than guessing solver-generated suffixes.
pub(super) fn singleton(response: &Value, name: &str) -> Result<String, String> {
    let rows = relation(response, name, 1)?;
    match rows.as_slice() {
        [row] => Ok(row[0].clone()),
        _ => Err("finite atom was not a singleton".into()),
    }
}

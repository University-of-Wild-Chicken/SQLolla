//! Deterministic Alloy facts for an explicitly complete table of Rust outcomes.
use super::FiniteModel;
use crate::alloy::Property;
use sha2::{Digest, Sha256};
use sqlolla_core::StatementOutcome;

/// Emit data, reachability and a separate violation command for the finite domain.
pub(super) fn model(data: &FiniteModel, property: Option<&Property>) -> Result<String, String> {
    let payload = serde_json::to_vec(&data.transitions).map_err(|e| e.to_string())?;
    let mut result = format!(
        "// Explicit evaluator-backed transitions; payload SHA256 {:x}\n",
        Sha256::digest(payload)
    );
    result.push_str(&declarations(data));
    result.push_str("pred ExecutionTable {\n");
    result.push_str(&transition_facts(data));
    result.push_str(&row_facts(data));
    result.push_str(&super::observations::facts(data));
    result.push_str("}\n");
    let scope = scope(data);
    result.push_str(&format!("run ExecutionTable {scope}\n"));
    if let Some(property) = property {
        result.push_str(&crate::alloy::property_check(
            "ExecutionTable",
            &violation(data, property)?,
            &scope,
        ));
    }
    for i in 0..data.transitions.len() {
        result.push_str(&format!(
            "run {{ ExecutionTable and Selected.choice = T{i} }} {scope}\n"
        ));
    }
    Ok(result)
}

/// Every transition, occurrence, column and scalar identity has an exact singleton.
fn declarations(data: &FiniteModel) -> String {
    let mut result = "abstract sig Value {}\nabstract sig Col {}\nabstract sig Row { cells: Col -> lone Value }\n".to_owned();
    result.push_str("abstract sig Status {}\none sig Success, Failure extends Status {}\n");
    result.push_str("abstract sig Transition { output: set Row, status: one Status, prePayload: one Value, postPayload: one Value, eventPayload: one Value, detailPayload: one Value }\none sig Selected { choice: one Transition }\n");
    for name in data.values.values() {
        result.push_str(&format!("one sig {name} extends Value {{}}\n"));
    }
    for (label, count, parent) in [
        ("K", width(data), "Col"),
        ("R", data.rows.len(), "Row"),
        ("T", data.transitions.len(), "Transition"),
    ] {
        for i in 0..count {
            result.push_str(&format!("one sig {label}{i} extends {parent} {{}}\n"));
        }
    }
    result
}

/// Fix transition membership without filtering errors or undesirable outcomes.
fn transition_facts(data: &FiniteModel) -> String {
    let mut result = String::new();
    for (i, transition) in data.transitions.iter().enumerate() {
        let rows: Vec<_> = data.output_rows[i]
            .iter()
            .map(|r| format!("R{r}"))
            .collect();
        let status = match transition.outcome {
            StatementOutcome::Succeeded(_) => "Success",
            StatementOutcome::SqlError(_) => "Failure",
        };
        result.push_str(&format!(
            "T{i}.output = {}\nT{i}.status = {status}\n",
            union(&rows)
        ));
    }
    result
}

/// Preserve every output position and duplicate occurrence using compiler-owned atoms.
fn row_facts(data: &FiniteModel) -> String {
    let mut result = String::new();
    for (i, row) in data.rows.iter().enumerate() {
        let cells: Vec<_> = row
            .iter()
            .enumerate()
            .map(|(column, value)| format!("K{column}->{}", data.values[value]))
            .collect();
        let relation = if cells.is_empty() {
            "none->none".into()
        } else {
            cells.join(" + ")
        };
        result.push_str(&format!("R{i}.cells = {relation}\n"));
    }
    result
}

/// The property constrains selected output data, never which transitions exist.
fn violation(data: &FiniteModel, property: &Property) -> Result<String, String> {
    match property {
        Property::NoSqlError => Ok("Selected.choice.status = Failure".into()),
        Property::MaxRows { max } if *max >= data.rows.len() => Ok("some none".into()),
        Property::MaxRows { max } => Ok(format!("#Selected.choice.output > {max}")),
        Property::NoValue { column, value } | Property::OnlyValue { column, value } => {
            if *column >= width(data) {
                return Err("property column absent from finite output schema".into());
            }
            if crate::alloy::type_of(value) != data.schema[*column].ty {
                return Err("finite property value type differs from the output column".into());
            }
            let comparison = if matches!(property, Property::OnlyValue { .. }) {
                "!="
            } else {
                "="
            };
            Ok(format!(
                "some r:Selected.choice.output | r.cells[K{column}] {comparison} {}",
                data.values[value]
            ))
        }
    }
}

/// Provide exact auxiliary scopes and a signed range that contains every row count.
fn scope(data: &FiniteModel) -> String {
    let bits = (usize::BITS - data.rows.len().max(1).leading_zeros() + 1).max(4);
    format!(
        "for 0 but {bits} Int, exactly {} Transition, exactly {} Row, exactly {} Col, exactly {} Value, exactly 2 Status",
        data.transitions.len(),
        data.rows.len(),
        width(data),
        data.values.len()
    )
}

/// Empty row collections remain exactly empty, not an unconstrained relation.
fn union(names: &[String]) -> String {
    if names.is_empty() {
        "none".into()
    } else {
        names.join(" + ")
    }
}

/// Determine structural column capacity across all returned and exposed rows.
fn width(data: &FiniteModel) -> usize {
    data.schema.len()
}

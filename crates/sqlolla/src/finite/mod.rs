//! Explicit finite execution tables; PostgreSQL remains the independent semantic oracle.
mod emit;
mod observations;
mod verify;
use crate::alloy::Property;
use serde::{Deserialize, Serialize};
use sqlolla_core::{Catalog, Cell, Column, Database, Row, StatementOutcome};
use std::collections::{BTreeMap, BTreeSet};

/// A complete evaluator transition under one explicitly supplied input database.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Transition {
    /// Explicit input catalog for catalog-aware execution and complete witness replay.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub input_catalog: Option<Catalog>,
    /// Exact initial transactional state.
    pub input: Database,
    /// One possible success or SQL-error behavior.
    pub outcome: StatementOutcome,
}

/// Finite model and the deterministic decoding inventory for its transitions.
pub struct FiniteModel {
    /// Generated Alloy source, including payload fingerprints.
    pub model: String,
    /// Complete transition list; a resource failure never returns a prefix.
    pub transitions: Vec<Transition>,
    pub(crate) schema: Vec<Column>,
    pub(crate) values: BTreeMap<Cell, String>,
    pub(crate) rows: Vec<Row>,
    pub(crate) output_rows: Vec<Vec<usize>>,
}

impl FiniteModel {
    /// Check every encoded output field before resolving the chosen transition.
    pub fn decode(&self, response: &serde_json::Value) -> Result<usize, String> {
        verify::decode(self, response)
    }
}

/// Enumerate concrete transitions into a finite SAT selection model without executing SQL.
pub fn encode(
    transitions: Vec<Transition>,
    schema: &[Column],
    property: Option<&Property>,
) -> Result<FiniteModel, String> {
    if transitions.is_empty() || transitions.len() > 256 {
        return Err("finite transition count must be between one and 256".into());
    }
    let (rows, output_rows) = flatten_rows(&transitions)?;
    let values = collect_values(&rows, &transitions, property);
    if rows.iter().any(|row| row.len() != schema.len()) {
        return Err("finite outcome row width differs from its bound schema".into());
    }
    let mut result = FiniteModel {
        model: String::new(),
        transitions,
        schema: schema.to_vec(),
        values,
        rows,
        output_rows,
    };
    result.model = emit::model(&result, property)?;
    if result.model.len() > 2_097_152 {
        return Err("finite transition model exceeds two MiB".into());
    }
    Ok(result)
}

/// Retain exposed failure rows and duplicate output occurrences separately.
fn flatten_rows(transitions: &[Transition]) -> Result<(Vec<Row>, Vec<Vec<usize>>), String> {
    let mut rows = Vec::new();
    let mut indices = Vec::new();
    for transition in transitions {
        let output = match &transition.outcome {
            StatementOutcome::Succeeded(value) => &value.rows,
            StatementOutcome::SqlError(value) => &value.exposed_rows,
        };
        if rows.len().saturating_add(output.len()) > 4096 || output.iter().any(|r| r.len() > 64) {
            return Err("finite output occurrence or column capacity exceeded".into());
        }
        indices.push((rows.len()..rows.len() + output.len()).collect());
        rows.extend(output.iter().cloned());
    }
    Ok((rows, indices))
}

/// Canonical typed values include the policy target even when absent from every output.
fn collect_values(
    rows: &[Row],
    transitions: &[Transition],
    property: Option<&Property>,
) -> BTreeMap<Cell, String> {
    let mut cells: BTreeSet<_> = rows.iter().flatten().cloned().collect();
    for transition in transitions {
        cells.extend(
            observations::payloads(transition)
                .into_iter()
                .map(|(_, value)| value),
        );
    }
    if let Some(Property::NoValue { value, .. } | Property::OnlyValue { value, .. }) = property {
        cells.insert(value.clone());
    }
    cells
        .into_iter()
        .enumerate()
        .map(|(i, value)| (value, format!("V{i}")))
        .collect()
}

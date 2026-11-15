//! Typed comparison keeps relational bags, row positions and all effect/error observations.
use super::*;
use sqlolla_core::{Bag, Catalog, Database, ResultOrdering, StatementOutcome, StatementSuccess};

/// Complete input and behavior in common form, independent of solver atoms and transition slots.
#[derive(PartialEq, Eq, Serialize)]
pub(super) struct Observation {
    pub database: Database,
    pub catalog: Catalog,
    pub outcome: StatementOutcome,
}

impl Observation {
    /// Read typed replay evidence and normalize the native deterministic SELECT envelope.
    pub(super) fn decode(bundle: &Bundle, semantic: &Value) -> Outcome<Self> {
        let database = serde_json::from_value(semantic["database"].clone()).map_err(json_error)?;
        let catalog = serde_json::from_value(semantic["catalog"].clone()).map_err(json_error)?;
        if bundle.request.catalog != catalog {
            return Err(failure(
                "refinement.input_catalog",
                "Replay input catalog differs from its archived request",
            ));
        }
        let outcome = match bundle.assurance_kind.as_str() {
            "native_ra_alloy_with_rust_replay" => native(semantic, &database, &catalog)?,
            "evaluator_backed_table_with_rust_replay" => {
                serde_json::from_value(semantic["outcome"].clone()).map_err(json_error)?
            }
            _ => {
                return Err(failure(
                    "refinement.profile",
                    "Unsupported archived replay profile",
                ));
            }
        };
        Ok(Self {
            database,
            catalog,
            outcome: canonical(outcome),
        })
    }
}

/// Native replay guarantees pure unordered SELECT with unchanged declared state.
fn native(semantic: &Value, database: &Database, catalog: &Catalog) -> Outcome<StatementOutcome> {
    let rows: Bag = serde_json::from_value(semantic["output"].clone()).map_err(json_error)?;
    Ok(StatementOutcome::Succeeded(StatementSuccess {
        catalog: Some(Box::new(catalog.clone())),
        database: database.clone(),
        affected: rows.len() as u64,
        command_tag: format!("SELECT {}", rows.len()),
        rows,
        ordering: ResultOrdering::Bag,
        effects: Vec::new(),
    }))
}

/// Sort only bags, keeping ordered and exposed rows, effect order and typed identity exact.
pub(super) fn canonical(mut outcome: StatementOutcome) -> StatementOutcome {
    if let StatementOutcome::Succeeded(value) = &mut outcome {
        if value.ordering == ResultOrdering::Bag {
            value.rows.sort();
        }
        value.database.values_mut().for_each(|rows| rows.sort());
    }
    outcome
}

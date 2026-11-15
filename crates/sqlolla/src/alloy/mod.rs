//! Occurrence-preserving relational encoding with explicit finite capacities.
mod builder;
mod expressions;
mod native;
mod operators;
mod set;
pub use native::supports;

use serde::{Deserialize, Serialize};
use sqlolla_core::{Bag, Cell, Column, Database, Plan};
use std::collections::BTreeMap;

/// Hard bounds prevent silent truncation and uncontrolled model expansion.
pub const MAX_SLOTS: usize = 256;

/// A supported read-result property; all indices refer to bound output positions.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum Property {
    /// Every admitted execution must complete without an SQL error.
    NoSqlError,
    /// No output row may contain this value at the selected position.
    NoValue {
        /// Zero-based output column position.
        column: usize,
        /// Forbidden exact typed value, including a typed NULL.
        value: Cell,
    },
    /// Every output row must contain this value at the selected position.
    OnlyValue {
        /// Zero-based output column position.
        column: usize,
        /// Required exact typed value, including a typed NULL.
        value: Cell,
    },
    /// Output cardinality may not exceed the specified count.
    MaxRows {
        /// Largest permitted output bag cardinality.
        max: usize,
    },
}

/// One relation's complete, ordered occurrence-slot inventory.
#[derive(Clone, Debug)]
pub struct Layout {
    /// Stable compiler-owned Alloy signature identifier.
    pub name: String,
    /// Complete finite slot names, including potentially inactive slots.
    pub rows: Vec<String>,
    /// Output cells' ordered types and source names.
    pub schema: Vec<Column>,
}

/// Concrete checking or finite symbolic input generation.
pub enum Inputs<'a> {
    /// Every supplied occurrence is active and its cells are fixed.
    Concrete(&'a Database),
    /// Inputs range over declared values and up to the listed slot counts.
    Symbolic {
        /// Maximum occurrences per stable catalog table identifier.
        bounds: &'a BTreeMap<String, usize>,
        /// Explicit finite typed input values, filtered by each column type.
        domain: &'a [Cell],
    },
}

/// Canonical model plus positional metadata required to decode its instances.
pub struct Encoded {
    /// Complete deterministic Alloy source with explicit scopes.
    pub model: String,
    /// Root relation layout.
    pub output: Layout,
    /// Shared base relations keyed by stable catalog IDs.
    pub inputs: BTreeMap<String, Layout>,
    /// Compiler-owned cell atoms and their original typed values.
    pub values: BTreeMap<String, Cell>,
}

/// Encode all admitted operators or return a structured coverage/resource failure.
pub fn encode(
    plan: &Plan,
    inputs: Inputs<'_>,
    property: Option<&Property>,
) -> Result<Encoded, String> {
    if !supports(plan) {
        return Err("plan requires the explicit outcome encoding profile".into());
    }
    builder::Builder::new(plan, inputs, property)?.compile(plan, property)
}

/// Recover a cell's declared scalar family for boundary validation.
pub fn type_of(cell: &Cell) -> sqlolla_core::SqlType {
    builder::cell_type(cell)
}

/// Independently check a decoded result bag against a supported property.
pub fn violated(property: &Property, bag: &Bag) -> bool {
    match property {
        Property::NoSqlError => false,
        Property::MaxRows { max } => bag.len() > *max,
        Property::NoValue { column, value } => bag.iter().any(|r| r.get(*column) == Some(value)),
        Property::OnlyValue { column, value } => bag.iter().any(|r| r.get(*column) != Some(value)),
    }
}

/// Keep query reachability independent of the property whose negation Alloy checks.
pub(crate) fn property_check(semantics: &str, violation: &str, scope: &str) -> String {
    format!(
        "pred Violates {{ {violation} }}\npred ResultProperty {{ not Violates }}\nassert QueryProperty {{ {semantics} implies ResultProperty }}\ncheck QueryProperty {scope}\n"
    )
}

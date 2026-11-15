//! Explicit table semantics needed to admit mutation and constrained initial states.
use crate::*;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// Complete declared table behavior; omission never asserts that a real table is unconstrained.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TableSemantics {
    /// Caller asserts all relevant table behavior is represented in this metadata.
    pub complete: bool,
    /// Defaults keyed by physical column position.
    #[serde(default)]
    pub defaults: BTreeMap<usize, DefaultSpec>,
    /// Primary key, separate from nullable unique constraints.
    #[serde(default)]
    pub primary_key: Option<KeyConstraint>,
    /// All unique constraints and their NULL/timing policies.
    #[serde(default)]
    pub unique: Vec<KeyConstraint>,
    /// Standalone unique indexes, including explicit partial predicates; not named constraints.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub unique_indexes: Vec<UniqueIndex>,
    /// All uniqueness diagnostic names in ascending PostgreSQL index OID order.
    /// Empty permits only observations independent of competing rejection order.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub unique_enforcement_order: Vec<String>,
    /// Row CHECK expressions retain parser provenance.
    #[serde(default)]
    pub checks: Vec<CheckConstraint>,
    /// Foreign keys and referential actions.
    #[serde(default)]
    pub foreign_keys: Vec<ForeignKey>,
    /// Trigger effects require their own execution model before writes are admitted.
    #[serde(default)]
    pub has_triggers: bool,
    /// Row security cannot disappear from a mutation's admission context.
    #[serde(default)]
    pub row_security: bool,
    /// Inherited or partition-routed writes need a multi-relation target model.
    #[serde(default)]
    pub has_inheritance_or_partitioning: bool,
}

/// An ordinary unique index enforces keys only for rows satisfying its optional predicate.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct UniqueIndex {
    /// Index name, physical key positions and NULL policy; indexes are immediate.
    pub key: KeyConstraint,
    /// Pinned-parser expression evidence; absent denotes a total index.
    pub predicate: Option<CheckConstraint>,
}

/// Defaults distinguish constants, parsed expressions and nontransactional allocation.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum DefaultSpec {
    /// Explicit typed constant default.
    Literal {
        /// Exact default value.
        value: Cell,
    },
    /// Default expression parsed by the pinned adapter before binding.
    Expression {
        /// Original expression for deterministic pipeline parsing and fingerprints.
        sql: String,
        /// Pipeline-generated parser envelope; absent means not yet admitted.
        parser: Option<serde_json::Value>,
    },
    /// Allocation requires explicit sequence state and retained nontransactional effects.
    Sequence {
        /// Resolved sequence identifier.
        sequence: String,
    },
}

/// Ordered columns and enforcement timing of a named unique/primary key.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct KeyConstraint {
    /// PostgreSQL constraint name.
    pub name: String,
    /// Physical column positions forming the key.
    pub columns: Vec<usize>,
    /// Whether NULL components participate in duplicate conflicts.
    #[serde(default)]
    pub nulls_not_distinct: bool,
    /// Deferrable uniqueness differs from immediate row-by-row enforcement.
    #[serde(default)]
    pub deferrable: bool,
    /// Initial deferred mode; commit checks remain distinct from statement checks.
    #[serde(default)]
    pub initially_deferred: bool,
}

/// CHECK SQL and its full pinned syntax envelope; arbitrary bound IR is not accepted here.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CheckConstraint {
    /// PostgreSQL named constraint.
    pub name: String,
    /// Original CHECK expression.
    pub sql: String,
    /// Parser-generated one-expression SELECT envelope.
    pub parser: Option<serde_json::Value>,
}

/// Referential action is explicit rather than assumed to frame all other tables.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ReferentialAction {
    /// Check at the applicable constraint boundary.
    NoAction,
    /// Reject a referenced-row change immediately.
    Restrict,
    /// Propagate a referenced-key update or deletion.
    Cascade,
    /// Set referencing columns to NULL.
    SetNull,
    /// Apply referencing-column defaults.
    SetDefault,
}

/// A complete simple-match foreign key definition.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ForeignKey {
    /// Named PostgreSQL constraint.
    pub name: String,
    /// Referencing column positions.
    pub columns: Vec<usize>,
    /// Stable referenced catalog table identifier.
    pub referenced_table: String,
    /// Referenced column positions in corresponding order.
    pub referenced_columns: Vec<usize>,
    /// Declared update action.
    pub on_update: ReferentialAction,
    /// Declared delete action.
    pub on_delete: ReferentialAction,
    /// Whether enforcement may occur at a later boundary.
    pub deferrable: bool,
    /// Whether this constraint starts deferred.
    pub initially_deferred: bool,
}

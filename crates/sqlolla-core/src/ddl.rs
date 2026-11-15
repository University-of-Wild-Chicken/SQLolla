//! Typed catalog definitions and explicit DDL plans; stored SQL is reparsed by the pinned adapter.
use crate::{Column, TableSemantics};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

/// Same-session current_schema() evidence, distinct from implicit effective lookup namespaces.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CreationSchema {
    /// None records that no explicit usable creation namespace exists.
    pub name: Option<String>,
}

/// Complete non-system namespace evidence includes names occupied by indexes and sequences.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CatalogNamespace {
    /// Every schema and every non-system relation-namespace object has been supplied.
    pub complete: bool,
    /// Schema names mapped to occupied names; refused system schemas retain existence only.
    pub schemas: BTreeMap<String, BTreeSet<String>>,
    /// Names known to be views, including definitions outside the admitted read context.
    #[serde(default)]
    pub views: BTreeMap<String, BTreeSet<String>>,
    /// Normal dependency tokens keyed by target schema/name; internal auto-dependencies are excluded.
    #[serde(default)]
    pub dependents: BTreeMap<String, BTreeMap<String, BTreeSet<String>>>,
}

/// A stored view retains its defining SQL and creation-time name-resolution context.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ViewDefinition {
    /// Exact schema containing the view.
    pub schema: String,
    /// Exact unqualified relation name.
    pub name: String,
    /// Complete ordered output labels after applying the declared alias prefix.
    pub columns: Vec<String>,
    /// Search path captured when the definition was established.
    pub search_path: Vec<String>,
    /// Direct relation identities fixed at creation, preventing later search-path shadowing.
    pub resolved_relations: Vec<(String, String)>,
    /// Original SELECT or CREATE VIEW SQL, never a caller-supplied bound plan.
    pub source_sql: String,
    /// Internal pinned parser envelope regenerated from source_sql; external input rejects this field.
    #[serde(skip)]
    pub prepared: Option<serde_json::Value>,
}

/// CREATE TABLE records whether an occupied identity made declaration analysis unnecessary.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct CreateTablePlan {
    /// Resolved existing namespace.
    pub schema: String,
    /// Exact table name within that namespace.
    pub name: String,
    /// Physical output positions; empty only when skip_existing is true.
    pub columns: Vec<Column>,
    /// Complete admitted constraints/defaults for a newly created table.
    pub semantics: TableSemantics,
    /// Explicit SQL IF NOT EXISTS flag.
    pub if_not_exists: bool,
    /// Binding observed an occupied name; execution rechecks that guard before skipping.
    pub skip_existing: bool,
}

/// CREATE VIEW installs a definition without executing or materializing its SELECT.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct CreateViewPlan {
    /// Complete stored definition and its prepared syntax provenance.
    pub definition: ViewDefinition,
    /// Direct physical-table and named-view dependencies resolved while binding the definition.
    pub dependencies: Vec<(String, String)>,
}

/// Produce a portable unambiguous token for a view dependency rather than using a process-local OID.
pub fn view_dependency(schema: &str, name: &str) -> String {
    format!("view:{}", serde_json::json!([schema, name]))
}

/// DROP VIEW targets qualified identities and preserves its explicit failure policy.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct DropViewPlan {
    /// Ordered schema/name pairs supplied by deterministic name resolution.
    pub names: Vec<(String, String)>,
    /// Missing targets are skipped only when explicitly requested.
    pub if_exists: bool,
    /// Cascade is preserved even when the current execution profile refuses it.
    pub cascade: bool,
}

//! Context-backed relation lookup over an explicit complete catalog inventory.
mod names;
use crate::json::error;
use crate::{scalar, Cell, Result, SqlType};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

/// One actual relation identity, including indexes, sequences and views.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RelationIdentity {
    /// Exact namespace identifier.
    pub schema: String,
    /// Exact relation identifier.
    pub name: String,
    /// Unsigned PostgreSQL object identity.
    pub oid: u32,
}

/// Complete visible namespace inventory and the effective PostgreSQL search path.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CatalogLookup {
    /// Caller attests that no relevant relation identity has been omitted.
    pub complete: bool,
    /// Current database identity for three-part relation references.
    pub current_database: String,
    /// Effective searchable schemas, including implicit system/temp schemas, in lookup order.
    pub search_path: Vec<String>,
    /// Every existing namespace and the current role's effective USAGE permission.
    pub schema_usage: BTreeMap<String, bool>,
    /// Every catalog relation identity; duplicate names or object identities are invalid.
    pub relations: Vec<RelationIdentity>,
}

impl CatalogLookup {
    /// Validate structural completeness without pretending to authenticate an external server.
    pub(crate) fn validate(&self) -> Result<()> {
        if !self.complete {
            return Err(error(
                "unsupported.catalog_lookup",
                "Complete relation inventory required",
            ));
        }
        if self.relations.len() > 4096 || self.search_path.len() > 256 {
            return Err(error(
                "resource.catalog_lookup",
                "Relation inventory exceeds declared capacity",
            ));
        }
        self.validate_relations()?;
        if !identifier(&self.current_database)
            || self.schema_usage.keys().any(|name| !identifier(name))
            || self
                .search_path
                .iter()
                .any(|name| self.schema_usage.get(name) != Some(&true))
        {
            return Err(error(
                "invalid.catalog_lookup",
                "Invalid database or effective schema name",
            ));
        }
        Ok(())
    }

    /// Duplicate identities or qualified names would make relation resolution ambiguous.
    fn validate_relations(&self) -> Result<()> {
        let mut names = BTreeSet::new();
        let mut identities = BTreeSet::new();
        for relation in &self.relations {
            if !identifier(&relation.schema)
                || !identifier(&relation.name)
                || relation.oid == 0
                || !self.schema_usage.contains_key(&relation.schema)
                || !names.insert((&relation.schema, &relation.name))
                || !identities.insert(relation.oid)
            {
                return Err(error(
                    "invalid.catalog_lookup",
                    "Invalid or duplicate relation identity",
                ));
            }
        }
        Ok(())
    }
}

/// Stable and effectful calls must not be evaluated by immutable constant folding.
pub(crate) fn requires_context(name: &str) -> bool {
    matches!(
        name,
        "to_regclass" | "pg_terminate_backend" | "nextval" | "__sqlolla_regclass_input"
    )
}

/// Binding and execution must agree on the exact object identities used by regclass literals.
pub(crate) fn merge(catalog: &crate::Catalog, context: &mut crate::ExecutionContext) -> Result<()> {
    validate_catalog(catalog)?;
    if let Some(lookup) = &catalog.relation_lookup {
        if context
            .catalog_lookup
            .as_ref()
            .is_some_and(|value| value != lookup.as_ref())
        {
            return Err(error(
                "invalid.catalog_lookup",
                "Binding and execution inventories disagree",
            ));
        }
        context.catalog_lookup = Some((**lookup).clone());
    }
    Ok(())
}

/// A declared inventory carries the same effective search path as the binding catalog.
pub(crate) fn validate_catalog(catalog: &crate::Catalog) -> Result<()> {
    if let Some(lookup) = &catalog.relation_lookup {
        lookup.validate()?;
        if lookup.search_path != catalog.search_path {
            return Err(error(
                "invalid.catalog_search_path",
                "Binding object lookup path disagrees",
            ));
        }
    }
    Ok(())
}

/// Strict regclass input keeps missing-context admission separate from missing-relation SQL errors.
pub(crate) fn input(value: &Cell, context: Option<&CatalogLookup>) -> Result<Cell> {
    if value.is_null() {
        return Ok(Cell::Null(SqlType::Regclass));
    }
    let text = name_text(value)?;
    if text == "-" || (!text.is_empty() && text.bytes().all(|value| value.is_ascii_digit())) {
        return scalar::input(&SqlType::Regclass, if text == "-" { "0" } else { &text });
    }
    let context =
        context.ok_or_else(|| error("unsupported.catalog_lookup", "Relation inventory omitted"))?;
    context.validate()?;
    let parts = names::split(&text)?
        .filter(|parts| !parts.is_empty())
        .ok_or_else(|| {
            error(
                "unsupported.regclass_name_syntax",
                "Strict object-name syntax needs its error profile",
            )
        })?;
    let oid =
        lookup(&parts, context)?.ok_or_else(|| error("sql.42P01", "Relation does not exist"))?;
    scalar::input(&SqlType::Regclass, &oid.to_string())
}

/// Resolve a strict to_regclass call without consulting any live host or database state.
pub(crate) fn resolve(value: &Cell, context: Option<&CatalogLookup>) -> Result<Cell> {
    if value.is_null() {
        return Ok(Cell::Null(SqlType::Regclass));
    }
    let text = name_text(value)?;
    if text == "-" {
        return scalar::input(&SqlType::Regclass, "0");
    }
    if !text.is_empty() && text.bytes().all(|value| value.is_ascii_digit()) {
        return numeric(&text);
    }
    let Some(parts) = names::split(&text)? else {
        return Ok(Cell::Null(SqlType::Regclass));
    };
    if parts.is_empty() {
        return Ok(Cell::Null(SqlType::Regclass));
    }
    let context =
        context.ok_or_else(|| error("unsupported.catalog_lookup", "Relation inventory omitted"))?;
    context.validate()?;
    let oid = lookup(&parts, context)?;
    oid.map_or(Ok(Cell::Null(SqlType::Regclass)), |oid| {
        scalar::input(&SqlType::Regclass, &oid.to_string())
    })
}

/// Name lookup shares the scalar byte budget before allocating decoded identifier pieces.
fn name_text(value: &Cell) -> Result<String> {
    let text = scalar::output(value)?;
    if text.len() > 1_048_576 {
        return Err(error(
            "resource.catalog_name",
            "Relation name exceeds byte budget",
        ));
    }
    Ok(text)
}

/// PostgreSQL's safe regclass input converts numeric input failures into NULL.
fn numeric(text: &str) -> Result<Cell> {
    match scalar::input(&SqlType::Regclass, text) {
        Err(failure) if failure.code.starts_with("sql.") => Ok(Cell::Null(SqlType::Regclass)),
        result => result,
    }
}

/// Qualifiers and path order select object identity; absent names remain NULL.
fn lookup(parts: &[String], context: &CatalogLookup) -> Result<Option<u32>> {
    match parts {
        [] => Ok(None),
        [name] => unqualified(context, name),
        [schema, name] => qualified(context, schema, name),
        [database, schema, name] if database == &context.current_database => {
            qualified(context, schema, name)
        }
        [_, _, _] => Err(error(
            "sql.0A000",
            "Cross-database references are not implemented",
        )),
        _ => Err(error(
            "sql.42601",
            "Improper relation name: too many dotted names",
        )),
    }
}

/// Exact identifiers retain case and punctuation after the dedicated identifier parser.
fn qualified(context: &CatalogLookup, schema: &str, name: &str) -> Result<Option<u32>> {
    match context.schema_usage.get(schema) {
        None => return Ok(None),
        Some(false) => return Err(error("sql.42501", "Permission denied for schema")),
        Some(true) => {}
    }
    Ok(context
        .relations
        .iter()
        .find(|relation| relation.schema == schema && relation.name == name)
        .map(|relation| relation.oid))
}

/// Only effective searchable namespaces participate in unqualified relation resolution.
fn unqualified(context: &CatalogLookup, name: &str) -> Result<Option<u32>> {
    for schema in &context.search_path {
        if let Some(oid) = qualified(context, schema, name)? {
            return Ok(Some(oid));
        }
    }
    Ok(None)
}

/// Catalog identifiers are nonempty UTF-8 names already truncated by the source catalog.
fn identifier(value: &str) -> bool {
    !value.is_empty() && value.len() <= 63 && !value.contains('\0')
}

//! Structural context admission and prestate constraint admissibility.
use crate::json::error;
use crate::*;
use std::collections::BTreeSet;

/// Validate finite resources and declared metadata without treating SQL violations as engine failure.
pub fn validate_execution_context(
    catalog: &Catalog,
    database: &Database,
    context: &ExecutionContext,
) -> Result<()> {
    crate::execution::validate_context(context)?;
    if let Some(sequences) = &context.sequences {
        crate::sequence::validate(sequences)?;
    }
    validate_catalog_lookup(catalog, context)?;
    validate_database(catalog, database)?;
    if let Some(tuples) = &context.tuples {
        tuples.validate(catalog, database)?;
    }
    validate_locks(catalog, database, context)?;
    for (id, metadata) in &context.table_metadata {
        let table = catalog
            .tables
            .get(id)
            .ok_or_else(|| error("invalid.metadata_table", "Unknown metadata table"))?;
        validate_table(metadata, table, catalog)?;
    }
    Ok(())
}

/// One statement cannot use contradictory effective search paths or database identities.
fn validate_catalog_lookup(catalog: &Catalog, context: &ExecutionContext) -> Result<()> {
    let Some(lookup) = &context.catalog_lookup else {
        return Ok(());
    };
    lookup.validate()?;
    if lookup.search_path != catalog.search_path {
        return Err(error(
            "invalid.catalog_search_path",
            "Lookup and binding effective paths disagree",
        ));
    }
    if let Some(database) = context.settings.get("current_database") {
        if database.sql_type() != SqlType::Name
            || database.is_null()
            || scalar::output(database)? != lookup.current_database
        {
            return Err(error(
                "invalid.catalog_database",
                "Lookup and session database identities disagree",
            ));
        }
    }
    Ok(())
}

/// Lock occurrence identities are meaningful only inside the supplied closed physical snapshot.
fn validate_locks(
    catalog: &Catalog,
    database: &Database,
    context: &ExecutionContext,
) -> Result<()> {
    let Some(locks) = &context.locks else {
        return Ok(());
    };
    validate_lock_completeness(locks)?;
    for held in &locks.held_rows {
        validate_lock_owner(held, catalog, database)?;
    }
    if locks
        .row_share_conflicts
        .iter()
        .any(|table| !catalog.tables.contains_key(table))
    {
        return Err(error("invalid.lock_table", "Unknown relation lock table"));
    }
    validate_lock_ordinals(locks, database)
}

/// Structurally valid lock records may refer to rows absent in a particular symbolic input.
fn validate_lock_ordinals(locks: &LockContext, database: &Database) -> Result<()> {
    if locks
        .held_rows
        .iter()
        .any(|held| held.occurrence >= database[&held.table].len())
    {
        return Err(error(
            "invalid.lock_ordinal",
            "Held occurrence is absent from this input state",
        ));
    }
    Ok(())
}

/// Partial ownership evidence cannot establish absence of conflicting locks.
fn validate_lock_completeness(locks: &LockContext) -> Result<()> {
    if !locks.complete || locks.transaction.is_empty() {
        return Err(error(
            "unsupported.lock_context",
            "Complete lock ownership snapshot required",
        ));
    }
    Ok(())
}

/// Validate structural lock evidence independently of whether one finite input contains the row.
fn validate_lock_owner(held: &HeldRowLock, catalog: &Catalog, database: &Database) -> Result<()> {
    if !catalog.tables.contains_key(&held.table) || !database.contains_key(&held.table) {
        return Err(error(
            "invalid.lock_table",
            "Lock table missing from closed catalog or snapshot",
        ));
    }
    if held.owner.is_empty() {
        return Err(error("invalid.lock_owner", "Empty transaction owner"));
    }
    Ok(())
}

/// Reject invalid initial states separately from SQL errors produced by a later mutation.
pub fn admissible_initial_state(
    catalog: &Catalog,
    database: &Database,
    context: &ExecutionContext,
) -> Result<bool> {
    match validate_execution_context(catalog, database, context) {
        Err(failure) if failure.code == "invalid.lock_ordinal" => return Ok(false),
        result => result?,
    }
    match initial_constraints(catalog, database, context) {
        Ok(()) => Ok(true),
        Err(failure) if failure.code.starts_with("sql.") => Ok(false),
        Err(failure) => Err(failure),
    }
}

/// Validate each CHECK against actual declared column identities in the initial catalog.
fn initial_constraints(
    catalog: &Catalog,
    database: &Database,
    context: &ExecutionContext,
) -> Result<()> {
    crate::constraints::database(database, context)?;
    for (id, metadata) in &context.table_metadata {
        let rows = database
            .get(id)
            .ok_or_else(|| error("invalid.database", "Constraint table missing"))?;
        let table = &catalog.tables[id];
        crate::constraints::checks(rows, metadata, &table.columns, context)?;
        crate::unique_index::validate(rows, metadata, &table.columns, context)?;
    }
    Ok(())
}

/// A target's completeness assertion is mandatory before admitting any write.
pub(crate) fn mutation_metadata<'a>(
    target: &MutationTarget,
    context: &'a ExecutionContext,
) -> Result<&'a TableSemantics> {
    crate::constraints::require_statement_timing(context)?;
    let metadata = context
        .table_metadata
        .get(&target.table)
        .filter(|value| value.complete)
        .ok_or_else(|| {
            error(
                "unsupported.mutation_context",
                "Complete table semantics are required",
            )
        })?;
    if metadata.has_triggers || metadata.row_security || metadata.has_inheritance_or_partitioning {
        return Err(error(
            "unsupported.table_effects",
            "Trigger, RLS or inheritance effects need a model",
        ));
    }
    Ok(metadata)
}

/// Validate column positions, constraint arities and declared default targets.
fn validate_table(metadata: &TableSemantics, table: &Table, catalog: &Catalog) -> Result<()> {
    if metadata.row_security || metadata.has_inheritance_or_partitioning {
        return Err(error(
            "unsupported.table_visibility",
            "RLS, inheritance and partition visibility require a model",
        ));
    }
    for index in metadata.defaults.keys() {
        indices(&[*index], table.columns.len())?;
    }
    for key in metadata.primary_key.iter().chain(&metadata.unique) {
        indices(&key.columns, table.columns.len())?;
        if key.initially_deferred && !key.deferrable {
            return Err(error(
                "invalid.constraint_timing",
                "Nondeferrable constraint starts deferred",
            ));
        }
    }
    for key in &metadata.foreign_keys {
        validate_foreign(key, table, catalog)?;
    }
    validate_indexes(metadata, table)?;
    Ok(())
}

/// Standalone indexes require valid key positions and cannot pretend to be deferred constraints.
fn validate_indexes(metadata: &TableSemantics, table: &Table) -> Result<()> {
    crate::constraint_order::rules(metadata)?;
    let mut names = BTreeSet::new();
    for index in &metadata.unique_indexes {
        indices(&index.key.columns, table.columns.len())?;
        if index.key.deferrable || index.key.initially_deferred || !names.insert(&index.key.name) {
            return Err(error(
                "invalid.unique_index",
                "Invalid index timing or duplicate index identity",
            ));
        }
    }
    Ok(())
}

/// Validate foreign-key timing and both sides of its positional key mapping.
fn validate_foreign(key: &ForeignKey, table: &Table, catalog: &Catalog) -> Result<()> {
    if key.initially_deferred && !key.deferrable {
        return Err(error(
            "invalid.constraint_timing",
            "Nondeferrable foreign key starts deferred",
        ));
    }
    indices(&key.columns, table.columns.len())?;
    let other = catalog
        .tables
        .get(&key.referenced_table)
        .ok_or_else(|| error("invalid.foreign_key", "Unknown referenced table"))?;
    indices(&key.referenced_columns, other.columns.len())?;
    if key.columns.len() != key.referenced_columns.len() {
        return Err(error("invalid.foreign_key", "Foreign key arity mismatch"));
    }
    Ok(())
}

/// Require a nonempty, distinct, in-range sequence of physical columns.
fn indices(columns: &[usize], width: usize) -> Result<()> {
    let unique = columns.iter().collect::<BTreeSet<_>>();
    if columns.is_empty() || unique.len() != columns.len() || columns.iter().any(|i| *i >= width) {
        return Err(error(
            "invalid.constraint_columns",
            "Invalid constraint column positions",
        ));
    }
    Ok(())
}

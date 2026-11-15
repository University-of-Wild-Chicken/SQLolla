//! Explicit catalog transitions preserve physical bags and never execute a stored view definition.
use super::*;
use crate::binding::ddl::namespace;
use std::collections::BTreeSet;

/// Merge persisted constraints into execution context and reject contradictory duplicate declarations.
pub fn merged_catalog_context(
    catalog: &Catalog,
    context: &ExecutionContext,
) -> Result<ExecutionContext> {
    let mut merged = context.clone();
    crate::catalog_lookup::merge(catalog, &mut merged)?;
    crate::sql_function::merge(catalog, &mut merged)?;
    for (id, semantics) in &catalog.table_semantics {
        if merged
            .table_metadata
            .get(id)
            .is_some_and(|value| value != semantics)
        {
            return Err(error(
                "invalid.catalog_semantics",
                "Execution metadata contradicts stored table semantics",
            ));
        }
        merged.table_metadata.insert(id.clone(), semantics.clone());
    }
    Ok(merged)
}

/// Evaluate a statement with an explicit catalog poststate while preserving the legacy API.
pub fn evaluate_with_catalog(
    plan: &StatementPlan,
    catalog: &Catalog,
    database: &Database,
    context: &ExecutionContext,
) -> Result<OutcomeSet> {
    let context = merged_catalog_context(catalog, context)?;
    validate_execution_context(catalog, database, &context)?;
    let transition = match plan {
        StatementPlan::CreateTable(plan) => Some(create_table(plan, catalog, database)),
        StatementPlan::CreateView(plan) => Some(create_view(plan, catalog, database)),
        StatementPlan::DropView(plan) => Some(drop_view(plan, catalog, database)),
        _ => None,
    };
    if let Some(result) = transition {
        return transition_outcome(result);
    }
    let mut outcomes = evaluate_statement(plan, database, &context)?;
    for outcome in &mut outcomes.outcomes {
        if let StatementOutcome::Succeeded(value) = outcome {
            value.catalog = Some(Box::new(catalog.clone()));
        }
    }
    Ok(outcomes)
}

/// SQL errors retain the normal statement failure channel; coverage failures remain outer errors.
fn transition_outcome(result: Result<StatementSuccess>) -> Result<OutcomeSet> {
    let outcome = match result {
        Ok(value) => StatementOutcome::Succeeded(value),
        Err(failure) => match execution_error(failure)? {
            Err(sql) => StatementOutcome::SqlError(*sql),
            Ok(_) => return Err(error("invalid.ddl_result", "Unexpected relational success")),
        },
    };
    Ok(OutcomeSet {
        outcomes: vec![outcome],
    })
}

/// Catalog DDL returns no result rows and does not report affected table occurrences.
fn success(catalog: Catalog, database: Database, tag: &str) -> StatementSuccess {
    StatementSuccess {
        catalog: Some(Box::new(catalog)),
        database,
        rows: vec![],
        ordering: ResultOrdering::Bag,
        affected: 0,
        command_tag: tag.into(),
        effects: vec![],
    }
}

/// Create a new empty physical relation, or preserve every byte of an existing skipped target.
fn create_table(
    plan: &CreateTablePlan,
    catalog: &Catalog,
    database: &Database,
) -> Result<StatementSuccess> {
    let occupied = namespace(catalog)?
        .schemas
        .get(&plan.schema)
        .is_some_and(|names| names.contains(&plan.name));
    if occupied {
        if plan.if_not_exists {
            return Ok(success(catalog.clone(), database.clone(), "CREATE TABLE"));
        }
        return Err(error("sql.42P07", "Relation already exists"));
    }
    if plan.skip_existing {
        return Err(error(
            "invalid.stale_ddl",
            "Skipped target disappeared after binding",
        ));
    }
    install_table(plan, catalog, database)
}

/// Install the physical table, empty bag, persisted constraints and backing-index namespace names.
fn install_table(
    plan: &CreateTablePlan,
    catalog: &Catalog,
    database: &Database,
) -> Result<StatementSuccess> {
    let mut catalog = catalog.clone();
    catalog.relation_lookup = None;
    let mut database = database.clone();
    let id = format!("{}.{}", plan.schema, plan.name);
    unused_id(&catalog, &id)?;
    if database.contains_key(&id) {
        return Err(error("invalid.catalog_id", "Database identity occupied"));
    }
    catalog.tables.insert(
        id.clone(),
        Table {
            schema: plan.schema.clone(),
            name: plan.name.clone(),
            columns: plan.columns.clone(),
        },
    );
    catalog
        .table_semantics
        .insert(id.clone(), plan.semantics.clone());
    database.insert(id, vec![]);
    table_namespace(plan, mutable_namespace(&mut catalog)?)?;
    Ok(success(catalog, database, "CREATE TABLE"))
}

/// Reject stale plans whose declared creation schema disappeared before evaluation.
fn table_namespace(plan: &CreateTablePlan, namespace: &mut CatalogNamespace) -> Result<()> {
    let names = namespace
        .schemas
        .get_mut(&plan.schema)
        .ok_or_else(|| error("invalid.stale_ddl", "Creation schema disappeared"))?;
    names.insert(plan.name.clone());
    for key in plan
        .semantics
        .primary_key
        .iter()
        .chain(&plan.semantics.unique)
    {
        names.insert(key.name.clone());
    }
    Ok(())
}

/// Generated stable identifiers must not collide with unrelated caller-assigned identifiers.
fn unused_id(catalog: &Catalog, id: &str) -> Result<()> {
    if catalog.tables.contains_key(id) || catalog.views.contains_key(id) {
        return Err(error(
            "invalid.catalog_id",
            "Generated relation identity occupied",
        ));
    }
    Ok(())
}

/// Return explicit namespace state without panicking on a malformed direct core call.
fn mutable_namespace(catalog: &mut Catalog) -> Result<&mut CatalogNamespace> {
    catalog
        .namespace
        .as_mut()
        .ok_or_else(|| error("unsupported.ddl_namespace", "Namespace evidence required"))
}

/// Install the definition and dependency edges without materializing its query.
fn create_view(
    plan: &CreateViewPlan,
    catalog: &Catalog,
    database: &Database,
) -> Result<StatementSuccess> {
    let view = &plan.definition;
    let names = namespace(catalog)?
        .schemas
        .get(&view.schema)
        .ok_or_else(|| error("sql.3F000", "Schema does not exist"))?;
    if names.contains(&view.name) {
        return Err(error("sql.42P07", "Relation already exists"));
    }
    let mut catalog = catalog.clone();
    catalog.relation_lookup = None;
    let id = format!("{}.{}", view.schema, view.name);
    unused_id(&catalog, &id)?;
    catalog.views.insert(id, view.clone());
    view_namespace(plan, mutable_namespace(&mut catalog)?)?;
    Ok(success(catalog, database.clone(), "CREATE VIEW"))
}

/// A new view adds only its own name and direct normal-dependency tokens.
fn view_namespace(plan: &CreateViewPlan, namespace: &mut CatalogNamespace) -> Result<()> {
    let view = &plan.definition;
    namespace
        .schemas
        .get_mut(&view.schema)
        .ok_or_else(|| error("invalid.stale_ddl", "Creation schema disappeared"))?
        .insert(view.name.clone());
    namespace
        .views
        .entry(view.schema.clone())
        .or_default()
        .insert(view.name.clone());
    for (schema, name) in &plan.dependencies {
        namespace
            .dependents
            .entry(schema.clone())
            .or_default()
            .entry(name.clone())
            .or_default()
            .insert(view_dependency(&view.schema, &view.name));
    }
    Ok(())
}

/// Validate the entire DROP set before removing any definition or dependency edge.
fn drop_view(
    plan: &DropViewPlan,
    catalog: &Catalog,
    database: &Database,
) -> Result<StatementSuccess> {
    if plan.cascade {
        return Err(error(
            "unsupported.ddl_cascade",
            "Dependent transitions are not admitted",
        ));
    }
    let targets = plan.names.iter().cloned().collect::<BTreeSet<_>>();
    let tokens = targets
        .iter()
        .map(|(schema, name)| view_dependency(schema, name))
        .collect::<BTreeSet<_>>();
    for (schema, name) in &targets {
        validate_drop(plan, catalog, schema, name, &tokens)?;
    }
    let mut catalog = catalog.clone();
    remove_definitions(&mut catalog, &targets);
    remove_namespace(mutable_namespace(&mut catalog)?, &targets, &tokens);
    Ok(success(catalog, database.clone(), "DROP VIEW"))
}

/// Removing an actual definition invalidates object identities; a skipped DROP preserves evidence.
fn remove_definitions(catalog: &mut Catalog, targets: &BTreeSet<(String, String)>) {
    let previous = catalog.views.len();
    catalog
        .views
        .retain(|_, view| !targets.contains(&(view.schema.clone(), view.name.clone())));
    if catalog.views.len() != previous {
        catalog.relation_lookup = None;
    }
}

/// Remove names and incoming/outgoing dependency edges only after the complete DROP set validates.
fn remove_namespace(
    namespace: &mut CatalogNamespace,
    targets: &BTreeSet<(String, String)>,
    tokens: &BTreeSet<String>,
) {
    for (schema, name) in targets {
        if let Some(names) = namespace.schemas.get_mut(schema) {
            names.remove(name);
        }
        if let Some(names) = namespace.views.get_mut(schema) {
            names.remove(name);
        }
        if let Some(names) = namespace.dependents.get_mut(schema) {
            names.remove(name);
        }
    }
    for relations in namespace.dependents.values_mut() {
        for uses in relations.values_mut() {
            uses.retain(|token| !tokens.contains(token));
        }
    }
    namespace.views.retain(|_, names| !names.is_empty());
    for relations in namespace.dependents.values_mut() {
        relations.retain(|_, uses| !uses.is_empty());
    }
    namespace
        .dependents
        .retain(|_, relations| !relations.is_empty());
}

/// Distinguish missing, wrong-kind and unmodeled dependent objects under complete namespace evidence.
fn validate_drop(
    plan: &DropViewPlan,
    catalog: &Catalog,
    schema: &str,
    name: &str,
    removed: &BTreeSet<String>,
) -> Result<()> {
    let namespace = namespace(catalog)?;
    if !namespace
        .schemas
        .get(schema)
        .is_some_and(|names| names.contains(name))
    {
        return if plan.if_exists {
            Ok(())
        } else {
            Err(error("sql.42P01", "View does not exist"))
        };
    }
    validate_present_view(catalog, namespace, schema, name, removed)
}

/// Present targets require view kind evidence, a prepared definition and no retained dependents.
fn validate_present_view(
    catalog: &Catalog,
    namespace: &CatalogNamespace,
    schema: &str,
    name: &str,
    removed: &BTreeSet<String>,
) -> Result<()> {
    if !namespace
        .views
        .get(schema)
        .is_some_and(|names| names.contains(name))
    {
        return Err(error("sql.42809", "Relation is not a view"));
    }
    require_view_definition(catalog, schema, name)?;
    if namespace
        .dependents
        .get(schema)
        .and_then(|names| names.get(name))
        .is_some_and(|uses| uses.iter().any(|token| !removed.contains(token)))
    {
        return Err(error("sql.2BP01", "Other objects depend on this view"));
    }
    Ok(())
}

/// An actual view outside the modeled catalog cannot be deleted with a fabricated definition.
fn require_view_definition(catalog: &Catalog, schema: &str, name: &str) -> Result<()> {
    if !catalog
        .views
        .values()
        .any(|view| view.schema == schema && view.name == name)
    {
        return Err(error(
            "unsupported.unprepared_view",
            "Dropped view definition was not supplied",
        ));
    }
    Ok(())
}

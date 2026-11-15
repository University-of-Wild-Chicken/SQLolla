//! Source-aware DDL binding preserves stored definitions and complete namespace assumptions.
mod table;
mod view;
use super::*;
use std::collections::BTreeSet;

/// Bind a source-backed statement; CREATE VIEW retains exact SQL for future deterministic reparsing.
pub fn bind_statement_with_source(
    report: &Value,
    sql: &str,
    catalog: &Catalog,
    params: &Parameters,
) -> Result<StatementPlan> {
    super::validate_parameters(params)?;
    validate_catalog_extensions(catalog)?;
    let tag = report["result"]["ast"]["tag"].as_str().unwrap_or("");
    if !matches!(tag, "create_table" | "create_view" | "drop_view") {
        return super::bind_statement(report, catalog, params);
    }
    let result = bind_ddl(report, sql, catalog, params);
    match result {
        Ok(plan) => Ok(plan),
        Err(failure) => super::rejected_statement(failure),
    }
}

/// Every DDL syntax node still passes the pinned envelope and closed catalog gates.
fn bind_ddl(
    report: &Value,
    sql: &str,
    catalog: &Catalog,
    params: &Parameters,
) -> Result<StatementPlan> {
    normalize::validate_report(report)?;
    source::validate_catalog(catalog)?;
    namespace(catalog)?;
    let scope = binder(catalog, params, true);
    let node = &report["result"]["ast"];
    let result = match text(node, "tag")? {
        "create_table" => scope.create_table(node).map(StatementPlan::CreateTable),
        "create_view" => scope
            .create_view(node, sql, report)
            .map(StatementPlan::CreateView),
        "drop_view" => scope.drop_view(node).map(StatementPlan::DropView),
        tag => Err(error("unsupported.statement", tag)),
    };
    if scope.locking_seen.get() {
        return Err(error(
            "unsupported.ddl_initialization",
            "DDL relation opening requires its own lock contract",
        ));
    }
    result
}

/// Validate added catalog definitions even when a query does not reference them directly.
pub(in crate::binding) fn validate_catalog_extensions(catalog: &Catalog) -> Result<()> {
    validate_creation_schema(catalog)?;
    if catalog.views.len() > 64 {
        return Err(error("resource.catalog_views", "View count exceeds 64"));
    }
    let mut names = catalog
        .tables
        .values()
        .map(|table| (table.schema.clone(), table.name.clone()))
        .collect::<BTreeSet<_>>();
    for view in catalog.views.values() {
        if !names.insert((view.schema.clone(), view.name.clone())) {
            return Err(error("invalid.catalog", "Duplicate table/view identity"));
        }
        validate_view_definition(view)?;
    }
    for id in catalog.table_semantics.keys() {
        if !catalog.tables.contains_key(id) {
            return Err(error(
                "invalid.catalog_semantics",
                "Stored constraints refer to unknown table",
            ));
        }
    }
    Ok(())
}

/// Keep stored source and lexical context bounded before preparing or expanding it.
fn validate_view_definition(view: &ViewDefinition) -> Result<()> {
    if view.source_sql.is_empty() || view.source_sql.len() > 65_536 || view.search_path.is_empty() {
        return Err(error(
            "invalid.view_definition",
            "Missing or excessive view source/context",
        ));
    }
    unique_names(&view.columns)
}

/// Missing namespace evidence is a coverage refusal, not evidence that no object collides.
pub(crate) fn namespace(catalog: &Catalog) -> Result<&CatalogNamespace> {
    let namespace = catalog
        .namespace
        .as_ref()
        .filter(|value| value.complete)
        .ok_or_else(|| {
            error(
                "unsupported.ddl_namespace",
                "Complete schema/relation namespace and dependency evidence required",
            )
        })?;
    for table in catalog.tables.values() {
        require_name(namespace, &table.schema, &table.name)?;
    }
    for view in catalog.views.values() {
        require_name(namespace, &view.schema, &view.name)?;
        if !namespace
            .views
            .get(&view.schema)
            .is_some_and(|names| names.contains(&view.name))
        {
            return Err(error(
                "invalid.ddl_namespace",
                "Declared view absent from namespace kind evidence",
            ));
        }
    }
    Ok(namespace)
}

/// Declared table/view identities must agree with the supplied complete namespace.
fn require_name(namespace: &CatalogNamespace, schema: &str, name: &str) -> Result<()> {
    if schema.starts_with("pg_") || schema == "information_schema" {
        return Ok(());
    }
    if !namespace
        .schemas
        .get(schema)
        .is_some_and(|names| names.contains(name))
    {
        return Err(error(
            "invalid.ddl_namespace",
            "Declared relation absent from namespace",
        ));
    }
    Ok(())
}

/// Restrict identifiers before PostgreSQL's byte-length truncation could change their identity.
fn identifier(name: &str) -> Result<String> {
    if name.is_empty() || name.contains('\0') {
        return Err(error("sql.42602", "Invalid relation identifier"));
    }
    if name.len() > 63 {
        return Err(error(
            "unsupported.identifier_truncation",
            "Identifier exceeds PostgreSQL name width",
        ));
    }
    Ok(name.into())
}

/// Stored output labels remain nonempty, unique and positionally bounded.
fn unique_names(names: &[String]) -> Result<()> {
    if names.is_empty() || names.len() > 64 {
        return Err(error(
            "unsupported.ddl_columns",
            "Column count must be 1..64",
        ));
    }
    let mut seen = BTreeSet::new();
    for name in names {
        identifier(name)?;
        if !seen.insert(name) {
            return Err(error("sql.42701", "Duplicate column name"));
        }
    }
    Ok(())
}

/// A declared schema is required before CREATE may use an explicit or search-path name.
fn creation_schema(catalog: &Catalog, explicit: Option<&str>) -> Result<String> {
    let namespace = namespace(catalog)?;
    let schema = explicit
        .map(str::to_owned)
        .map_or_else(|| default_creation_schema(catalog), Ok)?;
    if !namespace.schemas.contains_key(&schema) {
        return Err(error("sql.3F000", "Schema does not exist"));
    }
    if schema.starts_with("pg_") || schema == "information_schema" {
        return Err(error(
            "unsupported.ddl_system_schema",
            "System-schema DDL requires additional catalog semantics",
        ));
    }
    identifier(&schema)
}

/// Effective lookup paths cannot reconstruct whether pg_catalog was explicit in search_path.
fn default_creation_schema(catalog: &Catalog) -> Result<String> {
    if let Some(observed) = &catalog.creation_schema {
        return observed
            .name
            .clone()
            .ok_or_else(|| error("sql.3F000", "No schema selected for creation"));
    }
    if catalog.relation_lookup.is_some() {
        return Err(error(
            "unsupported.ddl_creation_context",
            "current_schema() evidence required with effective lookup context",
        ));
    }
    let namespace = namespace(catalog)?;
    catalog
        .search_path
        .iter()
        .find(|name| namespace.schemas.contains_key(*name))
        .cloned()
        .ok_or_else(|| error("sql.3F000", "No schema selected for creation"))
}

/// Supplied creation identity must be a valid existing namespace in the effective lookup context.
fn validate_creation_schema(catalog: &Catalog) -> Result<()> {
    let Some(name) = catalog
        .creation_schema
        .as_ref()
        .and_then(|value| value.name.as_ref())
    else {
        return Ok(());
    };
    if name.is_empty()
        || name.contains('\0')
        || name.len() > 63
        || !catalog.search_path.contains(name)
    {
        return Err(error(
            "invalid.creation_schema",
            "Creation schema is absent from effective lookup path",
        ));
    }
    if catalog
        .namespace
        .as_ref()
        .is_some_and(|namespace| !namespace.schemas.contains_key(name))
    {
        return Err(error(
            "invalid.creation_schema",
            "Creation schema is absent from declared namespace",
        ));
    }
    Ok(())
}

/// Resolve parser identifier arrays without splitting quoted identifiers on dots.
fn qualified_name(node: &Value) -> Result<(Option<String>, String)> {
    let names = node
        .as_array()
        .ok_or_else(|| error("invalid.ast", "Expected identifier components"))?;
    let names = names
        .iter()
        .map(|value| {
            value
                .as_str()
                .ok_or_else(|| error("invalid.ast", "Nontext identifier"))
        })
        .collect::<Result<Vec<_>>>()?;
    match names.as_slice() {
        [name] => Ok((None, identifier(name)?)),
        [schema, name] => Ok((Some(identifier(schema)?), identifier(name)?)),
        _ => Err(error(
            "unsupported.ddl_name",
            "Expected one or two identifier components",
        )),
    }
}

/// Occupancy includes indexes/sequences, not just table and view definitions.
fn occupied(catalog: &Catalog, schema: &str, name: &str) -> Result<bool> {
    Ok(namespace(catalog)?
        .schemas
        .get(schema)
        .is_some_and(|names| names.contains(name)))
}

impl Binder<'_> {
    /// Resolve all DROP targets before any catalog deletion can become visible.
    fn drop_view(&self, node: &Value) -> Result<DropViewPlan> {
        fields(node, &["tag", "names", "if_exists", "cascade"])?;
        let cascade = boolean(node, "cascade")?;
        if cascade {
            return Err(error(
                "unsupported.ddl_cascade",
                "CASCADE requires all dependent object transitions",
            ));
        }
        let names = array(node, "names")?
            .iter()
            .map(|name| self.drop_name(name))
            .collect::<Result<Vec<_>>>()?;
        if names.is_empty() {
            return Err(error("invalid.ast", "DROP VIEW requires a target"));
        }
        Ok(DropViewPlan {
            names,
            if_exists: boolean(node, "if_exists")?,
            cascade,
        })
    }

    /// Unqualified DROP searches relation namespace order, including wrong-kind objects.
    fn drop_name(&self, node: &Value) -> Result<(String, String)> {
        let (schema, name) = qualified_name(node)?;
        if let Some(schema) = schema {
            drop_occupied(self.catalog, &schema, &name)?;
            return Ok((schema, name));
        }
        for schema in &self.catalog.search_path {
            if drop_occupied(self.catalog, schema, &name)? {
                return Ok((schema.clone(), name));
            }
        }
        let schema = self
            .catalog
            .search_path
            .first()
            .ok_or_else(|| error("sql.42P01", "Relation does not exist"))?;
        Ok((schema.clone(), name))
    }
}

/// Complete lookup can prove system-name absence without authorizing any system-object transition.
fn drop_occupied(catalog: &Catalog, schema: &str, name: &str) -> Result<bool> {
    if schema.starts_with("pg_") || schema == "information_schema" {
        let lookup = catalog.relation_lookup.as_ref().ok_or_else(|| {
            error(
                "unsupported.ddl_system_schema",
                "System-name absence requires complete relation lookup",
            )
        })?;
        if lookup
            .relations
            .iter()
            .any(|relation| relation.schema == schema && relation.name == name)
        {
            return Err(error(
                "unsupported.ddl_system_schema",
                "System-object DDL requires additional catalog semantics",
            ));
        }
        return Ok(false);
    }
    occupied(catalog, schema, name)
}

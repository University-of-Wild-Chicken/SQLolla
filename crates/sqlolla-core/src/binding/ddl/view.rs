//! Stored views are rebound from pinned syntax under isolated creation-time scope.
use super::*;

impl Binder<'_> {
    /// CREATE VIEW checks the SELECT's types but never executes or constant-folds its rows.
    pub(super) fn create_view(
        &self,
        node: &Value,
        sql: &str,
        report: &Value,
    ) -> Result<CreateViewPlan> {
        view_flags(node)?;
        let (schema, name) = qualified_name(&node["name"])?;
        let schema = creation_schema(self.catalog, schema.as_deref())?;
        if occupied(self.catalog, &schema, &name)? {
            return Err(error("sql.42P07", "Relation already exists"));
        }
        let params = Parameters::new();
        let scope = isolated(self, self.catalog, &params);
        let plan = scope.query(&normalize::query(&node["query"])?, None)?;
        let plan = source::rename(plan, &name, Some(schema.clone()), array(node, "columns")?)?;
        let dependencies = dependencies(&scope)?;
        let mut definition = definition(self.catalog, schema, name, plan, sql, report)?;
        definition.resolved_relations = dependencies.clone();
        Ok(CreateViewPlan {
            definition,
            dependencies,
        })
    }

    /// Resolve a view only within this search-path schema and record its direct dependency identity.
    pub(in crate::binding) fn catalog_view(
        &self,
        name: &str,
        schema: &str,
    ) -> Result<Option<Plan>> {
        let Some((id, view)) = self
            .catalog
            .views
            .iter()
            .find(|(_, view)| view.name == name && view.schema == schema)
        else {
            return Ok(None);
        };
        self.charge()?;
        self.view_uses.borrow_mut().insert(id.clone());
        let plan = self.expand_view(id, view)?;
        let names = view
            .columns
            .iter()
            .map(|name| Value::String(name.clone()))
            .collect::<Vec<_>>();
        source::rename(plan, name, Some(schema.into()), &names).map(Some)
    }

    /// Removing the expanding definition makes self and mutual cycles terminate without caller capture.
    fn expand_view(&self, id: &str, view: &ViewDefinition) -> Result<Plan> {
        let mut catalog = self.catalog.clone();
        catalog.search_path = view.search_path.clone();
        catalog.views.remove(id);
        let params = Parameters::new();
        let mut scope = isolated(self, &catalog, &params);
        scope.resolved_relations = Some(view.resolved_relations.iter().cloned().collect());
        let plan = scope.query(&stored_query(view)?, None)?;
        if plan.schema.len() != view.columns.len() {
            return Err(error(
                "invalid.view_definition",
                "Stored view output width changed",
            ));
        }
        Ok(plan)
    }
}

/// Store the full ordered output labels with exact source and pinned preparation.
fn definition(
    catalog: &Catalog,
    schema: String,
    name: String,
    plan: Plan,
    sql: &str,
    report: &Value,
) -> Result<ViewDefinition> {
    let columns = plan
        .schema
        .iter()
        .map(|column| column.name.clone())
        .collect::<Vec<_>>();
    unique_names(&columns)?;
    Ok(ViewDefinition {
        schema,
        name,
        columns,
        search_path: catalog.search_path.clone(),
        resolved_relations: vec![],
        source_sql: sql.into(),
        prepared: Some(report.clone()),
    })
}

/// Definition binding shares fuel but cannot capture caller CTEs, parameters or outer rows.
fn isolated<'a>(parent: &Binder<'_>, catalog: &'a Catalog, params: &'a Parameters) -> Binder<'a> {
    let mut scope = binder(catalog, params, true);
    scope.work = parent.work.clone();
    scope.initial_relations = parent.initial_relations.clone();
    scope.locking_seen = parent.locking_seen.clone();
    scope
}

/// Reject every unimplemented CREATE VIEW policy before consuming its query.
fn view_flags(node: &Value) -> Result<()> {
    fields(
        node,
        &[
            "tag",
            "name",
            "columns",
            "query",
            "recursive",
            "replace",
            "temporary",
            "options",
            "check_option",
        ],
    )?;
    if boolean(node, "recursive")?
        || boolean(node, "replace")?
        || boolean(node, "temporary")?
        || !array(node, "options")?.is_empty()
        || !node["check_option"].is_null()
    {
        return Err(error(
            "unsupported.ddl_view_options",
            "Recursive/replacement/temporary/security/check-option views require additional semantics",
        ));
    }
    Ok(())
}

/// Revalidated stored reports may contain exact CREATE VIEW SQL or a standalone SELECT.
fn stored_query(view: &ViewDefinition) -> Result<Value> {
    let report = view.prepared.as_ref().ok_or_else(|| {
        error(
            "unsupported.unprepared_view",
            "Stored source must be reparsed by the pinned parser",
        )
    })?;
    if text(report, "protocol")? == "sqlolla.parser.v1" {
        return legacy_ast(report).cloned();
    }
    normalize::validate_report(report)?;
    let node = &report["result"]["ast"];
    if text(node, "tag")? != "create_view" {
        return normalize::query(node);
    }
    view_flags(node)?;
    let (schema, name) = qualified_name(&node["name"])?;
    if name != view.name || schema.is_some_and(|schema| schema != view.schema) {
        return Err(error(
            "invalid.view_definition",
            "Source CREATE VIEW identity differs",
        ));
    }
    normalize::query(&node["query"])
}

/// Record direct named dependencies, leaving nested view dependencies attached to that view.
fn dependencies(scope: &Binder<'_>) -> Result<Vec<(String, String)>> {
    let mut names = BTreeSet::new();
    for id in scope.table_uses.borrow().iter() {
        let table = scope
            .catalog
            .tables
            .get(id)
            .ok_or_else(|| error("invalid.view_dependency", "Unknown table trace"))?;
        names.insert((table.schema.clone(), table.name.clone()));
    }
    for id in scope.view_uses.borrow().iter() {
        let view = scope
            .catalog
            .views
            .get(id)
            .ok_or_else(|| error("invalid.view_dependency", "Unknown view trace"))?;
        names.insert((view.schema.clone(), view.name.clone()));
    }
    Ok(names.into_iter().collect())
}

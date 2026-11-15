//! FROM binding, catalog resolution, lexical aliases and join null extension.
use super::*;
use crate::json::optional;
use std::collections::BTreeSet;

/// Reject catalog ambiguity before interpreting any external database rows.
pub(super) fn validate_catalog(catalog: &Catalog) -> Result<()> {
    super::ddl::validate_catalog_extensions(catalog)?;
    validate_identifiers(catalog)?;
    let mut tables = BTreeSet::new();
    for table in catalog.tables.values() {
        if !tables.insert((&table.schema, &table.name)) {
            return Err(error("invalid.catalog", "Duplicate table name"));
        }
        let mut columns = BTreeSet::new();
        if table.columns.is_empty() {
            return Err(error(
                "unsupported.empty_table_schema",
                "Zero-column tables are not admitted",
            ));
        }
        for column in &table.columns {
            if matches!(&column.ty,SqlType::Array(element) if matches!(element.as_ref(),SqlType::Array(_)))
            {
                return Err(error(
                    "invalid.array_type",
                    "SQL array dimensionality belongs to values, not nested nominal types",
                ));
            }
            if !columns.insert(&column.name) {
                return Err(error("invalid.catalog", "Duplicate table column"));
            }
        }
    }
    Ok(())
}

/// External stable IDs cannot impersonate compiler-owned statement result relations.
fn validate_identifiers(catalog: &Catalog) -> Result<()> {
    crate::catalog_lookup::validate_catalog(catalog)?;
    crate::sql_function::validate_catalog(catalog)?;
    if catalog
        .tables
        .keys()
        .chain(catalog.views.keys())
        .any(|key| key.starts_with("\0sqlolla.cte:"))
    {
        return Err(error(
            "invalid.catalog_id",
            "Compiler-reserved CTE identifier",
        ));
    }
    Ok(())
}

/// Rename a relation and optionally its prefix of columns, hiding earlier qualifiers.
pub(super) fn rename(
    mut plan: Plan,
    alias: &str,
    schema: Option<String>,
    names: &[Value],
) -> Result<Plan> {
    if names.len() > plan.schema.len() {
        return Err(error("invalid.alias_columns", "Too many column aliases"));
    }
    for (index, column) in plan.schema.iter_mut().enumerate() {
        if let Some(name) = names.get(index) {
            column.name = name
                .as_str()
                .ok_or_else(|| error("invalid.ast", "Nonstring column alias"))?
                .into();
        }
        column.qualifier = Some(alias.into());
        column.schema = schema.clone();
    }
    Ok(plan)
}

impl Binder<'_> {
    /// Resolve independent FROM nodes; missing FROM is a single empty tuple.
    pub(super) fn source(&self, node: &Value) -> Result<Plan> {
        self.charge()?;
        if node.is_null() {
            return Ok(Plan {
                schema: vec![],
                node: RelNode::Values { rows: vec![vec![]] },
            });
        }
        let plan = match text(node, "tag")? {
            "table" => self.table(node),
            "product" => self.product(node),
            "join" => self.join(node),
            "derived" => self.derived(node),
            "extended_table" => self.table(node).map(unordered),
            "extended_derived" => self.derived(node).map(unordered),
            "locked_derived" => self.locked_derived(node).map(unordered),
            tag => Err(error("unsupported.source", tag)),
        }?;
        super::budget::check(&plan)?;
        Ok(plan)
    }

    /// Resolve a CTE before catalog search; explicit schemas bypass CTE lookup.
    fn table(&self, node: &Value) -> Result<Plan> {
        fields(node, &["tag", "name", "alias", "columns", "schema"])?;
        let name = text(node, "name")?;
        let alias = optional(node, "alias")?;
        let schema = optional(node, "schema")?;
        let (plan, actual_schema) = if schema.is_none() && self.ctes.contains_key(name) {
            (self.ctes[name].clone(), None)
        } else {
            self.catalog_table(name, schema.as_deref())?
        };
        let visible_schema = if alias.is_some() { None } else { actual_schema };
        rename(
            plan,
            alias.as_deref().unwrap_or(name),
            visible_schema,
            array(node, "columns")?,
        )
    }

    /// Search schemas in declared order and preserve a stable scan identifier.
    pub(super) fn catalog_table(
        &self,
        name: &str,
        schema: Option<&str>,
    ) -> Result<(Plan, Option<String>)> {
        let schemas: Vec<&str> = schema.map(|name| vec![name]).unwrap_or_else(|| {
            self.catalog
                .search_path
                .iter()
                .map(String::as_str)
                .collect()
        });
        for schema_name in schemas {
            if self
                .resolved_relations
                .as_ref()
                .is_some_and(|names| !names.contains(&(schema_name.to_owned(), name.to_owned())))
            {
                continue;
            }
            if let Some(plan) = self.physical_table(name, schema_name) {
                return Ok((plan, Some(schema_name.to_owned())));
            }
            if let Some(plan) = self.catalog_view(name, schema_name)? {
                return Ok((plan, Some(schema_name.to_owned())));
            }
        }
        Err(error("invalid.table", format!("Unknown table {name}")))
    }

    /// Record the resolved physical identity before exposing its typed scan schema.
    fn physical_table(&self, name: &str, schema: &str) -> Option<Plan> {
        for (id, table) in &self.catalog.tables {
            if table.name == name && table.schema == schema {
                self.table_uses.borrow_mut().insert(id.clone());
                return Some(Plan {
                    schema: table.columns.clone(),
                    node: RelNode::Scan { table: id.clone() },
                });
            }
        }
        None
    }

    /// Bind a noncorrelated subquery and expose only its declared alias.
    fn derived(&self, node: &Value) -> Result<Plan> {
        fields(node, &["tag", "query", "alias", "columns"])?;
        let alias = optional(node, "alias")?.ok_or_else(|| {
            error(
                "unsupported.derived_alias",
                "Derived relation requires an alias",
            )
        })?;
        rename(
            self.query(&node["query"], None)?,
            &alias,
            None,
            array(node, "columns")?,
        )
    }

    /// A nested locking relation retains its entire bound acquisition contract under its alias.
    fn locked_derived(&self, node: &Value) -> Result<Plan> {
        fields(node, &["tag", "query", "alias", "columns"])?;
        let alias = optional(node, "alias")?.ok_or_else(|| {
            error(
                "unsupported.derived_alias",
                "Derived relation requires an alias",
            )
        })?;
        let locked = self.locked_query(&node["query"])?;
        let plan = Plan {
            schema: locked.schema.clone(),
            node: RelNode::Locked(Box::new(locked)),
        };
        rename(plan, &alias, None, array(node, "columns")?)
    }

    /// Lower comma and CROSS JOIN products without suppressing duplicates.
    fn product(&self, node: &Value) -> Result<Plan> {
        fields(node, &["tag", "left", "right"])?;
        let (left, right) = (self.source(&node["left"])?, self.source(&node["right"])?);
        let schema = joined_schema(&left, &right)?;
        Ok(Plan {
            schema,
            node: RelNode::Product {
                left: Box::new(left),
                right: Box::new(right),
            },
        })
    }

    /// Bind ON before extending nullable output types for an outer join.
    fn join(&self, node: &Value) -> Result<Plan> {
        fields(node, &["tag", "kind", "left", "right", "on"])?;
        let (left, right) = (self.source(&node["left"])?, self.source(&node["right"])?);
        let mut schema = joined_schema(&left, &right)?;
        let on = self
            .expressions(&schema)
            .bind(&node["on"], Some(SqlType::Bool))?;
        let kind = join_kind(text(node, "kind")?)?;
        extend_nullability(&mut schema, left.schema.len(), kind);
        Ok(Plan {
            schema,
            node: RelNode::Join {
                kind,
                left: Box::new(left),
                right: Box::new(right),
                on,
            },
        })
    }
}

/// FROM references expose relational bags even when their query selects an ordered page.
fn unordered(input: Plan) -> Plan {
    Plan {
        schema: input.schema.clone(),
        node: RelNode::Unordered {
            input: Box::new(input),
        },
    }
}

/// Merge scopes while rejecting reused visible table aliases.
fn joined_schema(left: &Plan, right: &Plan) -> Result<Vec<Column>> {
    for a in &left.schema {
        for b in &right.schema {
            if a.qualifier.is_some()
                && a.qualifier == b.qualifier
                && (a.schema.is_none() || b.schema.is_none() || a.schema == b.schema)
            {
                return Err(error(
                    "invalid.duplicate_alias",
                    "Duplicate visible table alias",
                ));
            }
        }
    }
    Ok(left.schema.iter().chain(&right.schema).cloned().collect())
}

/// Decode only the ON-join kinds whose semantics are implemented.
fn join_kind(kind: &str) -> Result<JoinKind> {
    match kind {
        "inner" => Ok(JoinKind::Inner),
        "left" => Ok(JoinKind::Left),
        "right" => Ok(JoinKind::Right),
        "full" => Ok(JoinKind::Full),
        _ => Err(error("unsupported.join", kind)),
    }
}

/// Null extension changes output annotations but not child scan requirements.
fn extend_nullability(schema: &mut [Column], split: usize, kind: JoinKind) {
    for (index, column) in schema.iter_mut().enumerate() {
        if (index < split && matches!(kind, JoinKind::Right | JoinKind::Full))
            || (index >= split && matches!(kind, JoinKind::Left | JoinKind::Full))
        {
            column.nullable = true;
        }
    }
}

//! Bind explicit single-target effects without treating writes as relational reads.
use super::*;
use crate::json::optional;
use serde_json::json;
use std::collections::BTreeSet;

/// INSERT fields cannot omit conflict or RETURNING behavior during admission.
const INSERT_FIELDS: &[&str] = &[
    "tag",
    "table",
    "alias",
    "columns",
    "source",
    "on_conflict",
    "returning",
];

impl Binder<'_> {
    /// Preserve effect identity and refuse unimplemented statement families.
    pub(super) fn mutation(&self, node: &Value) -> Result<StatementPlan> {
        match text(node, "tag")? {
            "insert" => self.insert(node).map(StatementPlan::Insert),
            "update" => self.update(node).map(StatementPlan::Update),
            "delete" => self.delete(node).map(StatementPlan::Delete),
            tag => Err(error("unsupported.statement", tag)),
        }
    }

    /// Resolve a physical target and its visible alias before binding expressions.
    fn target(&self, node: &Value) -> Result<MutationTarget> {
        let name = text(node, "table")?;
        let (plan, schema) = self.catalog_table(name, None)?;
        let alias = optional(node, "alias")?;
        let visible = if alias.is_some() { None } else { schema };
        let plan = source::rename(plan, alias.as_deref().unwrap_or(name), visible, &[])?;
        let RelNode::Scan { table } = plan.node else {
            return Err(error("invalid.target", "Expected table scan"));
        };
        Ok(MutationTarget {
            table,
            schema: plan.schema,
        })
    }

    /// Bind RETURNING against the statement-specific old/new target row.
    fn returning(&self, node: &Value, schema: &[Column]) -> Result<Returning> {
        let items = array(node, "returning")?;
        if items.is_empty() {
            return Ok(Returning {
                system_columns: false,
                schema: vec![],
                expressions: vec![],
            });
        }
        let (schema, expressions) = self.projections(&json!({"items":items}), schema, None)?;
        Ok(Returning {
            system_columns: false,
            schema,
            expressions,
        })
    }

    /// Missing WHERE is TRUE; UNKNOWN remains an unselected occurrence.
    fn predicate(&self, node: &Value, schema: &[Column]) -> Result<BoundExpr> {
        if node["where"].is_null() {
            return Ok(crate::expression::literal(Cell::Bool(true)));
        }
        self.expressions(schema)
            .bind(&node["where"], Some(SqlType::Bool))
    }

    /// INSERT binds explicit source, per-position defaults and conflict actions separately.
    fn insert(&self, node: &Value) -> Result<InsertPlan> {
        fields(node, INSERT_FIELDS)?;
        let target = self.target(node)?;
        let conflict = self.conflict(&node["on_conflict"], &target)?;
        let columns = insert_columns(array(node, "columns")?, &target.schema)?;
        let types = columns
            .iter()
            .map(|i| target.schema[*i].ty.clone())
            .collect::<Vec<_>>();
        let (input, default_columns) = super::defaults::source(&node["source"], &columns)?;
        let source = self.insert_source(&input, &types)?;
        if source.schema.len() != columns.len() {
            return Err(error("sql.42601", "INSERT width mismatch"));
        }
        for (column, ty) in source.schema.iter().zip(types) {
            if column.ty != ty {
                return Err(error("sql.42804", "INSERT type mismatch"));
            }
        }
        let returning = self.insert_returning(node, &target.schema)?;
        Ok(InsertPlan {
            default_columns,
            conflict,
            target,
            source,
            columns,
            returning,
        })
    }

    /// Dispatch the insertion source without losing target-column contextual types.
    fn insert_source(&self, input: &Value, types: &[SqlType]) -> Result<Plan> {
        match text(input, "tag")? {
            "values" => self.insert_values(input, types),
            "query_source" => {
                fields(input, &["tag", "query"])?;
                self.query(&normalize::query(&input["query"])?, Some(types))
            }
            tag => Err(error("unsupported.insert_source", tag)),
        }
    }

    /// INSERT VALUES resolves unknown literals against its target columns directly.
    fn insert_values(&self, node: &Value, types: &[SqlType]) -> Result<Plan> {
        fields(node, &["tag", "rows"])?;
        let mut rows = Vec::new();
        for row in array(node, "rows")? {
            if row.as_array().is_none_or(|row| row.len() != types.len()) {
                return Err(error("sql.42601", "INSERT VALUES width mismatch"));
            }
            rows.push(
                row.as_array()
                    .unwrap()
                    .iter()
                    .zip(types)
                    .map(|(node, ty)| self.expressions(&[]).assignment(node, ty))
                    .collect::<Result<_>>()?,
            );
        }
        if rows.is_empty() {
            return Err(error("sql.42601", "INSERT VALUES requires a row"));
        }
        let schema = super::values::value_schema(types);
        Ok(Plan {
            schema,
            node: RelNode::EvaluateValues { rows },
        })
    }

    /// Every assignment is bound against the same original row scope.
    fn update(&self, node: &Value) -> Result<UpdatePlan> {
        mutation_fields(
            node,
            &["tag", "table", "alias", "assignments", "where", "returning"],
            "from",
        )?;
        let target = self.target(node)?;
        let (source, scope) = self.mutation_source(&node["from"], &target.schema)?;
        let assignments =
            self.scoped_assignments(array(node, "assignments")?, &target.schema, &scope)?;
        let predicate = self.predicate(node, &scope)?;
        let returning = self.returning(node, &scope)?;
        Ok(UpdatePlan {
            source,
            target,
            predicate,
            assignments,
            returning,
        })
    }

    /// Repeated targets are rejected and every expression sees the original row scope.
    pub(super) fn assignments(
        &self,
        items: &[Value],
        schema: &[Column],
    ) -> Result<Vec<Assignment>> {
        self.scoped_assignments(items, schema, schema)
    }

    /// Assignment targets resolve only in the physical target, while expressions see FROM.
    fn scoped_assignments(
        &self,
        items: &[Value],
        schema: &[Column],
        scope: &[Column],
    ) -> Result<Vec<Assignment>> {
        let mut seen = BTreeSet::new();
        let mut assignments = Vec::new();
        for item in items {
            fields(item, &["column", "value"])?;
            let column = column_index(text(item, "column")?, schema)?;
            if !seen.insert(column) {
                return Err(error("sql.42601", "Repeated UPDATE target"));
            }
            let expression = self
                .expressions(scope)
                .assignment(&item["value"], &schema[column].ty)?;
            assignments.push(Assignment { column, expression });
        }
        if assignments.is_empty() {
            return Err(error("sql.42601", "Empty UPDATE assignments"));
        }
        Ok(assignments)
    }

    /// DELETE predicates and returned expressions refer to the original target row.
    fn delete(&self, node: &Value) -> Result<DeletePlan> {
        mutation_fields(
            node,
            &["tag", "table", "alias", "where", "returning"],
            "using",
        )?;
        let target = self.target(node)?;
        let (source, scope) = self.mutation_source(&node["using"], &target.schema)?;
        let predicate = self.predicate(node, &scope)?;
        let returning = self.returning(node, &scope)?;
        Ok(DeletePlan {
            source,
            target,
            predicate,
            returning,
        })
    }

    /// Optional CST FROM/USING fields retain aliases, query normalization and source typing.
    fn mutation_source(
        &self,
        node: &Value,
        target: &[Column],
    ) -> Result<(Option<Plan>, Vec<Column>)> {
        if node.is_null() {
            return Ok((None, target.to_vec()));
        }
        let plan = self.source(&normalize::source(node)?)?;
        mutation_aliases(target, &plan.schema)?;
        let mut scope = target.to_vec();
        scope.extend(plan.schema.clone());
        Ok((Some(plan), scope))
    }
}

/// A FROM/USING alias cannot silently introduce a second binding for the mutation target name.
fn mutation_aliases(target: &[Column], source: &[Column]) -> Result<()> {
    let names = target
        .iter()
        .filter_map(|column| column.qualifier.as_deref())
        .collect::<BTreeSet<_>>();
    if source
        .iter()
        .filter_map(|column| column.qualifier.as_deref())
        .any(|name| names.contains(name))
    {
        return Err(error(
            "sql.42712",
            "Mutation source repeats the target alias",
        ));
    }
    Ok(())
}

/// Advanced and CST exports differ only in whether a null source field is explicitly present.
fn mutation_fields(node: &Value, required: &[&str], optional: &str) -> Result<()> {
    let mut allowed = required.to_vec();
    if node.get(optional).is_some() {
        allowed.push(optional);
    }
    fields(node, &allowed)
}

/// Resolve exact physical target columns without allowing duplicate assignments.
pub(super) fn column_index(name: &str, schema: &[Column]) -> Result<usize> {
    schema
        .iter()
        .position(|column| column.name == name)
        .ok_or_else(|| error("sql.42703", "Unknown target column"))
}

/// An omitted INSERT column list means all physical columns in declaration order.
fn insert_columns(names: &[Value], schema: &[Column]) -> Result<Vec<usize>> {
    if names.is_empty() {
        return Ok((0..schema.len()).collect());
    }
    let mut columns = Vec::new();
    for name in names {
        let name = name
            .as_str()
            .ok_or_else(|| error("invalid.ast", "Column name must be text"))?;
        let index = column_index(name, schema)?;
        if columns.contains(&index) {
            return Err(error("sql.42701", "Repeated INSERT target"));
        }
        columns.push(index);
    }
    Ok(columns)
}

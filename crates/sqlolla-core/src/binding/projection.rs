//! SELECT binding and positional wildcard expansion.
use super::*;
use crate::json::optional;

/// Wildcards are positions, avoiding ambiguous-name rebinding after expansion.
enum Item<'a> {
    Position(usize),
    Expression(&'a Value, Option<String>),
}

impl Binder<'_> {
    /// Infer projection output types before contextual set-operand resolution.
    pub(super) fn select_hints(&self, node: &Value) -> Result<Vec<Option<SqlType>>> {
        let input = self.source(&node["source"])?;
        let (node, input) = self.expanded_sets(node, input)?;
        let (node, input, _) = self.expanded_windows(&node, input, &[])?;
        let expressions = self.expressions(&input.schema);
        items(&node, &input.schema)?
            .iter()
            .map(|item| match item {
                Item::Position(index) => Ok(Some(input.schema[*index].ty.clone())),
                Item::Expression(value, _) => expressions.hint(value),
            })
            .collect()
    }

    /// Apply WHERE before projection, and DISTINCT only after projection.
    pub(super) fn select(&self, node: &Value, expected: Option<&[SqlType]>) -> Result<Plan> {
        fields(node, &["tag", "items", "source", "where", "distinct"])?;
        let input = self.filtered_source(node)?;
        let (node, input) = self.expanded_sets(node, input)?;
        let (node, input, _) = self.expanded_windows(&node, input, &[])?;
        let (schema, expressions) = self.projections(&node, &input.schema, expected)?;
        let mut plan = Plan {
            schema,
            node: RelNode::Project {
                input: Box::new(input),
                expressions,
            },
        };
        if boolean(&node, "distinct")? {
            plan = Plan {
                schema: plan.schema.clone(),
                node: RelNode::Distinct {
                    input: Box::new(plan),
                },
            };
        }
        Ok(plan)
    }

    /// Bind WHERE against source columns before SELECT aliases become visible.
    pub(super) fn filtered_source(&self, node: &Value) -> Result<Plan> {
        let input = self.source(&node["source"])?;
        if node["where"].is_null() {
            return Ok(input);
        }
        let predicate = self
            .expressions(&input.schema)
            .bind(&node["where"], Some(SqlType::Bool))?;
        Ok(Plan {
            schema: input.schema.clone(),
            node: RelNode::Filter {
                input: Box::new(input),
                predicate,
            },
        })
    }

    /// Bind all projection positions using their individual contextual types.
    pub(super) fn projections(
        &self,
        node: &Value,
        input: &[Column],
        expected: Option<&[SqlType]>,
    ) -> Result<(Vec<Column>, Vec<BoundExpr>)> {
        let items = items(node, input)?;
        if expected.is_some_and(|types| types.len() != items.len()) {
            return Err(error("invalid.projection_arity", "Context width mismatch"));
        }
        let mut columns = Vec::new();
        let mut expressions = Vec::new();
        for (index, item) in items.into_iter().enumerate() {
            let ty = expected.map(|types| types[index].clone());
            let (name, expr) = self.project_item(item, input, ty)?;
            columns.push(Column {
                name,
                ty: expr.ty.clone(),
                nullable: expr.nullable,
                qualifier: None,
                schema: None,
            });
            expressions.push(expr);
        }
        Ok((columns, expressions))
    }

    /// Resolve a wildcard position directly or bind a selected scalar expression.
    fn project_item(
        &self,
        item: Item<'_>,
        input: &[Column],
        expected: Option<SqlType>,
    ) -> Result<(String, BoundExpr)> {
        match item {
            Item::Expression(value, alias) => {
                let bound = self.expressions(input).bind(value, expected)?;
                let name = alias.unwrap_or_else(|| crate::expression::output_label(value, &bound));
                Ok((name, bound))
            }
            Item::Position(index) => {
                let column = &input[index];
                if expected.is_some_and(|ty| ty != column.ty) {
                    return Err(error("invalid.type", "Wildcard type mismatch"));
                }
                Ok((
                    column.name.clone(),
                    BoundExpr {
                        ty: column.ty.clone(),
                        nullable: column.nullable,
                        node: ExprNode::Column(index),
                    },
                ))
            }
        }
    }
}

/// Expand projection items while retaining column order and duplicate names.
fn items<'a>(node: &'a Value, schema: &[Column]) -> Result<Vec<Item<'a>>> {
    let schema = super::set_returning::visible(schema);
    let mut result = Vec::new();
    for item in array(node, "items")? {
        match text(item, "tag")? {
            "wildcard" => result.extend(wildcard(item, schema)?.into_iter().map(Item::Position)),
            "expression" => {
                fields(item, &["tag", "value", "alias"])?;
                let name = optional(item, "alias")?;
                result.push(Item::Expression(&item["value"], name));
            }
            tag => return Err(error("unsupported.select_item", tag)),
        }
    }
    if result.is_empty() {
        return Err(error(
            "unsupported.empty_projection",
            "Projection cannot be empty",
        ));
    }
    Ok(result)
}

/// A qualified wildcard must refer to a visible source, never a hidden alias.
fn wildcard(node: &Value, schema: &[Column]) -> Result<Vec<usize>> {
    fields(node, &["tag", "qualifier", "schema"])?;
    let (qualifier, namespace) = (optional(node, "qualifier")?, optional(node, "schema")?);
    crate::expression::qualifier_scope(&qualifier, &namespace, schema)?;
    let positions: Vec<_> = schema
        .iter()
        .enumerate()
        .filter(|(_, column)| {
            qualifier
                .as_ref()
                .is_none_or(|q| column.qualifier.as_ref() == Some(q))
                && namespace
                    .as_ref()
                    .is_none_or(|s| column.schema.as_ref() == Some(s))
        })
        .map(|(index, _)| index)
        .collect();
    if positions.is_empty() {
        return Err(error("invalid.wildcard", "Wildcard source is not visible"));
    }
    Ok(positions)
}

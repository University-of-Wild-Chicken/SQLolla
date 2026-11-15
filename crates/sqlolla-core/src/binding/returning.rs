//! INSERT RETURNING binds system fields explicitly without adding them to wildcard expansion.
use super::*;
use serde_json::json;

impl Binder<'_> {
    /// Non-wildcard expressions can name xmin/xmax; the physical user schema remains unchanged.
    pub(super) fn insert_returning(&self, node: &Value, input: &[Column]) -> Result<Returning> {
        let extended = system_scope(input)?;
        let mut result = Returning {
            system_columns: false,
            schema: vec![],
            expressions: vec![],
        };
        for item in array(node, "returning")? {
            let scope = if item["tag"] == "wildcard" {
                input
            } else {
                &extended
            };
            let (schema, expressions) = self.projections(&json!({"items":[item]}), scope, None)?;
            result.schema.extend(schema);
            result.expressions.extend(expressions);
        }
        result.system_columns = uses_header(&mut result.expressions, input.len())?;
        Ok(result)
    }
}

/// System-field qualification follows the mutation target's visible table alias and namespace.
fn system_scope(input: &[Column]) -> Result<Vec<Column>> {
    if input
        .iter()
        .any(|column| matches!(column.name.as_str(), "xmin" | "xmax"))
    {
        return Err(error(
            "invalid.catalog_system_column",
            "Physical columns cannot shadow xmin/xmax",
        ));
    }
    let first = input
        .first()
        .ok_or_else(|| error("unsupported.empty_table", "Missing target schema"))?;
    let mut columns = input.to_vec();
    for name in ["xmin", "xmax"] {
        columns.push(Column {
            name: name.into(),
            ty: SqlType::Xid,
            nullable: false,
            qualifier: first.qualifier.clone(),
            schema: first.schema.clone(),
        });
    }
    Ok(columns)
}

/// Correlated system-column subqueries need separate version lineage and are not silently rebound.
fn uses_header(expressions: &mut [BoundExpr], width: usize) -> Result<bool> {
    let mut system = false;
    let mut nested = false;
    for expression in expressions {
        inspect(expression, width, &mut system, &mut nested)?;
    }
    if system && nested {
        return Err(error(
            "unsupported.returning_system_subquery",
            "Nested system-column scope requires version lineage",
        ));
    }
    Ok(system)
}

/// Inspect every scalar root while keeping relational column positions in their own scope.
fn inspect(expr: &mut BoundExpr, width: usize, system: &mut bool, nested: &mut bool) -> Result<()> {
    match &mut expr.node {
        ExprNode::Column(index) | ExprNode::OuterColumn { index, .. } => *system |= *index >= width,
        ExprNode::Subquery { query, left, .. } => {
            *nested = true;
            if let Some(left) = left {
                inspect(left, width, system, nested)?;
            }
            crate::rewrite::relation(query, &mut |expr| nested_header(expr, width, 0, system))?;
            return Ok(());
        }
        _ => {}
    }
    crate::rewrite::scalar_children(expr, &mut |expr| inspect(expr, width, system, nested))
}

/// Only correlations to this RETURNING scope identify its hidden fields inside nested queries.
fn nested_header(
    expr: &mut BoundExpr,
    width: usize,
    nesting: usize,
    system: &mut bool,
) -> Result<()> {
    match &mut expr.node {
        ExprNode::OuterColumn { depth, index } if *depth == nesting => *system |= *index >= width,
        ExprNode::Subquery { query, left, .. } => {
            if let Some(left) = left {
                nested_header(left, width, nesting, system)?;
            }
            crate::rewrite::relation(query, &mut |expr| {
                nested_header(expr, width, nesting + 1, system)
            })?;
            return Ok(());
        }
        _ => {}
    }
    crate::rewrite::scalar_children(expr, &mut |expr| {
        nested_header(expr, width, nesting, system)
    })
}

//! Singleton syntactic VALUES lineage permits constant propagation; database rows never do.
use crate::*;

/// Apply the singleton-source rule only to relational statement inputs.
pub(super) fn statement(plan: &mut StatementPlan) -> Result<()> {
    match plan {
        StatementPlan::Initialized { body, .. } => statement(body),
        StatementPlan::With(plan) => {
            for binding in &mut plan.bindings {
                statement(&mut binding.statement)?;
            }
            statement(&mut plan.body)
        }
        StatementPlan::Query(plan) => relation(plan),
        StatementPlan::LockedQuery(plan) => relation(&mut plan.source),
        StatementPlan::Insert(plan) => relation(&mut plan.source),
        _ => Ok(()),
    }
}

/// Child literals become available before parent conditional expressions are simplified.
pub(super) fn relation(plan: &mut Plan) -> Result<()> {
    children(&mut plan.node)?;
    let row = match &plan.node {
        RelNode::Window { input, .. } => window_lineage(input),
        RelNode::Filter { input, .. }
        | RelNode::Project { input, .. }
        | RelNode::Order { input, .. }
        | RelNode::Aggregate { input, .. } => known_row(input),
        _ => None,
    };
    crate::rewrite::relation_expressions(plan, &mut |expr| {
        if let Some(row) = &row {
            replace_columns(expr, row)?;
        }
        super::expression(expr)
    })?;
    literal_values(plan);
    Ok(())
}

/// Restore finite VALUES after planning only when every cell is already a literal.
fn literal_values(plan: &mut Plan) {
    let RelNode::EvaluateValues { rows } = &plan.node else {
        return;
    };
    if let Some(rows) = rows.iter().map(|row| literal_row(row)).collect() {
        plan.node = RelNode::Values { rows };
    }
}

/// Window key planning retains singleton value facts even when a filter removes its occurrence.
fn window_lineage(plan: &Plan) -> Option<Row> {
    match &plan.node {
        RelNode::Filter { input, .. } => window_lineage(input),
        _ => known_row(plan),
    }
}

/// Descend bound relation inputs without ever reading the external Database argument.
fn children(node: &mut RelNode) -> Result<()> {
    match node {
        RelNode::Locked(plan) => relation(&mut plan.source),
        RelNode::Filter { input, .. }
        | RelNode::Project { input, .. }
        | RelNode::Order { input, .. }
        | RelNode::Slice { input, .. }
        | RelNode::Distinct { input }
        | RelNode::DistinctOn { input, .. }
        | RelNode::ExpandSets { input, .. }
        | RelNode::Window { input, .. }
        | RelNode::Unordered { input }
        | RelNode::Aggregate { input, .. } => relation(input),
        RelNode::Product { left, right }
        | RelNode::Set { left, right, .. }
        | RelNode::Join { left, right, .. } => {
            relation(left)?;
            relation(right)
        }
        RelNode::Values { .. } | RelNode::EvaluateValues { .. } | RelNode::Scan { .. } => Ok(()),
    }
}

/// Only a single syntactic occurrence and cardinality-preserving wrappers provide evidence.
fn known_row(plan: &Plan) -> Option<Row> {
    match &plan.node {
        RelNode::Values { rows } => match rows.as_slice() {
            [row] => Some(row.clone()),
            _ => None,
        },
        RelNode::EvaluateValues { rows } => match rows.as_slice() {
            [row] => literal_row(row),
            _ => None,
        },
        RelNode::Project { input, expressions } => {
            known_row(input).and_then(|_| literal_row(expressions))
        }
        RelNode::Unordered { input }
        | RelNode::Order { input, .. }
        | RelNode::Distinct { input } => known_row(input),
        RelNode::Filter { input, predicate }
            if predicate.node == ExprNode::Literal(Cell::Bool(true)) =>
        {
            known_row(input)
        }
        RelNode::Slice {
            input,
            offset: 0,
            limit,
        } if *limit != Some(0) => known_row(input),
        _ => None,
    }
}

/// A partially constant row supplies no evidence for this deliberately narrow initial rule.
fn literal_row(values: &[BoundExpr]) -> Option<Row> {
    values
        .iter()
        .map(|expr| match &expr.node {
            ExprNode::Literal(value) => Some(value.clone()),
            _ => None,
        })
        .collect()
}

/// Replace local columns, and the corresponding enclosing scope inside correlated queries.
fn replace_columns(expr: &mut BoundExpr, row: &Row) -> Result<()> {
    if let ExprNode::Column(index) = expr.node {
        let value = row.get(index).ok_or_else(|| {
            crate::json::error("invalid.column_index", "Singleton column outside row")
        })?;
        *expr = crate::expression::literal(value.clone());
        return Ok(());
    }
    if let ExprNode::Subquery { query, .. } = &mut expr.node {
        crate::rewrite::relation(query, &mut |expr| replace_outer(expr, row, 0))?;
    }
    crate::rewrite::scalar_children(expr, &mut |arg| replace_columns(arg, row))
}

/// Nested queries increase lexical depth without replacing their nearer outer row scopes.
fn replace_outer(expr: &mut BoundExpr, row: &Row, depth: usize) -> Result<()> {
    if let ExprNode::OuterColumn {
        depth: actual,
        index,
    } = expr.node
    {
        if actual == depth {
            let value = row.get(index).ok_or_else(|| {
                crate::json::error("invalid.outer_column", "Singleton outer column outside row")
            })?;
            *expr = crate::expression::literal(value.clone());
        }
        return Ok(());
    }
    if let ExprNode::Subquery { query, .. } = &mut expr.node {
        crate::rewrite::relation(query, &mut |expr| replace_outer(expr, row, depth + 1))?;
    }
    crate::rewrite::scalar_children(expr, &mut |arg| replace_outer(arg, row, depth))
}

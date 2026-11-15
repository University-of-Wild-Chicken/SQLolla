//! Conservative pre-encoding limits on expanded CTE plans and expression trees.
use crate::json::error;
use crate::*;

/// Validate a completed bounded subtree before storing or cloning it as a CTE.
pub(super) fn check(plan: &Plan) -> Result<()> {
    relation(plan, &mut 256)
}

/// Consume one expanded node without allowing unsigned counter wraparound.
fn consume(remaining: &mut usize) -> Result<()> {
    *remaining = remaining
        .checked_sub(1)
        .ok_or_else(|| error("resource.plan_nodes", "Expanded plan exceeds 256 nodes"))?;
    Ok(())
}

/// Visit bounded relation children and the scalar expressions they own.
fn relation(plan: &Plan, remaining: &mut usize) -> Result<()> {
    consume(remaining)?;
    if plan.schema.len() > 64 {
        return Err(error("resource.plan_width", "Plan exceeds 64 columns"));
    }
    relation_expressions(&plan.node, remaining)?;
    match &plan.node {
        RelNode::Values { .. } | RelNode::Scan { .. } => Ok(()),
        RelNode::EvaluateValues { .. } => Ok(()),
        RelNode::Locked(plan) => relation(&plan.source, remaining),
        RelNode::Filter { input, .. }
        | RelNode::Project { input, .. }
        | RelNode::Order { input, .. }
        | RelNode::DistinctOn { input, .. }
        | RelNode::ExpandSets { input, .. }
        | RelNode::Window { input, .. }
        | RelNode::Aggregate { input, .. } => relation(input, remaining),
        RelNode::Distinct { input }
        | RelNode::Slice { input, .. }
        | RelNode::Unordered { input } => relation(input, remaining),
        RelNode::Join { left, right, .. } => pair(left, right, remaining),
        RelNode::Product { left, right } | RelNode::Set { left, right, .. } => {
            pair(left, right, remaining)
        }
    }
}

/// Count scalar trees owned by one relation without revisiting relation descendants.
fn relation_expressions(node: &RelNode, remaining: &mut usize) -> Result<()> {
    match node {
        RelNode::EvaluateValues { rows } => {
            for row in rows {
                expressions(row, remaining)?;
            }
            Ok(())
        }
        RelNode::Filter { predicate, .. } => expression(predicate, remaining),
        RelNode::DistinctOn { keys, .. } => expressions(keys, remaining),
        RelNode::ExpandSets { arrays, .. } => expressions(arrays, remaining),
        RelNode::Window { spec, .. } => window(spec, remaining),
        RelNode::Project {
            expressions: values,
            ..
        } => expressions(values, remaining),
        RelNode::Order { keys, .. } => {
            for key in keys {
                expression(&key.expression, remaining)?;
            }
            Ok(())
        }
        RelNode::Join { on, .. } => expression(on, remaining),
        RelNode::Aggregate {
            groups, aggregates, ..
        } => aggregate(groups, aggregates, remaining),
        _ => Ok(()),
    }
}

/// Window partition/order expressions and shared function slots consume the same expansion budget.
fn window(spec: &WindowSpec, remaining: &mut usize) -> Result<()> {
    expressions(&spec.partition_by, remaining)?;
    for key in &spec.order_by {
        expression(&key.expression, remaining)?;
    }
    for _ in &spec.functions {
        consume(remaining)?;
    }
    Ok(())
}

/// Aggregate arguments and FILTER predicates share the expanded scalar-node budget.
fn aggregate(groups: &[BoundExpr], calls: &[AggregateExpr], remaining: &mut usize) -> Result<()> {
    expressions(groups, remaining)?;
    for call in calls {
        consume(remaining)?;
        if let Some(arg) = &call.argument {
            expression(arg, remaining)?;
        }
        if let Some(filter) = &call.filter {
            expression(filter, remaining)?;
        }
    }
    Ok(())
}

/// Visit each scalar root with the same expanded-node budget.
fn expressions(values: &[BoundExpr], remaining: &mut usize) -> Result<()> {
    for value in values {
        expression(value, remaining)?;
    }
    Ok(())
}

/// Count both branches; already-validated children bound the cost of this traversal.
fn pair(left: &Plan, right: &Plan, remaining: &mut usize) -> Result<()> {
    relation(left, remaining)?;
    relation(right, remaining)
}

/// Count scalar nodes so copying expression-rich CTEs also consumes capacity.
fn expression(expr: &BoundExpr, remaining: &mut usize) -> Result<()> {
    consume(remaining)?;
    match &expr.node {
        ExprNode::Literal(_)
        | ExprNode::Column(_)
        | ExprNode::Context { .. }
        | ExprNode::OuterColumn { .. } => Ok(()),
        ExprNode::Subquery { query, left, .. } => {
            if let Some(left) = left {
                expression(left, remaining)?;
            }
            relation(query, remaining)
        }
        ExprNode::Not(arg)
        | ExprNode::IsNull { arg, .. }
        | ExprNode::Negate(arg)
        | ExprNode::Cast { arg }
        | ExprNode::AssignmentCast { arg }
        | ExprNode::IntervalCast { arg, .. } => expression(arg, remaining),
        _ => composite_expression(expr, remaining),
    }
}

/// Count composite scalar children after their root has consumed its own node budget.
fn composite_expression(expr: &BoundExpr, remaining: &mut usize) -> Result<()> {
    match &expr.node {
        ExprNode::Binary { left, right, .. }
        | ExprNode::Quantified { left, right, .. }
        | ExprNode::Arithmetic { left, right, .. }
        | ExprNode::NullIf { left, right } => {
            expression(left, remaining)?;
            expression(right, remaining)
        }
        ExprNode::RowCompare { left, right, .. } => {
            expressions(left, remaining)?;
            expressions(right, remaining)
        }
        ExprNode::Pattern { arg, pattern, .. } => {
            expression(arg, remaining)?;
            expression(pattern, remaining)
        }
        ExprNode::Call { args, .. }
        | ExprNode::Coalesce(args)
        | ExprNode::Array { values: args, .. } => expressions(args, remaining),
        ExprNode::Case {
            branches,
            otherwise,
        } => case_expression(branches, otherwise, remaining),
        _ => Err(error(
            "invalid.scalar_traversal",
            "Expected composite expression",
        )),
    }
}

/// Count every CASE condition/result branch even if planning can later prune it.
fn case_expression(
    branches: &[(BoundExpr, BoundExpr)],
    otherwise: &BoundExpr,
    remaining: &mut usize,
) -> Result<()> {
    for (condition, value) in branches {
        expression(condition, remaining)?;
        expression(value, remaining)?;
    }
    expression(otherwise, remaining)
}

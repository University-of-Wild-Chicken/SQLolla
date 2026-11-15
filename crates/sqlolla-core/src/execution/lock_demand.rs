//! Conservative admission for eager operators whose PostgreSQL demand can change lock ownership.
use super::*;

/// Detect nested lock contracts without evaluating physical rows or scalar expressions.
pub(super) fn contains(plan: &Plan) -> Result<bool> {
    Ok(crate::cte::dependencies(&StatementPlan::Query(plan.clone()))?.locks)
}

/// Ordered scalar event traces require the same explicit consumer-demand boundaries as locks.
pub(super) fn ordered(plan: &Plan) -> Result<bool> {
    Ok(crate::cte::dependencies(&StatementPlan::Query(plan.clone()))?.ordered_effects)
}

/// Detect either acquisition observations or ordered events in a child dependency cone.
fn any(plan: &Plan) -> Result<bool> {
    Ok(contains(plan)? || ordered(plan)?)
}

/// Refuse consumers whose early exit or optimizer pushdown could change an eager lock footprint.
pub(super) fn admit(plan: &Plan) -> Result<()> {
    let unsafe_demand = match &plan.node {
        RelNode::Slice { input, .. } => any(input)?,
        RelNode::Product { left, right } | RelNode::Join { left, right, .. } => {
            any(left)? || any(right)?
        }
        RelNode::Filter { input, predicate } => {
            ordered(input)? || (contains(input)? && !subquery_filter(predicate))
        }
        RelNode::Set {
            op, left, right, ..
        } if *op != SetOp::Union => any(left)? || any(right)?,
        _ => false,
    };
    if unsafe_demand {
        return Err(error(
            "unsupported.nested_lock_demand",
            "This consumer needs demand-driven or pushed-down lock evaluation",
        ));
    }
    Ok(())
}

/// A single IN subplan predicate stays above the derived LockRows boundary in the admitted profile.
fn subquery_filter(expr: &BoundExpr) -> bool {
    match &expr.node {
        ExprNode::Not(arg) => subquery_filter(arg),
        ExprNode::Subquery {
            kind: SubqueryKind::In,
            left: Some(left),
            ..
        } => {
            matches!(left.node, ExprNode::Column(_))
        }
        _ => false,
    }
}

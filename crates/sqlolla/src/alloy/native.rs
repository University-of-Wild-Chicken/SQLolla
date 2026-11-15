//! Preserve the independently encoded M1 fragment as extended semantics grow.
use sqlolla_core::{BoundExpr, ExprNode, Plan, RelNode, SqlType};

/// Return true only for expressions with native, independent Alloy semantics.
fn expression(expr: &BoundExpr) -> bool {
    if !matches!(expr.ty, SqlType::Int32 | SqlType::Bool | SqlType::Text) {
        return false;
    }
    match &expr.node {
        ExprNode::Literal(_) | ExprNode::Column(_) => true,
        ExprNode::Not(arg) | ExprNode::IsNull { arg, .. } => expression(arg),
        ExprNode::Binary { left, right, op } => {
            use sqlolla_core::BinaryOp::*;
            matches!(op, Eq | Ne | Lt | Le | Gt | Ge | And | Or)
                && expression(left)
                && expression(right)
        }
        #[allow(unreachable_patterns)]
        _ => false,
    }
}

/// Keep new ordering, scalar and effect semantics out of the native M1 encoder.
pub fn supports(plan: &Plan) -> bool {
    if plan
        .schema
        .iter()
        .any(|column| !matches!(column.ty, SqlType::Int32 | SqlType::Bool | SqlType::Text))
    {
        return false;
    }
    match &plan.node {
        RelNode::Values { .. } | RelNode::Scan { .. } => true,
        RelNode::Filter { input, predicate } => supports(input) && expression(predicate),
        RelNode::Project { input, expressions } => {
            supports(input) && expressions.iter().all(expression)
        }
        RelNode::Distinct { input } => supports(input),
        RelNode::Product { left, right } | RelNode::Set { left, right, .. } => {
            supports(left) && supports(right)
        }
        RelNode::Join {
            left, right, on, ..
        } => supports(left) && supports(right) && expression(on),
        _ => false,
    }
}

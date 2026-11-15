//! PostgreSQL-style immutable constant simplification, distinct from runtime branch execution.
use crate::*;
mod propagation;

/// A CASE branch pairs its Boolean selector with its value expression.
type CaseBranches = Vec<(BoundExpr, BoundExpr)>;

/// Simplify a fully type-checked statement; unused CTE definitions are already absent.
pub(crate) fn statement(mut plan: StatementPlan) -> Result<StatementPlan> {
    propagation::statement(&mut plan)?;
    crate::rewrite::statement(&mut plan, &mut expression)?;
    Ok(plan)
}

/// Fold immutable expressions after conditional simplification of CASE/COALESCE/Boolean nodes.
pub(crate) fn expression(expr: &mut BoundExpr) -> Result<()> {
    match &mut expr.node {
        ExprNode::Subquery { query, left, .. } => {
            if let Some(left) = left {
                expression(left)?;
            }
            propagation::relation(query)?;
        }
        ExprNode::Case { .. } => case(expr)?,
        ExprNode::Coalesce(_) => coalesce(expr)?,
        ExprNode::Binary {
            op: BinaryOp::And | BinaryOp::Or,
            ..
        } => boolean(expr)?,
        _ => crate::rewrite::scalar_children(expr, &mut expression)?,
    }
    if strict_null(expr) {
        *expr = crate::expression::literal(Cell::Null(expr.ty.clone()));
    }
    if constant(expr) {
        let value = eval_expr(expr, &vec![]).map_err(planning_failure)?;
        *expr = crate::expression::literal(value);
    }
    Ok(())
}

/// SQLSTATE gains planning provenance only at this observed constant-evaluation boundary.
fn planning_failure(mut error: Error) -> Error {
    if let Some(sql) = &mut error.sql {
        sql.phase = FailurePhase::Planning;
    } else if let Some(code) = error.code.strip_prefix("sql.") {
        error.sql = Some(Box::new(SqlFailure {
            sqlstate: code.into(),
            phase: FailurePhase::Planning,
            constraint_name: None,
            column_name: None,
            exposed_rows: vec![],
            effects: vec![],
        }));
    }
    error
}

/// A constant contains neither row references nor stable/session values.
fn constant(expr: &BoundExpr) -> bool {
    crate::function_registry::immutable_operation(expr) && structural_constant(expr)
}

/// Recursive constant shape remains separate from the current operation's volatility.
fn structural_constant(expr: &BoundExpr) -> bool {
    match &expr.node {
        ExprNode::Literal(_) => true,
        ExprNode::Column(_)
        | ExprNode::Context { .. }
        | ExprNode::OuterColumn { .. }
        | ExprNode::Subquery { .. } => false,
        ExprNode::Not(arg)
        | ExprNode::Negate(arg)
        | ExprNode::Cast { arg }
        | ExprNode::AssignmentCast { arg }
        | ExprNode::IntervalCast { arg, .. }
        | ExprNode::IsNull { arg, .. } => constant(arg),
        ExprNode::Binary { left, right, .. }
        | ExprNode::Quantified { left, right, .. }
        | ExprNode::Arithmetic { left, right, .. }
        | ExprNode::NullIf { left, right } => constant(left) && constant(right),
        ExprNode::RowCompare { left, right, .. } => left.iter().chain(right).all(constant),
        ExprNode::Pattern { arg, pattern, .. } => constant(arg) && constant(pattern),
        ExprNode::Call { name, args } => constant_call(name, args),
        ExprNode::Coalesce(args) | ExprNode::Array { values: args, .. } => {
            args.iter().all(constant)
        }
        ExprNode::Case {
            branches,
            otherwise,
        } => branches.iter().all(|(a, b)| constant(a) && constant(b)) && constant(otherwise),
    }
}

/// Discard statically false CASE results, but fold every still-reachable result expression.
fn case(expr: &mut BoundExpr) -> Result<()> {
    let ExprNode::Case {
        branches,
        otherwise,
    } = &mut expr.node
    else {
        unreachable!()
    };
    let (retained, fallback) = case_branches(std::mem::take(branches))?;
    let mut fallback = fallback.unwrap_or_else(|| (**otherwise).clone());
    expression(&mut fallback)?;
    if retained.is_empty() {
        *expr = fallback;
    } else {
        *branches = retained;
        **otherwise = fallback;
    }
    Ok(())
}

/// Simplify conditions before visiting their result, retaining only reachable branches.
fn case_branches(branches: CaseBranches) -> Result<(CaseBranches, Option<BoundExpr>)> {
    let mut retained = Vec::new();
    for (mut condition, mut value) in branches {
        expression(&mut condition)?;
        match &condition.node {
            ExprNode::Literal(Cell::Bool(false) | Cell::Null(_)) => continue,
            ExprNode::Literal(Cell::Bool(true)) => {
                expression(&mut value)?;
                return Ok((retained, Some(value)));
            }
            _ => {
                expression(&mut value)?;
                retained.push((condition, value));
            }
        }
    }
    Ok((retained, None))
}

/// A known non-NULL value cuts off later COALESCE arguments during planning too.
fn coalesce(expr: &mut BoundExpr) -> Result<()> {
    let ExprNode::Coalesce(args) = &mut expr.node else {
        unreachable!()
    };
    let mut retained = Vec::new();
    for mut arg in std::mem::take(args) {
        expression(&mut arg)?;
        if matches!(arg.node, ExprNode::Literal(Cell::Null(_))) {
            continue;
        }
        let stop = matches!(arg.node, ExprNode::Literal(_));
        retained.push(arg);
        if stop {
            break;
        }
    }
    match retained.len() {
        0 => *expr = crate::expression::literal(Cell::Null(expr.ty.clone())),
        1 => *expr = retained.remove(0),
        _ => *args = retained,
    }
    Ok(())
}

/// Fold Boolean arguments left-to-right, stopping at the operation's decisive constant.
fn boolean(expr: &mut BoundExpr) -> Result<()> {
    let ExprNode::Binary { op, left, right } = &mut expr.node else {
        unreachable!()
    };
    expression(left)?;
    let decisive = matches!(
        (op, &left.node),
        (BinaryOp::And, ExprNode::Literal(Cell::Bool(false)))
            | (BinaryOp::Or, ExprNode::Literal(Cell::Bool(true)))
    );
    if decisive {
        *expr = (**left).clone();
        return Ok(());
    }
    expression(right)
}

/// Registry volatility controls folding independently of recursive argument planning.
fn constant_call(name: &str, args: &[BoundExpr]) -> bool {
    crate::function_registry::immutable_call(name, args) && args.iter().all(constant)
}

/// Only declared strict operations may erase a volatile sibling after child planning succeeds.
fn strict_null(expr: &BoundExpr) -> bool {
    if !crate::function_registry::strict_operation(expr) {
        return false;
    }
    let mut known_null = false;
    let _ = crate::rewrite::scalar_children(&mut expr.clone(), &mut |child| {
        known_null |= matches!(child.node, ExprNode::Literal(Cell::Null(_)));
        Ok(())
    });
    known_null
}

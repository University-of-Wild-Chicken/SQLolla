//! Evaluation of admitted typed scalar nodes; external effects are deliberately separate.
use crate::json::error;
use crate::*;

/// Execute extended scalar operations using checked canonical datatype implementations.
pub(crate) fn evaluate(expr: &BoundExpr, row: &Row) -> Result<Cell> {
    match &expr.node {
        ExprNode::Context { .. } => context_required(),
        ExprNode::Arithmetic { op, left, right } => {
            scalar::arithmetic(op, &eval_expr(left, row)?, &eval_expr(right, row)?)
        }
        ExprNode::Negate(arg) => negate(arg, row),
        ExprNode::Cast { arg } => scalar::cast(&eval_expr(arg, row)?, &expr.ty),
        ExprNode::IntervalCast { arg, qualifier } => {
            scalar::interval_cast(&eval_expr(arg, row)?, qualifier)
        }
        ExprNode::AssignmentCast { arg } => {
            scalar::assignment_cast(&eval_expr(arg, row)?, &expr.ty)
        }
        ExprNode::Call { name, args } => call(name, args, row),
        ExprNode::Array { element, values } => array(element, values, row),
        _ => compound(expr, row),
    }
}

/// Compound scalar policies retain their own short-circuit and NULL contracts.
fn compound(expr: &BoundExpr, row: &Row) -> Result<Cell> {
    match &expr.node {
        ExprNode::RowCompare { op, left, right } => crate::row_eval::compare(*op, left, right, row),
        ExprNode::Pattern {
            arg,
            pattern,
            insensitive,
        } => crate::pattern::evaluate(arg, pattern, *insensitive, row),
        ExprNode::Quantified {
            op,
            all,
            left,
            right,
        } => quantified(*op, *all, left, right, row),
        ExprNode::Case {
            branches,
            otherwise,
        } => case(branches, otherwise, row),
        ExprNode::Coalesce(args) => coalesce(args, &expr.ty, row),
        ExprNode::NullIf { left, right } => nullif(left, right, row),
        _ => Err(error(
            "invalid.scalar",
            "Expected extended scalar expression",
        )),
    }
}

/// Stable context expressions cannot be evaluated without an explicit session snapshot.
fn context_required() -> Result<Cell> {
    Err(error(
        "unsupported.context_value",
        "Explicit execution context required",
    ))
}

/// Empty ANY is FALSE and empty ALL TRUE even for a NULL scalar; NULL arrays are UNKNOWN.
fn quantified(
    op: BinaryOp,
    all: bool,
    left: &BoundExpr,
    right: &BoundExpr,
    row: &Row,
) -> Result<Cell> {
    let left = eval_expr(left, row)?;
    let right = eval_expr(right, row)?;
    let Some(values) = scalar::array_elements(&right)? else {
        return Ok(Cell::Null(SqlType::Bool));
    };
    let mut unknown = false;
    for right in values {
        match compare_values(op, left.clone(), right)? {
            Cell::Bool(value) if value != all => return Ok(Cell::Bool(value)),
            Cell::Null(_) => unknown = true,
            Cell::Bool(_) => (),
            _ => return invalid_quantified_type(),
        }
    }
    Ok(if unknown {
        Cell::Null(SqlType::Bool)
    } else {
        Cell::Bool(all)
    })
}

/// A malformed typed comparison cannot become UNKNOWN and hide an engine inconsistency.
fn invalid_quantified_type() -> Result<Cell> {
    Err(error(
        "invalid.quantified_type",
        "Comparison must return Boolean",
    ))
}

/// Reuse the same scalar comparison semantics for every quantified member.
fn compare_values(op: BinaryOp, left: Cell, right: Cell) -> Result<Cell> {
    let expr = BoundExpr {
        ty: SqlType::Bool,
        nullable: true,
        node: ExprNode::Binary {
            op,
            left: Box::new(crate::expression::literal(left)),
            right: Box::new(crate::expression::literal(right)),
        },
    };
    eval_expr(&expr, &vec![])
}

/// The canonical constructor validates rectangularity and preserves array bounds.
fn array(element: &SqlType, values: &[BoundExpr], row: &Row) -> Result<Cell> {
    let values = values
        .iter()
        .map(|value| eval_expr(value, row))
        .collect::<Result<Vec<_>>>()?;
    scalar::array_constructor(element, &values)
}

/// Numeric negation reuses checked subtraction and preserves typed NULL.
fn negate(arg: &BoundExpr, row: &Row) -> Result<Cell> {
    let value = eval_expr(arg, row)?;
    let zero = scalar::input(&value.sql_type(), "0")?;
    scalar::arithmetic("subtract", &zero, &value)
}

/// Evaluate immutable function arguments in their declared positional order.
fn call(name: &str, args: &[BoundExpr], row: &Row) -> Result<Cell> {
    let values = args
        .iter()
        .map(|arg| eval_expr(arg, row))
        .collect::<Result<Vec<_>>>()?;
    scalar::function(name, &values)
}

/// CASE runtime branches are conditional; planner constant-folding is a separate obligation.
fn case(branches: &[(BoundExpr, BoundExpr)], otherwise: &BoundExpr, row: &Row) -> Result<Cell> {
    for (condition, value) in branches {
        if eval_expr(condition, row)? == Cell::Bool(true) {
            return eval_expr(value, row);
        }
    }
    eval_expr(otherwise, row)
}

/// COALESCE preserves short-circuit value selection and the resolved common SQL type.
fn coalesce(args: &[BoundExpr], ty: &SqlType, row: &Row) -> Result<Cell> {
    for arg in args {
        let value = eval_expr(arg, row)?;
        if !value.is_null() {
            return Ok(value);
        }
    }
    Ok(Cell::Null(ty.clone()))
}

/// NULLIF uses SQL equality; a NULL comparison operand does not make equality TRUE.
fn nullif(left: &BoundExpr, right: &BoundExpr, row: &Row) -> Result<Cell> {
    let a = eval_expr(left, row)?;
    let b = eval_expr(right, row)?;
    if !a.is_null() && !b.is_null() && scalar::compare_sql(&a, &b)? == std::cmp::Ordering::Equal {
        Ok(Cell::Null(left.ty.clone()))
    } else {
        Ok(a)
    }
}

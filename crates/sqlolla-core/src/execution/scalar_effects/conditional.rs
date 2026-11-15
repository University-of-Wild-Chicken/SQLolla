//! Lazy control flow preserves branch-local state before invoking a selected continuation.
use super::*;

/// A CASE condition contributes events before exactly one result or later condition runs.
pub(super) fn case(
    branches: &[(BoundExpr, BoundExpr)],
    otherwise: &BoundExpr,
    row: &Row,
    db: &Database,
    context: &Runtime<'_>,
) -> Result<Vec<ScalarOutcome>> {
    let Some(((condition, result), rest)) = branches.split_first() else {
        return evaluate(otherwise, row, db, context);
    };
    let mut output = Vec::new();
    for condition in evaluate(condition, row, db, context)? {
        let Ok(value) = condition else {
            push(&mut output, condition, context)?;
            continue;
        };
        let child = context.fork(&value.effects);
        let values = if value.value == Cell::Bool(true) {
            evaluate(result, row, db, &child)?
        } else {
            case(rest, otherwise, row, db, &child)?
        };
        append(&mut output, values, &value.effects, context)?;
    }
    Ok(output)
}

/// COALESCE allocates or emits from later arguments only when all preceding values were NULL.
pub(super) fn coalesce(
    args: &[BoundExpr],
    ty: &SqlType,
    row: &Row,
    db: &Database,
    context: &Runtime<'_>,
) -> Result<Vec<ScalarOutcome>> {
    let Some((first, rest)) = args.split_first() else {
        return Ok(vec![classify(Ok(Cell::Null(ty.clone())))?]);
    };
    let mut output = Vec::new();
    for outcome in evaluate(first, row, db, context)? {
        match outcome {
            Ok(value) if value.value.is_null() => {
                let child = context.fork(&value.effects);
                append(
                    &mut output,
                    coalesce(rest, ty, row, db, &child)?,
                    &value.effects,
                    context,
                )?;
            }
            value => push(&mut output, value, context)?,
        }
    }
    Ok(output)
}

/// A decisive left operand suppresses the unreachable right invocation, retaining SQL UNKNOWN.
pub(super) fn boolean(
    expr: &BoundExpr,
    row: &Row,
    db: &Database,
    context: &Runtime<'_>,
) -> Result<Vec<ScalarOutcome>> {
    let ExprNode::Binary { op, left, right } = &expr.node else {
        unreachable!()
    };
    let mut output = Vec::new();
    for left in evaluate(left, row, db, context)? {
        let Ok(left) = left else {
            push(&mut output, left, context)?;
            continue;
        };
        if decisive(*op, &left.value) {
            push(&mut output, Ok(left), context)?;
            continue;
        }
        let child = context.fork(&left.effects);
        let values = boolean_right(*op, &left.value, right, row, db, &child)?;
        append(&mut output, values, &left.effects, context)?;
    }
    Ok(output)
}

/// A decisive SQL truth value suppresses all right-side invocation effects.
fn decisive(op: BinaryOp, value: &Cell) -> bool {
    matches!(
        (op, value),
        (BinaryOp::And, Cell::Bool(false)) | (BinaryOp::Or, Cell::Bool(true))
    )
}

/// Combine right alternatives only after their inherited left-side state is fixed.
fn boolean_right(
    op: BinaryOp,
    left: &Cell,
    right: &BoundExpr,
    row: &Row,
    db: &Database,
    context: &Runtime<'_>,
) -> Result<Vec<ScalarOutcome>> {
    let mut output = Vec::new();
    for right in evaluate(right, row, db, context)? {
        let value = match right {
            Err(failure) => Err(failure),
            Ok(right) => prepend(
                classify(subquery::scalar_compare(op, left.clone(), right.value))?,
                &right.effects,
            ),
        };
        push(&mut output, value, context)?;
    }
    Ok(output)
}

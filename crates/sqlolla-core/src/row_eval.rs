//! Composite equality and lexicographic comparison preserve PostgreSQL row NULL rules.
use crate::*;

/// Evaluate component pairs left-to-right; a decisive comparison does not inspect later cells.
pub(crate) fn compare(
    op: BinaryOp,
    left: &[BoundExpr],
    right: &[BoundExpr],
    row: &Row,
) -> Result<Cell> {
    let equality = matches!(op, BinaryOp::Eq | BinaryOp::Ne);
    let mut unknown = false;
    for (a, b) in left.iter().zip(right) {
        let a = eval_expr(a, row)?;
        let b = eval_expr(b, row)?;
        if a.is_null() || b.is_null() {
            if !equality {
                return Ok(Cell::Null(SqlType::Bool));
            }
            unknown = true;
            continue;
        }
        let comparison = scalar::compare_sql(&a, &b)?;
        if comparison != std::cmp::Ordering::Equal {
            return Ok(Cell::Bool(compare_order(op, comparison)));
        }
    }
    Ok(if unknown {
        Cell::Null(SqlType::Bool)
    } else {
        Cell::Bool(matches!(op, BinaryOp::Eq | BinaryOp::Le | BinaryOp::Ge))
    })
}

/// Map the first unequal component to the row comparison's requested relation.
fn compare_order(op: BinaryOp, order: std::cmp::Ordering) -> bool {
    match op {
        BinaryOp::Eq => false,
        BinaryOp::Ne => true,
        BinaryOp::Lt | BinaryOp::Le => order.is_lt(),
        BinaryOp::Gt | BinaryOp::Ge => order.is_gt(),
        BinaryOp::And | BinaryOp::Or => false,
    }
}

//! Query-aware scalar execution preserves lazy branches and explicit outer-row substitution.
use super::*;

/// Evaluate a scalar under one shared statement budget; no external state is consulted.
pub(super) fn scalar(
    expr: &BoundExpr,
    row: &Row,
    db: &Database,
    context: &Runtime<'_>,
) -> Result<Cell> {
    context.charge()?;
    let value = resolve(expr, row, db, context)?;
    if value.sql_type() != expr.ty || (value.is_null() && !expr.nullable) {
        return Err(error(
            "invalid.expression_type",
            "Query expression annotation disagrees with value",
        ));
    }
    Ok(value)
}

/// Conditional nodes choose reachable children before ordinary scalar evaluation.
fn resolve(expr: &BoundExpr, row: &Row, db: &Database, context: &Runtime<'_>) -> Result<Cell> {
    match &expr.node {
        ExprNode::Subquery { .. } => query_value(expr, row, db, context),
        ExprNode::Context { .. } => super::session::resolve(expr, context),
        ExprNode::Call { name, args } if crate::catalog_lookup::requires_context(name) => {
            let args = args
                .iter()
                .map(|arg| scalar(arg, row, db, context))
                .collect::<Result<Vec<_>>>()?;
            super::session::function(name, &args, context)
        }
        ExprNode::OuterColumn { .. } => Err(error(
            "invalid.outer_scope",
            "Outer column was not specialized",
        )),
        ExprNode::Case {
            branches,
            otherwise,
        } => case(branches, otherwise, row, db, context),
        ExprNode::Coalesce(args) => coalesce(args, &expr.ty, row, db, context),
        ExprNode::Binary {
            op: BinaryOp::And | BinaryOp::Or,
            ..
        } => boolean(expr, row, db, context),
        ExprNode::RowCompare { op, left, right } => row_compare(*op, left, right, row, db, context),
        _ => grounded(expr, row, db, context),
    }
}

/// A decisive Boolean operand suppresses an unreachable runtime subquery, preserving UNKNOWN.
fn boolean(expr: &BoundExpr, row: &Row, db: &Database, context: &Runtime<'_>) -> Result<Cell> {
    let ExprNode::Binary { op, left, right } = &expr.node else {
        unreachable!()
    };
    let left = scalar(left, row, db, context)?;
    if matches!(
        (op, &left),
        (BinaryOp::And, Cell::Bool(false)) | (BinaryOp::Or, Cell::Bool(true))
    ) {
        return Ok(left);
    }
    scalar_compare(*op, left, scalar(right, row, db, context)?)
}

/// Reuse deterministic scalar operators after evaluating only their immediate child expressions.
fn grounded(expr: &BoundExpr, row: &Row, db: &Database, context: &Runtime<'_>) -> Result<Cell> {
    let mut value = expr.clone();
    crate::rewrite::scalar_children(&mut value, &mut |child| {
        *child = crate::expression::literal(scalar(child, row, db, context)?);
        Ok(())
    })?;
    eval_expr(&value, row)
}

/// CASE never evaluates a runtime result expression belonging to an unselected branch.
fn case(
    branches: &[(BoundExpr, BoundExpr)],
    otherwise: &BoundExpr,
    row: &Row,
    db: &Database,
    context: &Runtime<'_>,
) -> Result<Cell> {
    for (condition, value) in branches {
        if scalar(condition, row, db, context)? == Cell::Bool(true) {
            return scalar(value, row, db, context);
        }
    }
    scalar(otherwise, row, db, context)
}

/// COALESCE skips later query expressions once a non-NULL value is available.
fn coalesce(
    args: &[BoundExpr],
    ty: &SqlType,
    row: &Row,
    db: &Database,
    context: &Runtime<'_>,
) -> Result<Cell> {
    for arg in args {
        let value = scalar(arg, row, db, context)?;
        if !value.is_null() {
            return Ok(value);
        }
    }
    Ok(Cell::Null(ty.clone()))
}

/// Row comparison evaluates later components only while earlier pairs remain undecided.
fn row_compare(
    op: BinaryOp,
    left: &[BoundExpr],
    right: &[BoundExpr],
    row: &Row,
    db: &Database,
    context: &Runtime<'_>,
) -> Result<Cell> {
    let equality = matches!(op, BinaryOp::Eq | BinaryOp::Ne);
    let mut unknown = false;
    for (a, b) in left.iter().zip(right) {
        let a = scalar(a, row, db, context)?;
        let b = scalar(b, row, db, context)?;
        if a.is_null() || b.is_null() {
            if !equality {
                return Ok(Cell::Null(SqlType::Bool));
            }
            unknown = true;
            continue;
        }
        let compared = scalar_compare(op, a.clone(), b.clone())?;
        if crate::scalar::compare_sql(&a, &b)?.is_ne() {
            return Ok(compared);
        }
    }
    Ok(truth_result(
        unknown,
        matches!(op, BinaryOp::Eq | BinaryOp::Le | BinaryOp::Ge),
    ))
}

/// Compare complete decoded alternatives; differing observations are refused, never selected.
fn query_value(expr: &BoundExpr, row: &Row, db: &Database, context: &Runtime<'_>) -> Result<Cell> {
    let ExprNode::Subquery { query, .. } = &expr.node else {
        unreachable!()
    };
    let mut query = (**query).clone();
    crate::rewrite::relation(&mut query, &mut |value| specialize(value, row, 0, context))?;
    let mut observation: Option<Result<Cell>> = None;
    for outcome in super::relational::query(&query, db, context)? {
        let value = match outcome {
            Ok(value) => decode(expr, value, row, db, context),
            Err(_) => {
                return Err(error(
                    "unsupported.subquery_error_order",
                    "Streaming subquery error precedence requires additional semantics",
                ));
            }
        };
        if observation
            .as_ref()
            .is_some_and(|previous| previous != &value)
        {
            return Err(error(
                "unsupported.subquery_alternatives",
                "Subquery alternatives decode to different observations",
            ));
        }
        observation = Some(value);
    }
    observation.ok_or_else(|| error("invalid.subquery_outcome", "No child alternatives"))?
}

/// Replace exactly the selected outer scope, preserving nearer scopes inside nested queries.
pub(super) fn specialize(
    expr: &mut BoundExpr,
    row: &Row,
    depth: usize,
    context: &Runtime<'_>,
) -> Result<()> {
    context.charge()?;
    if let ExprNode::OuterColumn {
        depth: reference,
        index,
    } = &expr.node
    {
        if *reference == depth {
            let value = row
                .get(*index)
                .ok_or_else(|| error("invalid.outer_column", "Outer position exceeds row"))?;
            *expr = crate::expression::literal(value.clone());
        }
        return Ok(());
    }
    if let ExprNode::Subquery { query, .. } = &mut expr.node {
        crate::rewrite::relation(query, &mut |value| {
            specialize(value, row, depth + 1, context)
        })?;
    }
    crate::rewrite::scalar_children(expr, &mut |child| specialize(child, row, depth, context))
}

/// Decode cardinality, existence, array order or quantified three-valued truth explicitly.
pub(super) fn decode(
    expr: &BoundExpr,
    value: RelationResult,
    row: &Row,
    db: &Database,
    context: &Runtime<'_>,
) -> Result<Cell> {
    super::rel_effects::require_pure(&value)?;
    let ExprNode::Subquery { kind, left, op, .. } = &expr.node else {
        unreachable!()
    };
    match kind {
        SubqueryKind::Scalar => scalar_value(&expr.ty, &value.rows),
        SubqueryKind::Exists => Ok(Cell::Bool(!value.rows.is_empty())),
        SubqueryKind::Array => array_value(&expr.ty, value),
        SubqueryKind::In | SubqueryKind::Any | SubqueryKind::All => {
            let all = *kind == SubqueryKind::All;
            let left = comparison_left(left.as_deref())?;
            let left = comparison_value(left, value.rows.is_empty(), row, db, context)?;
            if value.rows.is_empty() {
                return Ok(Cell::Bool(all));
            }
            let op = op
                .ok_or_else(|| error("invalid.subquery_operator", "Missing comparison operator"))?;
            quantified(all, op, left, &value.rows)
        }
    }
}

/// Empty subqueries can skip runtime operands, but constant-lineage planning may still fail first.
fn comparison_value(
    expr: &BoundExpr,
    empty: bool,
    row: &Row,
    db: &Database,
    context: &Runtime<'_>,
) -> Result<Cell> {
    match scalar(expr, row, db, context) {
        Err(failure) if empty && (failure.sql.is_some() || failure.code.starts_with("sql.")) => {
            Err(error(
                "unsupported.subquery_operand_planning",
                "Empty subquery operand failure depends on unmodeled constant lineage",
            ))
        }
        result => result,
    }
}

/// A missing operand is malformed bound IR, never SQL NULL.
fn comparison_left(left: Option<&BoundExpr>) -> Result<&BoundExpr> {
    left.ok_or_else(|| error("invalid.subquery_operand", "Missing comparison operand"))
}

/// Construct SQL three-valued Boolean transport without losing UNKNOWN.
fn truth_result(unknown: bool, otherwise: bool) -> Cell {
    if unknown {
        Cell::Null(SqlType::Bool)
    } else {
        Cell::Bool(otherwise)
    }
}

/// Two identical occurrences still violate scalar subquery cardinality.
fn scalar_value(ty: &SqlType, rows: &Bag) -> Result<Cell> {
    match rows.as_slice() {
        [] => Ok(Cell::Null(ty.clone())),
        [row] => row
            .first()
            .cloned()
            .ok_or_else(|| error("invalid.subquery_width", "Missing scalar column")),
        _ => Err(error(
            "sql.21000",
            "Scalar subquery returned more than one row",
        )),
    }
}

/// Unordered ARRAY(query) is admitted only when every row permutation is observationally identical.
fn array_value(ty: &SqlType, relation: RelationResult) -> Result<Cell> {
    let SqlType::Array(element) = ty else {
        return Err(error("invalid.array_type", "Expected array type"));
    };
    let values = relation
        .rows
        .iter()
        .map(|row| row[0].clone())
        .collect::<Vec<_>>();
    if relation.ordering == ResultOrdering::Bag
        && values
            .first()
            .is_some_and(|first| values.iter().any(|value| value != first))
    {
        return Err(error(
            "unsupported.subquery_alternatives",
            "Unordered ARRAY query has multiple visible element orders",
        ));
    }
    crate::scalar::array_constructor(element, &values)
}

/// ANY/ALL retain NULL comparisons, with empty identities handled before evaluating the operand.
fn quantified(all: bool, op: BinaryOp, left: Cell, rows: &Bag) -> Result<Cell> {
    let mut unknown = false;
    for row in rows {
        match scalar_compare(op, left.clone(), row[0].clone())? {
            Cell::Bool(value) if value != all => return Ok(Cell::Bool(value)),
            Cell::Null(_) => unknown = true,
            _ => (),
        }
    }
    Ok(if unknown {
        Cell::Null(SqlType::Bool)
    } else {
        Cell::Bool(all)
    })
}

/// Literal comparison delegates to the same checked SQL scalar operator as ordinary predicates.
pub(super) fn scalar_compare(op: BinaryOp, left: Cell, right: Cell) -> Result<Cell> {
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

//! Relational type admission precedes row execution; generic scalar NULL behavior stays separate.
use crate::json::error;
use crate::*;

/// Require actual comparison support recursively, independently of a current value or row count.
pub(crate) fn require(ty: &SqlType) -> Result<()> {
    match ty {
        SqlType::Array(element) => require(element),
        _ => scalar::comparison_type(ty, ty).map(|_| ()),
    }
}

/// Equality-only families cannot acquire SQL ordering through Rust transport ordering.
pub(crate) fn ordered(ty: &SqlType) -> Result<()> {
    match ty {
        SqlType::Xid => Err(error("sql.42883", "No ordering operator for xid")),
        SqlType::Array(element) => ordered(element),
        _ => require(ty),
    }
}

/// Scalar and composite comparisons retain PostgreSQL operator-class distinctions.
pub(crate) fn operator(op: BinaryOp, ty: &SqlType) -> Result<()> {
    match op {
        BinaryOp::Lt | BinaryOp::Le | BinaryOp::Gt | BinaryOp::Ge if *ty == SqlType::Xid => {
            ordered(ty)
        }
        _ => Ok(()),
    }
}

/// Validate every relational comparison requirement before PostgreSQL-style query execution.
pub(crate) fn relation(plan: &Plan) -> Result<()> {
    requirements(plan).map_err(planning_failure)?;
    children(&plan.node)
}

/// DISTINCT, grouping, sorting and non-concatenating set operations require typed operators.
fn requirements(plan: &Plan) -> Result<()> {
    match &plan.node {
        RelNode::Distinct { .. } => columns(&plan.schema),
        RelNode::Set { op, all, .. } if *op != SetOp::Union || !all => columns(&plan.schema),
        RelNode::DistinctOn { keys, .. } => expressions(keys),
        RelNode::Order { keys, .. } => orders(keys),
        RelNode::Aggregate {
            groups, aggregates, ..
        } => {
            expressions(groups)?;
            aggregates
                .iter()
                .filter(|value| value.distinct)
                .filter_map(|value| value.argument.as_ref())
                .try_for_each(|value| require(&value.ty))
        }
        RelNode::Window { spec, .. } => {
            expressions(&spec.partition_by)?;
            orders(&spec.order_by)
        }
        _ => Ok(()),
    }
}

/// Check all physical output positions without inspecting their current bags.
fn columns(columns: &[Column]) -> Result<()> {
    columns.iter().try_for_each(|column| require(&column.ty))
}

/// Every distinct/group partition key retains its own declared scalar family.
fn expressions(expressions: &[BoundExpr]) -> Result<()> {
    expressions
        .iter()
        .try_for_each(|expression| require(&expression.ty))
}

/// NULL placement and direction do not create an otherwise absent ordering operator.
fn orders(orders: &[OrderKey]) -> Result<()> {
    orders
        .iter()
        .try_for_each(|order| ordered(&order.expression.ty))
}

/// Recursively inspect bound children even when an outer LIMIT or empty predicate hides them.
fn children(node: &RelNode) -> Result<()> {
    match node {
        RelNode::Locked(plan) => relation(&plan.source),
        RelNode::Filter { input, .. }
        | RelNode::Project { input, .. }
        | RelNode::Window { input, .. }
        | RelNode::ExpandSets { input, .. }
        | RelNode::Distinct { input }
        | RelNode::DistinctOn { input, .. }
        | RelNode::Order { input, .. }
        | RelNode::Slice { input, .. }
        | RelNode::Unordered { input }
        | RelNode::Aggregate { input, .. } => relation(input),
        RelNode::Product { left, right }
        | RelNode::Join { left, right, .. }
        | RelNode::Set { left, right, .. } => {
            relation(left)?;
            relation(right)
        }
        RelNode::Values { .. } | RelNode::EvaluateValues { .. } | RelNode::Scan { .. } => Ok(()),
    }
}

/// Actual PostgreSQL EXPLAIN probes establish this relational rejection before execution.
fn planning_failure(mut failure: Error) -> Error {
    if let Some(code) = failure.code.strip_prefix("sql.") {
        failure.sql = Some(Box::new(SqlFailure {
            sqlstate: code.into(),
            phase: FailurePhase::Planning,
            constraint_name: None,
            column_name: None,
            exposed_rows: vec![],
            effects: vec![],
        }));
        failure
    } else {
        error(&failure.code, failure.message)
    }
}

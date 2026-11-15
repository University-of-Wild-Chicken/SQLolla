//! Exhausting-consumer certificates keep referenced read CTEs distinct from actually executed CTEs.
use super::*;
use std::collections::BTreeSet;

/// Proven finite cardinality properties of already bound virtual CTE relations.
#[derive(Default)]
struct Bounds {
    at_most_one: BTreeSet<String>,
    exactly_one: BTreeSet<String>,
}

/// Structural demand distinguishes proved consumers from observations needing branch certificates.
pub(super) struct Demand {
    pub required: BTreeSet<String>,
    pub proven: BTreeSet<String>,
}

/// Record missing observable producers without treating eager evaluation as proof of demand.
pub(super) fn demand(plan: &WithPlan) -> Result<Demand> {
    let bounds = bounds(plan);
    let mut demanded = consumed(&plan.body, &bounds)?;
    let mut required = BTreeSet::new();
    for binding in plan.bindings.iter().rev() {
        let uses = crate::cte::dependencies(&binding.statement)?;
        let guaranteed = demanded.contains(&binding.table_id) || !uses.writes.is_empty();
        if (uses.locks || uses.ordered_effects) && uses.writes.is_empty() && !guaranteed {
            required.insert(binding.table_id.clone());
        }
        if guaranteed {
            demanded.extend(consumed(&binding.statement, &bounds)?);
        }
    }
    Ok(Demand {
        required,
        proven: demanded,
    })
}

/// Bounds follow syntax and prior CTE definitions, never one chosen physical row fixture.
fn bounds(plan: &WithPlan) -> Bounds {
    let mut bounds = Bounds::default();
    for binding in &plan.bindings {
        if cte_demand::singleton(&binding.statement, &bounds.at_most_one) {
            bounds.at_most_one.insert(binding.table_id.clone());
        }
        if let StatementPlan::Query(query) = binding.statement.as_ref() {
            if nonempty_singleton(query, &bounds.exactly_one) {
                bounds.exactly_one.insert(binding.table_id.clone());
            }
        }
    }
    bounds
}

/// Arbitrary mutation scans can skip or stop a source; they require a separate demand profile.
fn consumed(plan: &StatementPlan, bounds: &Bounds) -> Result<BTreeSet<String>> {
    match plan {
        StatementPlan::Query(query) => scans(query, bounds),
        _ => Ok(BTreeSet::new()),
    }
}

/// Only exhausting operators propagate a proof that their input must be requested to completion.
fn scans(plan: &Plan, bounds: &Bounds) -> Result<BTreeSet<String>> {
    match &plan.node {
        RelNode::Scan { table } => Ok(BTreeSet::from([table.clone()])),
        RelNode::Project { input, .. }
            if total_expressions(plan)? || cte_demand::relation(input, &bounds.at_most_one) =>
        {
            scans(input, bounds)
        }
        RelNode::Aggregate {
            input, aggregates, ..
        } if aggregates
            .iter()
            .all(|call| call.function == AggregateFunction::Count)
            && total_expressions(plan)? =>
        {
            scans(input, bounds)
        }
        _ => unary_scans(plan, bounds),
    }
}

/// Exhausting unary stages retain child demand only when their scalar work cannot stop it early.
fn unary_scans(plan: &Plan, bounds: &Bounds) -> Result<BTreeSet<String>> {
    match &plan.node {
        RelNode::Order { input, .. }
        | RelNode::Window { input, .. }
        | RelNode::DistinctOn { input, .. }
        | RelNode::ExpandSets { input, .. }
            if total_expressions(plan)? =>
        {
            scans(input, bounds)
        }
        RelNode::Unordered { input } | RelNode::Distinct { input } => scans(input, bounds),
        _ => binary_scans(plan, bounds),
    }
}

/// UNION exhausts both children; generic joins receive no such certificate from a reference alone.
fn binary_scans(plan: &Plan, bounds: &Bounds) -> Result<BTreeSet<String>> {
    match &plan.node {
        RelNode::Set {
            op: SetOp::Union,
            left,
            right,
            ..
        } => both(left, right, bounds),
        RelNode::Product { left, right } if singleton_pair(left, right, bounds) => {
            both(left, right, bounds)
        }
        RelNode::Join {
            kind: JoinKind::Inner,
            left,
            right,
            on,
        } if matches!(on.node, ExprNode::Literal(Cell::Bool(true)))
            && singleton_pair(left, right, bounds) =>
        {
            both(left, right, bounds)
        }
        _ => Ok(BTreeSet::new()),
    }
}

/// A nonempty singleton product cannot skip either input because its sibling is empty.
fn singleton_pair(left: &Plan, right: &Plan, bounds: &Bounds) -> bool {
    nonempty_singleton(left, &bounds.exactly_one) && nonempty_singleton(right, &bounds.exactly_one)
}

/// A complete UNION consumes both operands; certified products retain both dependency sets too.
fn both(left: &Plan, right: &Plan, bounds: &Bounds) -> Result<BTreeSet<String>> {
    let mut result = scans(left, bounds)?;
    result.extend(scans(right, bounds)?);
    Ok(result)
}

/// Exactly one output occurrence is stronger than an at-most-one bound that may be empty.
fn nonempty_singleton(plan: &Plan, inputs: &BTreeSet<String>) -> bool {
    match &plan.node {
        RelNode::Values { rows } => rows.len() == 1,
        RelNode::EvaluateValues { rows } => rows.len() == 1,
        RelNode::Scan { table } => inputs.contains(table),
        RelNode::Project { input, .. }
        | RelNode::Order { input, .. }
        | RelNode::Unordered { input }
        | RelNode::Distinct { input }
        | RelNode::Window { input, .. } => nonempty_singleton(input, inputs),
        RelNode::Product { left, right } => {
            nonempty_singleton(left, inputs) && nonempty_singleton(right, inputs)
        }
        _ => false,
    }
}

/// Potential scalar errors or nested demand cannot be hidden behind a claimed exhausting transform.
fn total_expressions(plan: &Plan) -> Result<bool> {
    let mut total = true;
    crate::rewrite::relation_expressions(&mut plan.clone(), &mut |expression| {
        total &= total_scalar(expression);
        Ok(())
    })?;
    Ok(total)
}

/// The initial total-expression profile excludes partial casts, arithmetic and effectful callbacks.
fn total_scalar(expression: &BoundExpr) -> bool {
    match &expression.node {
        ExprNode::Literal(_) | ExprNode::Column(_) => true,
        ExprNode::IsNull { arg, .. } | ExprNode::Not(arg) => total_scalar(arg),
        ExprNode::Binary { op, left, right } => {
            total_comparison(*op, &left.ty) && total_scalar(left) && total_scalar(right)
        }
        ExprNode::Coalesce(args) => args.iter().all(total_scalar),
        ExprNode::Case {
            branches,
            otherwise,
        } => {
            branches
                .iter()
                .all(|(condition, value)| total_scalar(condition) && total_scalar(value))
                && total_scalar(otherwise)
        }
        _ => false,
    }
}

/// Runtime-only unsupported array element comparators cannot be treated as total predicates.
fn total_comparison(op: BinaryOp, ty: &SqlType) -> bool {
    if matches!(
        op,
        BinaryOp::Lt | BinaryOp::Le | BinaryOp::Gt | BinaryOp::Ge
    ) {
        crate::comparison::ordered(ty).is_ok()
    } else {
        crate::comparison::require(ty).is_ok()
    }
}

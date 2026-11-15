//! Shared structural traversal preserves every bound expression while changing its meaning explicitly.
use crate::*;

/// Transform statement-owned expression roots without inventing or discarding effects.
pub(crate) fn statement<F>(plan: &mut StatementPlan, visit: &mut F) -> Result<()>
where
    F: FnMut(&mut BoundExpr) -> Result<()>,
{
    match plan {
        StatementPlan::Initialized { body, .. } => statement(body, visit),
        StatementPlan::With(plan) => with_statement(plan, visit),
        StatementPlan::Query(plan) => relation(plan, visit),
        StatementPlan::LockedQuery(plan) => relation(&mut plan.source, visit),
        StatementPlan::Insert(plan) => {
            insert(plan, visit)?;
            expressions(&mut plan.returning.expressions, visit)
        }
        StatementPlan::Update(plan) => {
            update(plan, visit)?;
            expressions(&mut plan.returning.expressions, visit)
        }
        StatementPlan::Delete(plan) => {
            optional_relation(&mut plan.source, visit)?;
            visit(&mut plan.predicate)?;
            expressions(&mut plan.returning.expressions, visit)
        }
        StatementPlan::Rejected { .. }
        | StatementPlan::CreateTable(_)
        | StatementPlan::CreateView(_)
        | StatementPlan::DropView(_) => Ok(()),
    }
}

/// Every UPDATE source, predicate and assignment keeps its original expression scope.
fn update<F>(plan: &mut UpdatePlan, visit: &mut F) -> Result<()>
where
    F: FnMut(&mut BoundExpr) -> Result<()>,
{
    optional_relation(&mut plan.source, visit)?;
    visit(&mut plan.predicate)?;
    for assignment in &mut plan.assignments {
        visit(&mut assignment.expression)?;
    }
    Ok(())
}

/// Demanded CTE expressions are planned under their own bound scopes.
fn with_statement<F>(plan: &mut WithPlan, visit: &mut F) -> Result<()>
where
    F: FnMut(&mut BoundExpr) -> Result<()>,
{
    for binding in &mut plan.bindings {
        statement(&mut binding.statement, visit)?;
    }
    statement(&mut plan.body, visit)
}

/// FROM and USING expressions remain part of statement planning and semantic rewrites.
fn optional_relation<F>(plan: &mut Option<Plan>, visit: &mut F) -> Result<()>
where
    F: FnMut(&mut BoundExpr) -> Result<()>,
{
    if let Some(plan) = plan {
        relation(plan, visit)?;
    }
    Ok(())
}

/// An INSERT owns both source expressions and the separate conflict-update scope.
fn insert<F>(plan: &mut InsertPlan, visit: &mut F) -> Result<()>
where
    F: FnMut(&mut BoundExpr) -> Result<()>,
{
    relation(&mut plan.source, visit)?;
    if let Some(conflict) = &mut plan.conflict {
        conflict_expressions(conflict, visit)?;
    }
    Ok(())
}

/// Conflict predicates and assignments participate in planning and context specialization.
fn conflict_expressions<F>(plan: &mut ConflictPlan, visit: &mut F) -> Result<()>
where
    F: FnMut(&mut BoundExpr) -> Result<()>,
{
    if let ConflictTarget::PredicateColumns { predicate, .. } = &mut plan.target {
        visit(predicate)?;
    }
    if let ConflictActionPlan::Update {
        assignments,
        predicate,
    } = &mut plan.action
    {
        visit(predicate)?;
        for assignment in assignments {
            visit(&mut assignment.expression)?;
        }
    }
    Ok(())
}

/// Traverse relational children before their owned scalar expression roots.
pub(crate) fn relation<F>(plan: &mut Plan, visit: &mut F) -> Result<()>
where
    F: FnMut(&mut BoundExpr) -> Result<()>,
{
    children(&mut plan.node, visit)?;
    relation_expressions(plan, visit)
}

/// Transform only the scalar roots owned by this relation, leaving child traversal to the caller.
pub(crate) fn relation_expressions<F>(plan: &mut Plan, visit: &mut F) -> Result<()>
where
    F: FnMut(&mut BoundExpr) -> Result<()>,
{
    match &mut plan.node {
        RelNode::EvaluateValues { rows } => {
            for row in rows {
                expressions(row, visit)?;
            }
            Ok(())
        }
        RelNode::Filter { predicate, .. } => visit(predicate),
        RelNode::DistinctOn { keys, .. } => expressions(keys, visit),
        RelNode::ExpandSets { arrays, .. } => expressions(arrays, visit),
        RelNode::Window { spec, .. } => window(spec, visit),
        RelNode::Project {
            expressions: values,
            ..
        } => expressions(values, visit),
        RelNode::Join { on, .. } => visit(on),
        RelNode::Aggregate {
            groups, aggregates, ..
        } => aggregate(groups, aggregates, visit),
        RelNode::Order { keys, .. } => order_keys(keys, visit),
        _ => Ok(()),
    }
}

/// Every owned ordering expression participates in the same fallible scalar traversal.
fn order_keys<F>(keys: &mut [OrderKey], visit: &mut F) -> Result<()>
where
    F: FnMut(&mut BoundExpr) -> Result<()>,
{
    for key in keys {
        visit(&mut key.expression)?;
    }
    Ok(())
}

/// Recursively transform all relation inputs with an exhaustive operator match.
fn children<F>(node: &mut RelNode, visit: &mut F) -> Result<()>
where
    F: FnMut(&mut BoundExpr) -> Result<()>,
{
    match node {
        RelNode::Locked(plan) => relation(&mut plan.source, visit),
        RelNode::Filter { input, .. }
        | RelNode::Project { input, .. }
        | RelNode::Distinct { input }
        | RelNode::DistinctOn { input, .. }
        | RelNode::ExpandSets { input, .. }
        | RelNode::Window { input, .. }
        | RelNode::Order { input, .. }
        | RelNode::Slice { input, .. }
        | RelNode::Unordered { input }
        | RelNode::Aggregate { input, .. } => relation(input, visit),
        RelNode::Product { left, right }
        | RelNode::Join { left, right, .. }
        | RelNode::Set { left, right, .. } => {
            relation(left, visit)?;
            relation(right, visit)
        }
        RelNode::Values { .. } | RelNode::EvaluateValues { .. } | RelNode::Scan { .. } => Ok(()),
    }
}

/// Every window partition and ordering expression participates in substitution and planner checks.
fn window<F>(spec: &mut WindowSpec, visit: &mut F) -> Result<()>
where
    F: FnMut(&mut BoundExpr) -> Result<()>,
{
    expressions(&mut spec.partition_by, visit)?;
    for key in &mut spec.order_by {
        visit(&mut key.expression)?;
    }
    Ok(())
}

/// Every aggregate input/filter participates in context substitution and planner checks.
fn aggregate<F>(groups: &mut [BoundExpr], calls: &mut [AggregateExpr], visit: &mut F) -> Result<()>
where
    F: FnMut(&mut BoundExpr) -> Result<()>,
{
    expressions(groups, visit)?;
    for call in calls {
        if let Some(filter) = &mut call.filter {
            visit(filter)?;
        }
        if let Some(arg) = &mut call.argument {
            visit(arg)?;
        }
    }
    Ok(())
}

/// Apply a fallible transform in explicit output-column order.
pub(crate) fn expressions<F>(values: &mut [BoundExpr], visit: &mut F) -> Result<()>
where
    F: FnMut(&mut BoundExpr) -> Result<()>,
{
    for value in values {
        visit(value)?;
    }
    Ok(())
}

/// Visit immediate scalar children; callers decide pre/post-order and conditional pruning.
pub(crate) fn scalar_children<F>(expr: &mut BoundExpr, visit: &mut F) -> Result<()>
where
    F: FnMut(&mut BoundExpr) -> Result<()>,
{
    match &mut expr.node {
        ExprNode::Literal(_)
        | ExprNode::Column(_)
        | ExprNode::Context { .. }
        | ExprNode::OuterColumn { .. } => Ok(()),
        ExprNode::Subquery { left, .. } => {
            if let Some(left) = left {
                visit(left)?;
            }
            Ok(())
        }
        ExprNode::Not(arg)
        | ExprNode::Negate(arg)
        | ExprNode::Cast { arg }
        | ExprNode::AssignmentCast { arg }
        | ExprNode::IntervalCast { arg, .. }
        | ExprNode::IsNull { arg, .. } => visit(arg),
        _ => composite_children(expr, visit),
    }
}

/// Composite expression families expose their direct scalar children without crossing query scopes.
fn composite_children<F>(expr: &mut BoundExpr, visit: &mut F) -> Result<()>
where
    F: FnMut(&mut BoundExpr) -> Result<()>,
{
    match &mut expr.node {
        ExprNode::Binary { left, right, .. }
        | ExprNode::Quantified { left, right, .. }
        | ExprNode::Arithmetic { left, right, .. }
        | ExprNode::NullIf { left, right } => {
            visit(left)?;
            visit(right)
        }
        ExprNode::RowCompare { left, right, .. } => {
            expressions(left, visit)?;
            expressions(right, visit)
        }
        ExprNode::Pattern { arg, pattern, .. } => {
            visit(arg)?;
            visit(pattern)
        }
        ExprNode::Call { args, .. }
        | ExprNode::Coalesce(args)
        | ExprNode::Array { values: args, .. } => expressions(args, visit),
        ExprNode::Case {
            branches,
            otherwise,
        } => case_children(branches, otherwise, visit),
        _ => invalid_composite(),
    }
}

/// Unrecognized IR families must not silently omit descendants from a transformation.
fn invalid_composite() -> Result<()> {
    Err(crate::json::error(
        "invalid.scalar_traversal",
        "Expected composite expression",
    ))
}

/// Traverse every CASE expression without imposing a branch selection policy.
fn case_children<F>(
    branches: &mut [(BoundExpr, BoundExpr)],
    otherwise: &mut BoundExpr,
    visit: &mut F,
) -> Result<()>
where
    F: FnMut(&mut BoundExpr) -> Result<()>,
{
    for (condition, value) in branches {
        visit(condition)?;
        visit(value)?;
    }
    visit(otherwise)
}

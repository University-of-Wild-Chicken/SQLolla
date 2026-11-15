//! Structural CTE dependencies and capability guards preserve snapshot and demand obligations.
use crate::json::error;
use crate::*;
use std::collections::BTreeSet;

/// Query or RETURNING columns exposed by a statement CTE.
pub(crate) fn schema(plan: &StatementPlan) -> Result<&[Column]> {
    match plan {
        StatementPlan::Initialized { body, .. } => schema(body),
        StatementPlan::With(plan) => schema(&plan.body),
        StatementPlan::Query(plan) => Ok(&plan.schema),
        StatementPlan::LockedQuery(plan) => Ok(&plan.schema),
        StatementPlan::Insert(plan) => Ok(&plan.returning.schema),
        StatementPlan::Update(plan) => Ok(&plan.returning.schema),
        StatementPlan::Delete(plan) => Ok(&plan.returning.schema),
        _ => Err(error(
            "unsupported.cte_statement",
            "CTE requires query or DML",
        )),
    }
}

/// Bound dependencies include reads nested inside scalar subqueries.
#[derive(Default)]
pub(crate) struct Dependencies {
    pub reads: BTreeSet<String>,
    pub writes: Vec<String>,
    pub locks: bool,
    pub ordered_effects: bool,
}

/// Remove unused pure CTEs only after binding; every writable CTE remains demanded.
pub(crate) fn retain_demanded(plan: &mut WithPlan) -> Result<()> {
    let mut needed = dependencies(&plan.body)?.reads;
    let mut live = BTreeSet::new();
    for binding in plan.bindings.iter().rev() {
        let uses = dependencies(&binding.statement)?;
        if needed.contains(&binding.table_id) || !uses.writes.is_empty() {
            if needed.contains(&binding.table_id) && schema(&binding.statement)?.is_empty() {
                return Err(error(
                    "unsupported.cte_no_returning",
                    "Referenced DML CTE requires RETURNING",
                ));
            }
            live.insert(binding.table_id.clone());
            needed.extend(uses.reads);
        }
    }
    plan.bindings
        .retain(|binding| live.contains(&binding.table_id));
    admission(plan)
}

/// Disjoint target deltas compose; execution independently certifies every observable read producer.
fn admission(plan: &WithPlan) -> Result<()> {
    let mut seen = BTreeSet::new();
    let uses = dependencies(&StatementPlan::With(plan.clone()))?;
    for target in uses.writes {
        if !seen.insert(target) {
            return Err(error(
                "unsupported.cte_write_overlap",
                "Overlapping CTE target writes require row-identity composition",
            ));
        }
    }
    Ok(())
}

/// Collect statement inputs without executing, reparsing or consulting table contents.
pub(crate) fn dependencies(plan: &StatementPlan) -> Result<Dependencies> {
    let mut uses = Dependencies::default();
    statement(plan, &mut uses)?;
    Ok(uses)
}

/// Visit each statement-owned relation and scalar expression exactly once structurally.
fn statement(plan: &StatementPlan, uses: &mut Dependencies) -> Result<()> {
    match plan {
        StatementPlan::Initialized { body, .. } => statement(body, uses),
        StatementPlan::With(plan) => with_dependencies(plan, uses),
        StatementPlan::Query(plan) => relation(plan, uses),
        StatementPlan::LockedQuery(plan) => locked(&plan.source, uses),
        StatementPlan::Insert(plan) => {
            uses.writes.push(plan.target.table.clone());
            relation(&plan.source, uses)?;
            own_expressions(&StatementPlan::Insert(plan.clone()), uses)
        }
        StatementPlan::Update(plan) => {
            uses.writes.push(plan.target.table.clone());
            optional(&plan.source, uses)?;
            own_expressions(&StatementPlan::Update(plan.clone()), uses)
        }
        StatementPlan::Delete(plan) => {
            uses.writes.push(plan.target.table.clone());
            optional(&plan.source, uses)?;
            own_expressions(&StatementPlan::Delete(plan.clone()), uses)
        }
        _ => Err(error(
            "unsupported.cte_statement",
            "CTE requires query or DML",
        )),
    }
}

/// A nested WITH retains every demanded child dependency and its final body dependency.
fn with_dependencies(plan: &WithPlan, uses: &mut Dependencies) -> Result<()> {
    for binding in &plan.bindings {
        statement(&binding.statement, uses)?;
    }
    statement(&plan.body, uses)
}

/// Optional mutation source references must participate in CTE demand analysis.
fn optional(plan: &Option<Plan>, uses: &mut Dependencies) -> Result<()> {
    if let Some(plan) = plan {
        relation(plan, uses)?;
    }
    Ok(())
}

/// Reuse the complete statement expression visitor for nested scalar query dependencies.
fn own_expressions(plan: &StatementPlan, uses: &mut Dependencies) -> Result<()> {
    crate::rewrite::statement(&mut plan.clone(), &mut |expr| expression(expr, uses))
}

/// Read-only relational traversal preserves physical and virtual relation dependencies.
fn relation(plan: &Plan, uses: &mut Dependencies) -> Result<()> {
    match &plan.node {
        RelNode::Locked(plan) => locked(&plan.source, uses)?,
        RelNode::Scan { table } => {
            uses.reads.insert(table.clone());
        }
        RelNode::Slice { input, .. }
        | RelNode::Filter { input, .. }
        | RelNode::Project { input, .. }
        | RelNode::Distinct { input }
        | RelNode::DistinctOn { input, .. }
        | RelNode::ExpandSets { input, .. }
        | RelNode::Window { input, .. }
        | RelNode::Order { input, .. }
        | RelNode::Unordered { input }
        | RelNode::Aggregate { input, .. } => relation(input, uses)?,
        RelNode::Product { left, right }
        | RelNode::Join { left, right, .. }
        | RelNode::Set { left, right, .. } => pair(left, right, uses)?,
        _ => {}
    }
    crate::rewrite::relation_expressions(&mut plan.clone(), &mut |expr| expression(expr, uses))
}

/// Correlated subqueries retain the same base snapshot and explicit lexical references.
fn expression(expr: &mut BoundExpr, uses: &mut Dependencies) -> Result<()> {
    if let ExprNode::Call { name, .. } = &expr.node {
        uses.ordered_effects |= crate::function_registry::builtin(name)
            .is_some_and(|spec| spec.effect != crate::function_registry::Effect::None);
    }
    if let ExprNode::Subquery { query, .. } = &expr.node {
        relation(query, uses)?;
    }
    crate::rewrite::scalar_children(expr, &mut |child| expression(child, uses))
}

/// Binary sources contribute both dependency cones without imposing execution order.
fn pair(left: &Plan, right: &Plan, uses: &mut Dependencies) -> Result<()> {
    relation(left, uses)?;
    relation(right, uses)
}

/// Record row acquisition capability while preserving the locked source's relation dependencies.
fn locked(source: &Plan, uses: &mut Dependencies) -> Result<()> {
    uses.locks = true;
    relation(source, uses)
}

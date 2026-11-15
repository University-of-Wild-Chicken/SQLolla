//! Whole-job scheduling is complete only where competing effectful jobs cannot interleave rows.
use super::*;
use std::collections::BTreeSet;

/// Count ordered jobs and refuse unrepresented tuple interleavings before any job can execute.
pub(super) fn atomic(plan: &WithPlan, context: &Runtime<'_>) -> Result<bool> {
    let mut singleton_inputs = BTreeSet::new();
    let mut jobs = Vec::new();
    for binding in &plan.bindings {
        let single = singleton(&binding.statement, &singleton_inputs);
        if ordered(&binding.statement, context)? {
            jobs.push(single);
        }
        if single {
            singleton_inputs.insert(binding.table_id.clone());
        }
    }
    if ordered(&plan.body, context)? {
        jobs.push(singleton(&plan.body, &singleton_inputs));
    }
    if jobs.len() > 1 && !jobs.iter().all(|single| *single) {
        return Err(error(
            "unsupported.cte_tuple_schedule",
            "Competing multirow effectful CTEs require tuple-level scheduling",
        ));
    }
    Ok(!jobs.is_empty())
}

/// Contextual INSERT defaults participate in ordering independently of explicit source calls.
fn ordered(plan: &StatementPlan, context: &Runtime<'_>) -> Result<bool> {
    let mut effects = crate::cte::dependencies(plan)?.ordered_effects;
    match plan {
        StatementPlan::Insert(insert) => effects |= insert_defaults::ordered(insert, context)?,
        StatementPlan::With(with) => {
            for binding in &with.bindings {
                effects |= ordered(&binding.statement, context)?;
            }
            effects |= ordered(&with.body, context)?;
        }
        _ => {}
    }
    Ok(effects)
}

/// Structural bounds never infer singleton producers from a chosen physical database fixture.
pub(super) fn singleton(plan: &StatementPlan, inputs: &BTreeSet<String>) -> bool {
    match plan {
        StatementPlan::Query(query) => relation(query, inputs),
        StatementPlan::Insert(insert) => relation(&insert.source, inputs),
        _ => false,
    }
}

/// Only operators preserving an at-most-one tuple bound participate in the atomic job profile.
pub(super) fn relation(plan: &Plan, inputs: &BTreeSet<String>) -> bool {
    match &plan.node {
        RelNode::Values { rows } => rows.len() <= 1,
        RelNode::EvaluateValues { rows } => rows.len() <= 1,
        RelNode::Scan { table } => inputs.contains(table),
        RelNode::Project { input, .. }
        | RelNode::Filter { input, .. }
        | RelNode::Order { input, .. }
        | RelNode::Unordered { input }
        | RelNode::Distinct { input }
        | RelNode::DistinctOn { input, .. }
        | RelNode::Window { input, .. }
        | RelNode::Slice { input, .. }
        | RelNode::Aggregate { input, .. } => relation(input, inputs),
        RelNode::Product { left, right } => relation(left, inputs) && relation(right, inputs),
        RelNode::Join {
            kind, left, right, ..
        } if *kind != JoinKind::Full => relation(left, inputs) && relation(right, inputs),
        _ => false,
    }
}

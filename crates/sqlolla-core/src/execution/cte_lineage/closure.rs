//! Backward closure starts only from full results and successful mutations, never from eager execution.
use super::*;

/// Each producer becomes necessary only after every distinct output occurrence has a necessary witness.
pub(super) fn verify(
    evidence: &Evidence,
    plan: &WithPlan,
    db: &Database,
    context: &Runtime<'_>,
) -> Result<()> {
    let mut guaranteed = BTreeSet::from([plan.bindings.len()]);
    for (index, binding) in plan.bindings.iter().enumerate() {
        if evidence.proven.contains(&binding.table_id)
            || !crate::cte::dependencies(&binding.statement)?
                .writes
                .is_empty()
        {
            guaranteed.insert(index);
        }
    }
    let mut coverage = BTreeMap::<String, BTreeSet<usize>>::new();
    let mut processed = BTreeSet::new();
    loop {
        let next = guaranteed.difference(&processed).next().copied();
        let Some(index) = next else {
            break;
        };
        context.charge()?;
        processed.insert(index);
        merge(&mut coverage, links(evidence, plan, index, db, context)?);
        extend(evidence, plan, &coverage, &mut guaranteed)?;
    }
    validate_required(evidence, plan, &guaranteed, context)
}

/// Instrumented read jobs use saved origins; mutations trace only already-materialized pure inputs.
fn links(
    evidence: &Evidence,
    plan: &WithPlan,
    index: usize,
    db: &Database,
    context: &Runtime<'_>,
) -> Result<BTreeMap<String, BTreeSet<usize>>> {
    let value = observed(evidence, index)?;
    if let Some(trace) = &value.trace {
        return Ok(trace::coverage(trace));
    }
    let statement = plan
        .bindings
        .get(index)
        .map_or(plan.body.as_ref(), |item| item.statement.as_ref());
    mutation::coverage(statement, value, db, &context.fork(&value.prefix))
}

/// Coverage unions are identity sets; this never canonicalizes SQL bag occurrences by their values.
fn merge(
    target: &mut BTreeMap<String, BTreeSet<usize>>,
    incoming: BTreeMap<String, BTreeSet<usize>>,
) {
    for (table, occurrences) in incoming {
        target.entry(table).or_default().extend(occurrences);
    }
}

/// Nonempty complete occurrence coverage is propagated only through event-closed producers.
fn extend(
    evidence: &Evidence,
    plan: &WithPlan,
    coverage: &BTreeMap<String, BTreeSet<usize>>,
    guaranteed: &mut BTreeSet<usize>,
) -> Result<()> {
    for (index, binding) in plan.bindings.iter().enumerate() {
        let rows = &observed(evidence, index)?.rows;
        let all = coverage
            .get(&binding.table_id)
            .is_some_and(|ids| (0..rows.len()).all(|id| ids.contains(&id)));
        if !rows.is_empty() && all && closed(&binding.statement)? {
            guaranteed.insert(index);
        }
    }
    Ok(())
}

/// A successful certificate cannot erase a nonempty event tail behind zero output occurrences.
fn validate_required(
    evidence: &Evidence,
    plan: &WithPlan,
    guaranteed: &BTreeSet<usize>,
    context: &Runtime<'_>,
) -> Result<()> {
    for (index, binding) in plan.bindings.iter().enumerate() {
        if !evidence.required.contains(&binding.table_id) {
            continue;
        }
        let value = observed(evidence, index)?;
        if no_novel_events(value, context) || guaranteed.contains(&index) {
            continue;
        }
        return Err(error(
            "unsupported.cte_consumer_demand",
            "Complete producer occurrence/event closure was not certified",
        ));
    }
    Ok(())
}

/// LockRows owns its consumed offset/page prefix; scalar events must occur in final output projection.
fn closed(plan: &StatementPlan) -> Result<bool> {
    match plan {
        StatementPlan::LockedQuery(_) => Ok(true),
        StatementPlan::Query(query) => closed_query(query),
        _ => Ok(false),
    }
}

/// No reducing stage or hidden effectful input may leave an unwitnessed event tail behind output rows.
fn closed_query(plan: &Plan) -> Result<bool> {
    match &plan.node {
        RelNode::Unordered { input } => closed_query(input),
        RelNode::Project { input, .. } => Ok(!lock_demand::ordered(input)?
            && !lock_demand::contains(input)?
            && trace::no_subquery(plan)),
        RelNode::Locked(_) => Ok(true),
        _ => Ok(!lock_demand::ordered(plan)? && !lock_demand::contains(plan)?),
    }
}

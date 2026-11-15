//! Independent effectful CTEs enumerate dependency-valid schedules instead of assuming lexical order.
use super::*;

/// The final body is another job; only its declared dependencies force it after a CTE.
pub(super) fn orders(plan: &WithPlan, context: &Runtime<'_>) -> Result<Vec<Vec<usize>>> {
    if !super::cte_demand::atomic(plan, context)? {
        return Ok(vec![(0..=plan.bindings.len()).collect()]);
    }
    let deps = dependencies(plan)?;
    let mut output = Vec::new();
    enumerate(&deps, &mut Vec::new(), &mut output, context)?;
    Ok(output)
}

/// CTE result identities define edges; ordinary physical table reads retain the original snapshot.
fn dependencies(plan: &WithPlan) -> Result<Vec<Vec<usize>>> {
    let mut output = Vec::new();
    for statement in plan
        .bindings
        .iter()
        .map(|item| item.statement.as_ref())
        .chain(std::iter::once(plan.body.as_ref()))
    {
        let reads = crate::cte::dependencies(statement)?.reads;
        output.push(
            plan.bindings
                .iter()
                .enumerate()
                .filter_map(|(index, item)| reads.contains(&item.table_id).then_some(index))
                .collect(),
        );
    }
    Ok(output)
}

/// Refuse capacity exhaustion before losing any permitted schedule or branch-local prefix.
fn enumerate(
    deps: &[Vec<usize>],
    prior: &mut Vec<usize>,
    output: &mut Vec<Vec<usize>>,
    context: &Runtime<'_>,
) -> Result<()> {
    context.charge()?;
    if prior.len() == deps.len() {
        return push(output, prior.clone(), context);
    }
    let mut advanced = false;
    for (index, dependencies) in deps.iter().enumerate() {
        if prior.contains(&index) || dependencies.iter().any(|id| !prior.contains(id)) {
            continue;
        }
        advanced = true;
        prior.push(index);
        enumerate(deps, prior, output, context)?;
        prior.pop();
    }
    if !advanced {
        return Err(error(
            "invalid.cte_schedule",
            "Cyclic or absent CTE dependency",
        ));
    }
    Ok(())
}

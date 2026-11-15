//! Enumerate only consumed unordered prefixes; never fabricate a preferred LIMIT lock order.
use super::*;

/// Retain each observed ordering or enumerate every relevant unordered locking prefix.
pub(super) fn candidates(
    value: &RelationResult,
    plan: &LockedQueryPlan,
    locks: &LockContext,
    context: &Runtime<'_>,
) -> Result<Vec<Bag>> {
    let rows = available(&value.rows, plan, locks)?;
    if value.ordering == ResultOrdering::Ordered || fixed_membership(plan, rows.len()) {
        return Ok(vec![rows]);
    }
    let needed = plan
        .limit
        .map_or(rows.len(), |n| plan.offset.saturating_add(n))
        .min(rows.len());
    let mut output = Vec::new();
    prefixes(
        &rows,
        needed,
        &mut vec![false; rows.len()],
        &mut Vec::new(),
        &mut output,
        context,
    )?;
    Ok(output)
}

/// Without a limiting boundary, permutations cannot change output membership or final ownership.
fn fixed_membership(plan: &LockedQueryPlan, rows: usize) -> bool {
    plan.limit == Some(0) || (plan.limit.is_none() && plan.offset == 0) || plan.offset >= rows
}

/// SKIP LOCKED removes static conflicts before any OFFSET/LIMIT count is consumed.
fn available(rows: &Bag, plan: &LockedQueryPlan, locks: &LockContext) -> Result<Bag> {
    let mut output = Vec::new();
    for row in rows {
        if plan.clause.wait_policy != RowWaitPolicy::SkipLocked
            || !row_unavailable(locks, &plan.table, ordinal(row)?, plan.clause.strength)
        {
            output.push(row.clone());
        }
    }
    Ok(output)
}

/// Every candidate prefix has distinct physical occurrences, even for duplicate SQL values.
fn prefixes(
    rows: &Bag,
    needed: usize,
    used: &mut [bool],
    prefix: &mut Bag,
    output: &mut Vec<Bag>,
    context: &Runtime<'_>,
) -> Result<()> {
    if prefix.len() == needed {
        return push(output, prefix.clone(), context);
    }
    for index in 0..rows.len() {
        if used[index] {
            continue;
        }
        used[index] = true;
        prefix.push(rows[index].clone());
        prefixes(rows, needed, used, prefix, output, context)?;
        prefix.pop();
        used[index] = false;
    }
    Ok(())
}

/// Decode only compiler-inserted nonnegative ordinals and reject malformed private rows.
pub(super) fn ordinal(row: &Row) -> Result<usize> {
    match row.last() {
        Some(Cell::Int64(value)) => usize::try_from(*value)
            .map_err(|_| error("invalid.lock_identity", "Negative occurrence identity")),
        _ => Err(error(
            "invalid.lock_identity",
            "Missing occurrence identity",
        )),
    }
}

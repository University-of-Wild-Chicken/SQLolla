//! Relation initialization precedes row demand and cannot invent tuple acquisition.
use super::*;

/// Resolve all fixed-snapshot relation waits before evaluating the already-planned body.
pub(super) fn execute(
    relations: &[String],
    body: &StatementPlan,
    database: &Database,
    context: &Runtime<'_>,
) -> Result<OutcomeSet> {
    let events = observations(relations, context)?;
    let child = context.fork(&events);
    let outcomes = evaluate_resolved(body, database, &child)?;
    Ok(OutcomeSet {
        outcomes: outcomes
            .outcomes
            .into_iter()
            .map(|outcome| rel_effects::statement(outcome, &events))
            .collect(),
    })
}

/// ROW SHARE conflict waits are independent of row NOWAIT/SKIP LOCKED clauses.
fn observations(relations: &[String], context: &Runtime<'_>) -> Result<Vec<EffectEvent>> {
    let locks = context.locks.as_ref().ok_or_else(|| {
        error(
            "unsupported.lock_context",
            "Relation initialization requires a complete fixed lock snapshot",
        )
    })?;
    if relations.is_empty() || relations.windows(2).any(|pair| pair[0] >= pair[1]) {
        return Err(error(
            "invalid.initialization",
            "Expected sorted distinct physical relations",
        ));
    }
    relations
        .iter()
        .map(|table| {
            crate::effects::validate_lock_context(locks, table)?;
            Ok(EffectEvent::TableLock {
                table: table.clone(),
                mode: "row_share".into(),
            })
        })
        .collect()
}

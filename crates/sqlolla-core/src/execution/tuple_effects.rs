//! Bind explicit header transitions to branch-local row witnesses without mutating the input snapshot.
use super::*;

/// System RETURNING requires headers even on empty sources; supplied profiles cannot be ignored.
pub(super) fn admit(plan: &InsertPlan, db: &Database, context: &Runtime<'_>) -> Result<()> {
    let Some(tuples) = &context.tuples else {
        return if plan.returning.system_columns {
            Err(error(
                "unsupported.tuple_context",
                "System columns require explicit tuple context",
            ))
        } else {
            Ok(())
        };
    };
    let rows = mutation::original(&plan.target, db)?;
    tuples.validate_target(&plan.target.table, rows)
}

/// One physical transition carries its exact row lineage and never alters the public table schema.
pub(super) fn record(
    target: &MutationTarget,
    occurrence: usize,
    action: TupleAction,
    row: &Row,
    effects: &mut Vec<EffectEvent>,
    db: &Database,
    context: &Runtime<'_>,
) -> Result<()> {
    let Some(tuples) = &context.tuples else {
        return Ok(());
    };
    let prefix = observations(context.inherited_effects(), effects);
    let event = row_event(target, occurrence, action, row, &prefix, db, tuples)?;
    validate_tuple_witness_bytes(&prefix, &event)?;
    mutation::check_row(row, &target.schema)?;
    if action == TupleAction::ConflictLock && event.before_row.as_ref() != Some(row) {
        return Err(error(
            "invalid.tuple_row",
            "Lock witness does not match current occurrence",
        ));
    }
    effects.push(EffectEvent::TupleVersion {
        observation: Box::new(event),
    });
    Ok(())
}

/// Ground one replayed header step in its original and immediate predecessor user rows.
fn row_event(
    target: &MutationTarget,
    occurrence: usize,
    action: TupleAction,
    row: &Row,
    prefix: &[TupleVersionEvent],
    db: &Database,
    context: &TupleContext,
) -> Result<TupleVersionEvent> {
    let transitions = prefix
        .iter()
        .map(|event| event.transition.clone())
        .collect::<Vec<_>>();
    let transition = tuple_step(context, &transitions, &target.table, occurrence, action)?;
    let original_row = db[&target.table].get(occurrence).cloned();
    let before_row = last(prefix, &target.table, occurrence)
        .map(|event| event.after_row.clone())
        .or(original_row.clone());
    Ok(TupleVersionEvent {
        transition,
        original_row,
        before_row,
        after_row: row.clone(),
    })
}

/// A stable candidate slot is resolved independently of row value duplicates or output sorting.
fn last<'a>(
    events: &'a [TupleVersionEvent],
    table: &str,
    occurrence: usize,
) -> Option<&'a TupleVersionEvent> {
    events
        .iter()
        .rev()
        .find(|event| event.transition.table == table && event.transition.occurrence == occurrence)
}

/// Keep ordered physical transitions separate from unrelated session/lock observations.
fn observations(inherited: &[EffectEvent], local: &[EffectEvent]) -> Vec<TupleVersionEvent> {
    inherited
        .iter()
        .chain(local)
        .filter_map(|event| match event {
            EffectEvent::TupleVersion { observation } => Some((**observation).clone()),
            _ => None,
        })
        .collect()
}

/// Append the returned version's identities only to the private expression evaluation scope.
pub(super) fn returning_row(
    output: &Returning,
    row: &Row,
    table: &str,
    occurrence: usize,
    effects: &[EffectEvent],
    context: &Runtime<'_>,
) -> Result<Row> {
    if !output.system_columns {
        return Ok(row.clone());
    }
    let events = observations(context.inherited_effects(), effects);
    let event = returned_header(&events, table, occurrence, row)?;
    let mut scope = row.clone();
    for value in [event.transition.after.xmin, event.transition.after.raw_xmax] {
        scope.push(crate::scalar::input(&SqlType::Xid, &value.to_string())?);
    }
    Ok(scope)
}

/// The returned row must be the version named by the most recent validated slot transition.
fn returned_header<'a>(
    events: &'a [TupleVersionEvent],
    table: &str,
    occurrence: usize,
    row: &Row,
) -> Result<&'a TupleVersionEvent> {
    let event = last(events, table, occurrence).ok_or_else(|| {
        error(
            "invalid.tuple_returning",
            "Returned row has no version witness",
        )
    })?;
    if event.after_row != *row {
        return Err(error(
            "invalid.tuple_returning",
            "Version witness row mismatch",
        ));
    }
    Ok(event)
}

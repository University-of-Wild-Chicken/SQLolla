//! Branch-local observation composition separates held ownership from ordered external events.
use super::*;
use std::collections::{BTreeMap, BTreeSet};

/// Canonicalize ownership only; external event order and duplicate occurrences remain observable.
pub(super) fn merge(left: &[EffectEvent], right: &[EffectEvent]) -> Vec<EffectEvent> {
    let mut tables = BTreeSet::new();
    let mut rows = BTreeMap::new();
    let mut events = Vec::new();
    for effect in left.iter().chain(right) {
        accumulate(effect, &mut tables, &mut rows, &mut events);
    }
    let mut result: Vec<_> = tables
        .into_iter()
        .map(|(table, mode)| EffectEvent::TableLock { table, mode })
        .collect();
    result.extend(
        rows.into_iter()
            .map(|((table, occurrence), strength)| EffectEvent::RowLock {
                table,
                occurrence,
                strength,
            }),
    );
    result.extend(advisory(left, right));
    result.extend(events);
    result
}

/// Separate commutative lock ownership from the order-sensitive external event subsequence.
fn accumulate(
    effect: &EffectEvent,
    tables: &mut BTreeSet<(String, String)>,
    rows: &mut BTreeMap<(String, usize), RowLockMode>,
    events: &mut Vec<EffectEvent>,
) {
    match effect {
        EffectEvent::TableLock { table, mode } => {
            tables.insert((table.clone(), mode.clone()));
        }
        EffectEvent::RowLock {
            table,
            occurrence,
            strength,
        } => {
            let held = rows
                .entry((table.clone(), *occurrence))
                .or_insert(*strength);
            *held = (*held).max(*strength);
        }
        EffectEvent::AdvisoryLock { .. } => {}
        _ => events.push(effect.clone()),
    }
}

/// Advisory ownership retains namespace, owner and every actually held mode as canonical facts.
fn advisory(left: &[EffectEvent], right: &[EffectEvent]) -> Vec<EffectEvent> {
    left.iter()
        .chain(right)
        .filter_map(|effect| match effect {
            EffectEvent::AdvisoryLock { held } => Some(held.clone()),
            _ => None,
        })
        .collect::<BTreeSet<_>>()
        .into_iter()
        .map(|held| EffectEvent::AdvisoryLock { held })
        .collect()
}

/// Rollback retains allocations and normalized physical tuple remnants, releasing transactional locks.
pub(super) fn rollback(effects: &[EffectEvent]) -> Vec<EffectEvent> {
    let mut retained: Vec<_> = effects
        .iter()
        .filter(|effect| {
            matches!(
                effect,
                EffectEvent::SequenceValue { .. }
                    | EffectEvent::BackendTerminationRequested { .. }
                    | EffectEvent::SqlWarning { .. }
            )
        })
        .cloned()
        .collect();
    let tuples = effects
        .iter()
        .filter_map(|effect| match effect {
            EffectEvent::TupleVersion { observation } => Some((**observation).clone()),
            _ => None,
        })
        .collect::<Vec<_>>();
    retained.extend(
        crate::rollback_tuple_events(&tuples)
            .into_iter()
            .map(|observation| EffectEvent::TupleVersion {
                observation: Box::new(observation),
            }),
    );
    retained
}

/// Attach inherited observations to one branch without changing its row or error identity.
pub(super) fn inherit(mut outcome: RelationOutcome, effects: &[EffectEvent]) -> RelationOutcome {
    match &mut outcome {
        Ok(value) => value.effects = merge(effects, &value.effects),
        Err(failure) => failure.effects = rollback(&merge(effects, &failure.effects)),
    }
    outcome
}

/// Lift a fallible relation transform over one branch and preserve its entire observation trace.
pub(super) fn transform<F>(
    value: RelationOutcome,
    context: &Runtime<'_>,
    apply: F,
) -> Result<Vec<RelationOutcome>>
where
    F: FnOnce(RelationResult, &Runtime<'_>) -> Result<Vec<RelationOutcome>>,
{
    let Ok(mut value) = value else {
        return Ok(vec![value]);
    };
    let effects = std::mem::take(&mut value.effects);
    let child = context.fork(&effects);
    let outcomes = match apply(value, &child) {
        Ok(outcomes) => outcomes,
        Err(failure) => vec![execution_error(failure)?],
    };
    Ok(outcomes
        .into_iter()
        .map(|outcome| inherit(outcome, &effects))
        .collect())
}

/// Statement composition obeys the same rollback and ordered-event rules as relational composition.
pub(super) fn statement(
    mut outcome: StatementOutcome,
    effects: &[EffectEvent],
) -> StatementOutcome {
    match &mut outcome {
        StatementOutcome::Succeeded(value) => value.effects = merge(effects, &value.effects),
        StatementOutcome::SqlError(failure) => {
            failure.effects = rollback(&merge(effects, &failure.effects));
        }
    }
    outcome
}

/// Scalar-only consumers must refuse observations that their return type cannot represent.
pub(super) fn require_pure(value: &RelationResult) -> Result<()> {
    if value.effects.is_empty() {
        return Ok(());
    }
    Err(error(
        "unsupported.subquery_effects",
        "Scalar decoding cannot discard child effects",
    ))
}

/// Binary evaluation order is unmodeled for noncommutative external event traces.
pub(super) fn parallel(left: &RelationOutcome, right: &RelationOutcome) -> Result<()> {
    for outcome in [left, right] {
        let effects = match outcome {
            Ok(value) => &value.effects,
            Err(failure) => &failure.effects,
        };
        if effects.iter().any(|event| {
            !matches!(
                event,
                EffectEvent::TableLock { .. } | EffectEvent::RowLock { .. }
            )
        }) {
            return Err(error(
                "unsupported.binary_effect_order",
                "Binary consumers require explicit external event evaluation order",
            ));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Reacquisition preserves strongest ownership while retaining duplicate ordered allocations.
    #[test]
    fn ownership_and_event_multiplicity_are_distinct() {
        let weak = EffectEvent::RowLock {
            table: "t".into(),
            occurrence: 0,
            strength: RowLockMode::Share,
        };
        let strong = EffectEvent::RowLock {
            table: "t".into(),
            occurrence: 0,
            strength: RowLockMode::Update,
        };
        let event = EffectEvent::SequenceValue {
            sequence: "s".into(),
            value: 7,
        };
        let combined = merge(&[weak, event.clone()], &[strong.clone(), event.clone()]);
        assert_eq!(combined, vec![strong, event.clone(), event]);
    }

    /// Failed atomic publication releases new ownership and transactional notifications only.
    #[test]
    fn rollback_retains_only_nontransactional_allocations() {
        let event = EffectEvent::SequenceValue {
            sequence: "s".into(),
            value: 7,
        };
        let effects = vec![
            EffectEvent::TableLock {
                table: "t".into(),
                mode: "row_share".into(),
            },
            event.clone(),
            EffectEvent::Notification {
                channel: "x".into(),
                payload: "y".into(),
            },
            event.clone(),
        ];
        assert_eq!(rollback(&effects), vec![event.clone(), event]);
    }
}

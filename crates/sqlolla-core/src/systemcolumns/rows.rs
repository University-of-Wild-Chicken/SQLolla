//! Transactional row rollback and raw header residue are projected together without conflation.
use super::*;

/// Retain one original-row witness per residual header; inserted visible versions disappear.
pub fn rollback_tuple_events(events: &[TupleVersionEvent]) -> Vec<TupleVersionEvent> {
    let transitions = events
        .iter()
        .map(|value| value.transition.clone())
        .collect::<Vec<_>>();
    rollback_tuple_observations(&transitions)
        .into_iter()
        .filter_map(|transition| {
            let event = events.iter().find(|value| {
                value.transition.table == transition.table
                    && value.transition.occurrence == transition.occurrence
            })?;
            let row = event.original_row.clone()?;
            Some(TupleVersionEvent {
                transition,
                original_row: Some(row.clone()),
                before_row: Some(row.clone()),
                after_row: row,
            })
        })
        .collect()
}

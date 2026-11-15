//! Replay explicit tuple journals and project physical abort residuals.
use super::*;

/// Materialized current header positions remain separate from the caller's table rows.
type HeaderState = BTreeMap<(String, usize), TupleHeader>;

/// Produce one bounded transition from explicit context and a validated branch-local prefix.
pub fn tuple_step(
    context: &TupleContext,
    prefix: &[TupleVersionObservation],
    table: &str,
    occurrence: usize,
    action: TupleAction,
) -> Result<TupleVersionObservation> {
    context.profile()?;
    capacity(prefix)?;
    let state = replay(context, prefix)?;
    let original = original(context, table, occurrence)?;
    check_slot(context.tables[table].len(), &state, table, occurrence)?;
    let before = state
        .get(&(table.into(), occurrence))
        .cloned()
        .or(original.clone());
    let after = transitions::apply(context.write_xid, before.as_ref(), action)?;
    Ok(TupleVersionObservation {
        table: table.into(),
        occurrence,
        phase: TupleObservationPhase::Applied,
        rollback: original
            .as_ref()
            .map(|header| transitions::abort(context.write_xid, header)),
        original,
        before,
        after,
    })
}

/// The public boundary refuses a truncated effect journal before producing another transition.
fn capacity(prefix: &[TupleVersionObservation]) -> Result<()> {
    if prefix.len() >= MAX_TUPLE_EVENTS {
        return Err(error(
            "resource.tuple_events",
            "Tuple journal capacity exceeded",
        ));
    }
    Ok(())
}

/// Input identity is read from the supplied snapshot, never from an intermediate version.
fn original(context: &TupleContext, table: &str, occurrence: usize) -> Result<Option<TupleHeader>> {
    let headers = context.tables.get(table).ok_or_else(|| {
        error(
            "unsupported.tuple_context",
            "Target tuple headers are missing",
        )
    })?;
    let original = headers.get(occurrence).cloned();
    if let Some(header) = &original {
        header.validate(context.write_xid)?;
    }
    Ok(original)
}

/// Appended slots are contiguous and never replace an existing input occurrence.
fn check_slot(initial: usize, state: &HeaderState, table: &str, occurrence: usize) -> Result<()> {
    let inserted = state
        .keys()
        .filter(|(key, slot)| key == table && *slot >= initial)
        .count();
    if occurrence > initial + inserted {
        return Err(error(
            "invalid.tuple_slot",
            "New tuple slots must be contiguous",
        ));
    }
    Ok(())
}

/// Validate every prefix entry, including entries for other tables and occurrences.
fn replay(context: &TupleContext, prefix: &[TupleVersionObservation]) -> Result<HeaderState> {
    let mut state = HeaderState::new();
    for entry in prefix {
        validate_entry(context, &state, entry)?;
        state.insert((entry.table.clone(), entry.occurrence), entry.after.clone());
    }
    Ok(state)
}

/// Prefix chaining, rollback source and physical transitions must all agree with the initial snapshot.
fn validate_entry(
    context: &TupleContext,
    state: &HeaderState,
    entry: &TupleVersionObservation,
) -> Result<()> {
    let original = original(context, &entry.table, entry.occurrence)?;
    let count = context.tables[&entry.table].len();
    check_slot(count, state, &entry.table, entry.occurrence)?;
    let before = state
        .get(&(entry.table.clone(), entry.occurrence))
        .cloned()
        .or(original.clone());
    let rollback = original
        .as_ref()
        .map(|header| transitions::abort(context.write_xid, header));
    if entry.phase != TupleObservationPhase::Applied
        || entry.original != original
        || entry.before != before
        || entry.rollback != rollback
        || !allowed(context.write_xid, before.as_ref(), &entry.after)
    {
        return Err(error(
            "invalid.tuple_journal",
            "Tuple prefix does not replay from this snapshot",
        ));
    }
    Ok(())
}

/// A supplied prefix must be realizable by one of the admitted physical operations.
fn allowed(writer: u32, before: Option<&TupleHeader>, after: &TupleHeader) -> bool {
    match before {
        None => *after == transitions::insert(writer),
        Some(header) => {
            *after == transitions::lock(writer, header)
                || *after == transitions::update(writer, header)
        }
    }
}

/// Project a validated journal to canonical original-row abort residue; inserted versions disappear.
pub fn rollback_tuple_observations(
    prefix: &[TupleVersionObservation],
) -> Vec<TupleVersionObservation> {
    let mut residuals = BTreeMap::new();
    for entry in prefix {
        let (Some(original), Some(after)) = (&entry.original, &entry.rollback) else {
            continue;
        };
        residuals.insert(
            (entry.table.clone(), entry.occurrence),
            TupleVersionObservation {
                table: entry.table.clone(),
                occurrence: entry.occurrence,
                phase: TupleObservationPhase::AbortedResidual,
                original: Some(original.clone()),
                before: Some(original.clone()),
                after: after.clone(),
                rollback: Some(after.clone()),
            },
        );
    }
    residuals.into_values().collect()
}

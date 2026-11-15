//! Pure, branch-local sequence allocation under an explicit exclusive snapshot.
use crate::json::error;
use crate::{EffectEvent, Result};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

/// Complete supplied sequence state; absent concurrency evidence cannot imply exclusivity.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SequenceContext {
    /// All sequence identities relevant to the declared execution are supplied.
    pub complete: bool,
    /// No unmodeled session consumes sequence values during this execution.
    pub exclusive_snapshot: bool,
    /// Stable sequence identity to exact catalog and current state.
    pub sequences: BTreeMap<String, SequenceSnapshot>,
}

/// Sequence catalog behavior and permission belong to the same observed session.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SequenceSnapshot {
    /// PostgreSQL relation OID used by regclass arguments.
    pub oid: u32,
    /// Exact schema identity.
    pub schema: String,
    /// Exact relation name, without SQL quoting syntax.
    pub name: String,
    /// Nontransactional state before execution.
    pub state: SequenceState,
    /// Nonzero signed allocation step.
    pub increment: i64,
    /// Inclusive minimum allocation value.
    pub minimum: i64,
    /// Inclusive maximum allocation value.
    pub maximum: i64,
    /// Wrap at the declared bound when true.
    pub cycle: bool,
    /// Initial profile requires one-value cache to avoid inventing backend cache ownership.
    pub cache: u64,
    /// Current role has USAGE or UPDATE permission.
    pub can_use: bool,
}

/// An uncalled sequence returns its current last value on its first allocation.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SequenceState {
    /// Last stored sequence value, not necessarily the value returned to a prior caller.
    pub last_value: i64,
    /// Whether nextval increments before returning.
    pub is_called: bool,
}

/// One successful transition retains both returned value and its nontransactional poststate.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SequenceAllocation {
    /// Returned PostgreSQL bigint.
    pub value: i64,
    /// State retained even when a later table write fails.
    pub state: SequenceState,
}

/// Validate explicit finite context before admitting any sequence-dependent SQL outcome.
pub fn validate(context: &SequenceContext) -> Result<()> {
    if !context.complete || !context.exclusive_snapshot {
        return Err(error(
            "unsupported.sequence_context",
            "Complete exclusive sequence snapshot required",
        ));
    }
    let mut oids = BTreeSet::new();
    let mut names = BTreeSet::new();
    for (identity, sequence) in &context.sequences {
        if identity.is_empty()
            || identity.contains('\0')
            || sequence.oid == 0
            || !oids.insert(sequence.oid)
            || !names.insert((&sequence.schema, &sequence.name))
        {
            return Err(error(
                "invalid.sequence_identity",
                "Distinct stable sequence identities required",
            ));
        }
        validate_snapshot(sequence)?;
    }
    Ok(())
}

/// Invalid bounds or state are malformed input; unmodeled caching is a coverage refusal.
fn validate_snapshot(sequence: &SequenceSnapshot) -> Result<()> {
    if sequence.increment == 0
        || sequence.minimum > sequence.maximum
        || !(sequence.minimum..=sequence.maximum).contains(&sequence.state.last_value)
        || sequence.schema.is_empty()
        || sequence.name.is_empty()
        || sequence.schema.contains('\0')
        || sequence.name.contains('\0')
    {
        return Err(error(
            "invalid.sequence_metadata",
            "Invalid sequence bounds, state or name",
        ));
    }
    if sequence.cache != 1 {
        return Err(error(
            "unsupported.sequence_cache",
            "Backend cache ownership is not modeled",
        ));
    }
    Ok(())
}

/// Allocate without mutating the supplied snapshot or another alternative's state.
pub fn allocate(sequence: &SequenceSnapshot, state: &SequenceState) -> Result<SequenceAllocation> {
    validate_snapshot(sequence)?;
    if !(sequence.minimum..=sequence.maximum).contains(&state.last_value) {
        return Err(error(
            "invalid.sequence_state",
            "Branch sequence state is outside bounds",
        ));
    }
    if !sequence.can_use {
        return Err(error("sql.42501", "Sequence allocation permission denied"));
    }
    let candidate = i128::from(state.last_value)
        + if state.is_called {
            i128::from(sequence.increment)
        } else {
            0
        };
    let value = bounded_value(sequence, candidate)?;
    Ok(SequenceAllocation {
        value,
        state: SequenceState {
            last_value: value,
            is_called: true,
        },
    })
}

/// Mathematical candidate arithmetic cannot overflow bigint before exhaustion/cycle checks.
fn bounded_value(sequence: &SequenceSnapshot, candidate: i128) -> Result<i64> {
    let outside = if candidate < i128::from(sequence.minimum) {
        Some(sequence.maximum)
    } else if candidate > i128::from(sequence.maximum) {
        Some(sequence.minimum)
    } else {
        None
    };
    match outside {
        Some(value) if sequence.cycle => Ok(value),
        Some(_) => Err(error("sql.2200H", "Sequence limit reached")),
        None => i64::try_from(candidate)
            .map_err(|_| error("invalid.sequence_range", "Invalid representable bounds")),
    }
}

/// Reconstruct only this branch's state from its explicit ordered allocation events.
pub fn states_after(
    context: &SequenceContext,
    events: &[EffectEvent],
) -> Result<BTreeMap<String, SequenceState>> {
    validate(context)?;
    let mut states = context
        .sequences
        .iter()
        .map(|(id, value)| (id.clone(), value.state.clone()))
        .collect::<BTreeMap<_, _>>();
    for event in events {
        let EffectEvent::SequenceValue { sequence, value } = event else {
            continue;
        };
        let state = states
            .get(sequence)
            .ok_or_else(|| error("invalid.sequence_trace", "Missing sequence state"))?;
        states.insert(
            sequence.clone(),
            trace_state(context, sequence, state, *value)?,
        );
    }
    Ok(states)
}

/// Verify one recorded transition before using it as the predecessor of a later allocation.
fn trace_state(
    context: &SequenceContext,
    identity: &str,
    state: &SequenceState,
    value: i64,
) -> Result<SequenceState> {
    let specification = context
        .sequences
        .get(identity)
        .ok_or_else(|| error("invalid.sequence_trace", "Unknown sequence event"))?;
    let allocation = allocate(specification, state)?;
    if allocation.value != value {
        return Err(error(
            "invalid.sequence_trace",
            "Allocation event disagrees with preceding state",
        ));
    }
    Ok(allocation.state)
}

/// Resolve one next allocation against inherited events without changing that inherited prefix.
pub fn next_event(
    context: &SequenceContext,
    events: &[EffectEvent],
    identity: &str,
) -> Result<EffectEvent> {
    let states = states_after(context, events)?;
    let specification = context
        .sequences
        .get(identity)
        .ok_or_else(|| error("sql.42P01", "Sequence does not exist"))?;
    let state = states
        .get(identity)
        .ok_or_else(|| error("invalid.sequence_state", "Missing resolved sequence"))?;
    let allocation = allocate(specification, state)?;
    Ok(EffectEvent::SequenceValue {
        sequence: identity.into(),
        value: allocation.value,
    })
}

//! Pure sequence transition regressions before SQL integration.
use sqlolla_core::*;
use std::collections::BTreeMap;

/// Build explicit metadata without borrowing sequence values from a host database.
fn snapshot() -> SequenceSnapshot {
    SequenceSnapshot {
        oid: 42,
        schema: "public".into(),
        name: "s".into(),
        state: SequenceState {
            last_value: 41,
            is_called: false,
        },
        increment: 3,
        minimum: 1,
        maximum: 100,
        cycle: false,
        cache: 1,
        can_use: true,
    }
}

/// First-call state differs from a prior allocation even when last_value is identical.
#[test]
fn first_call_and_increment() {
    let sequence = snapshot();
    let first = sequence::allocate(&sequence, &sequence.state).unwrap();
    assert_eq!(first.value, 41);
    assert!(first.state.is_called);
    let next = sequence::allocate(&sequence, &first.state).unwrap();
    assert_eq!(next.value, 44);
    assert!(!sequence.state.is_called);
}

/// Bound arithmetic is wider than bigint, so exhaustion cannot turn into a wrapped success.
#[test]
fn bigint_exhaustion_and_cycle() {
    let mut sequence = snapshot();
    sequence.maximum = i64::MAX;
    sequence.state = SequenceState {
        last_value: i64::MAX,
        is_called: true,
    };
    assert_eq!(
        sequence::allocate(&sequence, &sequence.state)
            .unwrap_err()
            .code,
        "sql.2200H"
    );
    sequence.cycle = true;
    assert_eq!(
        sequence::allocate(&sequence, &sequence.state)
            .unwrap()
            .value,
        sequence.minimum
    );
    sequence.minimum = i64::MIN;
    sequence.increment = -1;
    sequence.state.last_value = i64::MIN;
    assert_eq!(
        sequence::allocate(&sequence, &sequence.state)
            .unwrap()
            .value,
        i64::MAX
    );
}

/// A branch cannot consume another branch's allocation through shared mutable state.
#[test]
fn alternative_prefixes_are_independent() {
    let context = SequenceContext {
        complete: true,
        exclusive_snapshot: true,
        sequences: BTreeMap::from([("public.s".into(), snapshot())]),
    };
    let first = sequence::next_event(&context, &[], "public.s").unwrap();
    let repeated = sequence::next_event(&context, &[], "public.s").unwrap();
    assert_eq!(first, repeated);
    let second = sequence::next_event(&context, &[first], "public.s").unwrap();
    assert_eq!(
        second,
        EffectEvent::SequenceValue {
            sequence: "public.s".into(),
            value: 44
        }
    );
    assert!(!context.sequences["public.s"].state.is_called);
}

/// Forged allocation events and unmodeled cache/concurrent ownership cannot enter a valid trace.
#[test]
fn invalid_trace_and_context_refused() {
    let mut context = SequenceContext {
        complete: true,
        exclusive_snapshot: true,
        sequences: BTreeMap::from([("public.s".into(), snapshot())]),
    };
    let bad = EffectEvent::SequenceValue {
        sequence: "public.s".into(),
        value: 44,
    };
    assert_eq!(
        sequence::states_after(&context, &[bad]).unwrap_err().code,
        "invalid.sequence_trace"
    );
    context.sequences.get_mut("public.s").unwrap().cache = 2;
    assert_eq!(
        sequence::validate(&context).unwrap_err().code,
        "unsupported.sequence_cache"
    );
    context.exclusive_snapshot = false;
    assert_eq!(
        sequence::validate(&context).unwrap_err().code,
        "unsupported.sequence_context"
    );
}

/// Permission denial preserves the supplied sequence even for an uncalled first value.
#[test]
fn permission_denial_is_sql_error() {
    let mut sequence = snapshot();
    sequence.can_use = false;
    assert_eq!(
        sequence::allocate(&sequence, &sequence.state)
            .unwrap_err()
            .code,
        "sql.42501"
    );
    assert_eq!(sequence.state.last_value, 41);
    assert!(!sequence.state.is_called);
}

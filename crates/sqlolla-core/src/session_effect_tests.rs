//! Pure effect transitions preserve branch-local deltas and explicit ownership admission.
use crate::*;

/// Construct a complete fixed snapshot without any actual host database or signal access.
fn context() -> SessionEffectContext {
    SessionEffectContext {
        complete: true,
        transaction: "self".into(),
        held_advisory: Vec::new(),
    }
}

/// Pending duplicate invocations remain observable until commit delivery folds duplicates.
#[test]
fn notification_invocations_and_delivery_are_distinct() {
    let args = [Cell::Text("events".into()), Cell::Null(SqlType::Text)];
    let first = evaluate_session_effect("pg_notify", &args, &context(), &[]).unwrap();
    assert_eq!(first.value.sql_type(), SqlType::Void);
    assert!(!first.value.is_null());
    let second = evaluate_session_effect("pg_notify", &args, &context(), &first.effects).unwrap();
    let trace: Vec<_> = first.effects.into_iter().chain(second.effects).collect();
    assert_eq!(trace.len(), 2);
    assert_eq!(delivered_notifications(&trace).len(), 1);
}

/// Notification limits count UTF-8 bytes and a NULL channel is an SQL parameter error.
#[test]
fn notification_byte_boundaries() {
    let channel = Cell::Text("é".repeat(32));
    let args = [channel, Cell::Text("".into())];
    assert_eq!(
        evaluate_session_effect("pg_notify", &args, &context(), &[])
            .unwrap_err()
            .code,
        "sql.22023"
    );
    let args = [Cell::Null(SqlType::Text), Cell::Text("x".into())];
    assert_eq!(
        evaluate_session_effect("pg_notify", &args, &context(), &[])
            .unwrap_err()
            .code,
        "sql.22023"
    );
}

/// Reentrancy consumes the immutable prior trace without adding duplicate ownership.
#[test]
fn advisory_reentrant_delta_is_empty() {
    let args = [Cell::Int64(-1)];
    let first = evaluate_session_effect("pg_advisory_xact_lock", &args, &context(), &[]).unwrap();
    let second =
        evaluate_session_effect("pg_advisory_xact_lock", &args, &context(), &first.effects)
            .unwrap();
    assert_eq!(first.effects.len(), 1);
    assert!(second.effects.is_empty());
    assert!(evaluate_session_effect(
        "pg_advisory_xact_lock",
        &[Cell::Null(SqlType::Int64)],
        &context(),
        &[]
    )
    .unwrap()
    .value
    .is_null());
}

/// Known conflicting ownership remains a coverage refusal, not a fabricated success or timeout.
#[test]
fn advisory_conflict_is_not_a_sql_error_guess() {
    let mut context = context();
    context.held_advisory.push(AdvisoryHeld {
        key: AdvisoryKey::Int64 { key: 1 },
        owner: "other".into(),
        shared: false,
    });
    assert_eq!(
        evaluate_session_effect("pg_advisory_xact_lock", &[Cell::Int64(1)], &context, &[])
            .unwrap_err()
            .code,
        "unsupported.advisory_wait"
    );
    assert!(evaluate_session_effect(
        "pg_advisory_xact_lock",
        &[Cell::Int32(0), Cell::Int32(1)],
        &context,
        &[]
    )
    .is_ok());
}

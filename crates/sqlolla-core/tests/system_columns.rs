//! Tuple-header transitions preserve physical identity separately from logical table rollback.
use sqlolla_core::*;
use std::collections::BTreeMap;

/// One pristine visible input version belongs to an earlier committed transaction.
fn context() -> TupleContext {
    TupleContext {
        complete: true,
        exclusive_snapshot: true,
        top_level_no_savepoints: true,
        write_xid: 20,
        tables: BTreeMap::from([(
            "t".into(),
            vec![TupleHeader {
                xmin: 10,
                raw_xmax: 0,
                state: TupleXmaxState::Invalid,
            }],
        )]),
    }
}

/// Conflict update preserves a preceding own lock while ordinary unlocked update clears xmax.
#[test]
fn update_header_depends_on_lock_lineage() {
    let context = context();
    let ordinary = tuple_step(&context, &[], "t", 0, TupleAction::Update).unwrap();
    assert_eq!(ordinary.after.raw_xmax, 0);
    let lock = tuple_step(&context, &[], "t", 0, TupleAction::ConflictLock).unwrap();
    assert_eq!(lock.after.xmin, 10);
    let update = tuple_step(&context, &[lock], "t", 0, TupleAction::Update).unwrap();
    assert_eq!(update.after.xmin, 20);
    assert_eq!(update.after.raw_xmax, 20);
    assert_eq!(update.original.unwrap().xmin, 10);
}

/// Fresh insert versions have no preexisting visible version to restore on abort.
#[test]
fn inserted_version_is_absent_after_rollback() {
    let inserted = tuple_step(&context(), &[], "t", 1, TupleAction::Insert).unwrap();
    assert_eq!(inserted.after.xmin, 20);
    assert_eq!(inserted.after.raw_xmax, 0);
    assert!(inserted.original.is_none());
    assert!(rollback_tuple_observations(&[inserted]).is_empty());
}

/// A failed lock/update sequence restores original xmin but retains the aborted writer's raw xmax.
#[test]
fn abort_residual_is_not_a_table_data_rollback() {
    let context = context();
    let lock = tuple_step(&context, &[], "t", 0, TupleAction::ConflictLock).unwrap();
    let update = tuple_step(
        &context,
        std::slice::from_ref(&lock),
        "t",
        0,
        TupleAction::Update,
    )
    .unwrap();
    let residuals = rollback_tuple_observations(&[lock, update]);
    assert_eq!(residuals.len(), 1);
    assert_eq!(residuals[0].after.xmin, 10);
    assert_eq!(residuals[0].after.raw_xmax, 20);
    assert_eq!(residuals[0].phase, TupleObservationPhase::AbortedResidual);
    assert_eq!(rollback_tuple_observations(&residuals), residuals);
}

/// No SQL command spelling can replace an explicit prefix that proves an earlier own lock.
#[test]
fn prior_own_lock_matches_conflict_lock_transition() {
    let mut context = context();
    context.tables.get_mut("t").unwrap()[0] = TupleHeader {
        xmin: 10,
        raw_xmax: 20,
        state: TupleXmaxState::OwnLock,
    };
    let ordinary = tuple_step(&context, &[], "t", 0, TupleAction::Update).unwrap();
    assert_eq!(ordinary.after.raw_xmax, 20);
}

/// Changed roots, fabricated transitions and noncontiguous slot identities cannot resume a prefix.
#[test]
fn tuple_journal_replay_rejects_tampering() {
    let context = context();
    let mut entry = tuple_step(&context, &[], "t", 0, TupleAction::ConflictLock).unwrap();
    entry.after.raw_xmax = 99;
    assert_eq!(
        tuple_step(&context, &[entry], "t", 0, TupleAction::Update)
            .unwrap_err()
            .code,
        "invalid.tuple_journal"
    );
    assert_eq!(
        tuple_step(&context, &[], "t", 3, TupleAction::Insert)
            .unwrap_err()
            .code,
        "invalid.tuple_slot"
    );
}

/// Savepoint and concurrent-lock assumptions cannot disappear when the operation is otherwise simple.
#[test]
fn unavailable_profile_refuses_before_transition() {
    let mut context = context();
    context.top_level_no_savepoints = false;
    assert_eq!(
        tuple_step(&context, &[], "t", 1, TupleAction::Insert)
            .unwrap_err()
            .code,
        "unsupported.tuple_profile"
    );
    context.top_level_no_savepoints = true;
    context.tables.get_mut("t").unwrap()[0].state = TupleXmaxState::OwnLock;
    assert_eq!(
        tuple_step(&context, &[], "t", 0, TupleAction::Update)
            .unwrap_err()
            .code,
        "invalid.tuple_header"
    );
}

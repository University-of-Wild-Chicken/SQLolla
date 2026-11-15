//! Pure heap-header transitions mirror the checked restricted TupleVersion contract.
use super::*;

/// Fresh insertion has a current xmin and invalid zero xmax.
pub(super) fn insert(writer: u32) -> TupleHeader {
    TupleHeader {
        xmin: writer,
        raw_xmax: 0,
        state: TupleXmaxState::Invalid,
    }
}

/// Conflict locking changes raw xmax without creating a new row version.
pub(super) fn lock(writer: u32, before: &TupleHeader) -> TupleHeader {
    TupleHeader {
        xmin: before.xmin,
        raw_xmax: writer,
        state: TupleXmaxState::OwnLock,
    }
}

/// A new version preserves an existing own lock; otherwise xmax is initialized invalid.
pub(super) fn update(writer: u32, before: &TupleHeader) -> TupleHeader {
    if before.state == TupleXmaxState::OwnLock {
        TupleHeader {
            xmin: writer,
            raw_xmax: before.raw_xmax,
            state: TupleXmaxState::OwnLock,
        }
    } else {
        insert(writer)
    }
}

/// Abort restores the original visible version while retaining the writer's raw xmax identity.
pub(super) fn abort(writer: u32, original: &TupleHeader) -> TupleHeader {
    TupleHeader {
        xmin: original.xmin,
        raw_xmax: writer,
        state: TupleXmaxState::AbortedUpdater,
    }
}

/// Require a real predecessor for lock/update and an absent predecessor for insertion.
pub(super) fn apply(
    writer: u32,
    before: Option<&TupleHeader>,
    action: TupleAction,
) -> Result<TupleHeader> {
    match (action, before) {
        (TupleAction::Insert, None) => Ok(insert(writer)),
        (TupleAction::ConflictLock, Some(header)) => Ok(lock(writer, header)),
        (TupleAction::Update, Some(header)) => Ok(update(writer, header)),
        _ => Err(error(
            "invalid.tuple_transition",
            "Tuple operation disagrees with occurrence existence",
        )),
    }
}

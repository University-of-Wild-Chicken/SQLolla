//! Immediate row-lock behavior under an explicitly complete, fixed ownership snapshot.
use crate::{Column, FailurePhase, Plan, Result, SqlFailure};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

/// PostgreSQL row lock strength; ordering reflects increasing conflict sets.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RowLockMode {
    /// Protect key identity while allowing non-key changes.
    KeyShare,
    /// Shared row access conflicts with updates.
    Share,
    /// Non-key changes remain compatible with key-share locks.
    NoKeyUpdate,
    /// Strongest row lock conflicts with every other owner's row mode.
    Update,
}

/// Row conflict behavior, independently of relation-lock waiting.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RowWaitPolicy {
    /// A conflicting fixed snapshot cannot establish how a future wait completes.
    Block,
    /// Report SQLSTATE 55P03 if a requested row is unavailable.
    Nowait,
    /// Exclude conflicting rows before counting OFFSET or LIMIT.
    SkipLocked,
}

/// One resolved locking instruction for a single base relation.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RowLockClause {
    /// Requested row protection.
    pub strength: RowLockMode,
    /// Immediate conflict policy.
    pub wait_policy: RowWaitPolicy,
}

/// Stable occurrence identity is relative to the explicitly supplied input bag.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HeldRowLock {
    /// Stable declared table identifier.
    pub table: String,
    /// Zero-based input occurrence; duplicate-valued rows remain distinct.
    pub occurrence: usize,
    /// Explicit owner, independent of host backend process identifiers.
    pub owner: String,
    /// Existing row protection.
    pub strength: RowLockMode,
}

/// Fixed lock snapshot; no concurrent actor moves or commits are implied.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LockContext {
    /// Caller asserts complete relevant ownership and relation-conflict observations.
    pub complete: bool,
    /// Owner of this statement's transaction.
    pub transaction: String,
    /// Every currently held row lock in the relevant closed catalog.
    pub held_rows: Vec<HeldRowLock>,
    /// Tables whose existing relation locks conflict with acquiring ROW SHARE.
    pub row_share_conflicts: BTreeSet<String>,
}

/// Bound base-origin query before its final pagination is applied by lock acquisition.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct LockedQueryPlan {
    /// Pure bound query without its outer pagination; runtime adds occurrence identity.
    pub source: Plan,
    /// First base relation whose occurrences are locked.
    pub table: String,
    /// Additional distinct inner-join base origins, in source order; empty for a single table.
    #[serde(default)]
    pub joined_tables: Vec<String>,
    /// Exact resolved strength and wait policy.
    pub clause: RowLockClause,
    /// Rows consumed and locked before the first returned occurrence.
    pub offset: usize,
    /// Returned-row bound; zero locks no rows even with a positive offset.
    pub limit: Option<usize>,
    /// Original visible output metadata, excluding internal occurrence identity.
    pub schema: Vec<Column>,
}

/// Successful locked prefix and returned occurrences, kept distinct for OFFSET.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LockSelection {
    /// Base row ordinals whose locks are held after successful execution.
    pub held: Vec<usize>,
    /// Base row ordinals returned in this candidate order.
    pub returned: Vec<usize>,
}

/// PostgreSQL's symmetric row-conflict matrix; ownership is handled by the caller.
pub fn row_locks_conflict(left: RowLockMode, right: RowLockMode) -> bool {
    use RowLockMode::*;
    matches!(
        (left, right),
        (Update, _) | (_, Update) | (NoKeyUpdate, Share | NoKeyUpdate) | (Share, NoKeyUpdate)
    )
}

/// Determine whether another owner prevents this row mode under the fixed snapshot.
pub fn row_unavailable(
    context: &LockContext,
    table: &str,
    occurrence: usize,
    mode: RowLockMode,
) -> bool {
    context.held_rows.iter().any(|held| {
        held.table == table
            && held.occurrence == occurrence
            && held.owner != context.transaction
            && row_locks_conflict(mode, held.strength)
    })
}

/// Apply locking before pagination without locking rows beyond the consumed prefix.
pub fn select_locked_rows(
    candidates: &[usize],
    plan: &LockedQueryPlan,
    context: &LockContext,
) -> Result<LockSelection> {
    validate_lock_context(context, &plan.table)?;
    let mut result = LockSelection {
        held: Vec::new(),
        returned: Vec::new(),
    };
    if plan.limit == Some(0) {
        return Ok(result);
    }
    for &occurrence in candidates {
        if row_unavailable(context, &plan.table, occurrence, plan.clause.strength) {
            if plan.clause.wait_policy == RowWaitPolicy::SkipLocked {
                continue;
            }
            return Err(lock_failure(plan.clause.wait_policy));
        }
        result.held.push(occurrence);
        if result.held.len() > plan.offset {
            result.returned.push(occurrence);
        }
        if plan.limit == Some(result.returned.len()) {
            break;
        }
    }
    Ok(result)
}

/// Missing observations and unresolved relation-lock waits cannot become successful reads.
pub(crate) fn validate_lock_context(context: &LockContext, table: &str) -> Result<()> {
    if !context.complete || context.transaction.is_empty() {
        return Err(crate::json::error(
            "unsupported.lock_context",
            "A complete fixed lock snapshot and owner are required",
        ));
    }
    if context.row_share_conflicts.contains(table) {
        return Err(crate::json::error(
            "unsupported.lock_wait",
            "ROW SHARE relation-lock waiting requires a concurrent schedule",
        ));
    }
    Ok(())
}

/// NOWAIT is a SQL error; an ordinary unresolved wait remains a coverage refusal.
fn lock_failure(policy: RowWaitPolicy) -> crate::Error {
    if policy == RowWaitPolicy::Nowait {
        let mut error = crate::json::error("sql.55P03", "A requested row lock is unavailable");
        error.sql = Some(Box::new(SqlFailure {
            sqlstate: "55P03".into(),
            phase: FailurePhase::Statement,
            constraint_name: None,
            column_name: None,
            exposed_rows: vec![],
            effects: vec![],
        }));
        error
    } else {
        crate::json::error(
            "unsupported.lock_wait",
            "Conflicting blocking row locks require a concurrent schedule",
        )
    }
}

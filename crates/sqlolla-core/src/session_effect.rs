//! Pure notification/advisory transitions over explicit fixed ownership evidence.
use crate::json::error;
use crate::{Cell, EffectEvent, Result, SqlType};
use serde::{Deserialize, Serialize};

/// PostgreSQL's bigint and integer-pair advisory key spaces are deliberately distinct.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum AdvisoryKey {
    /// One signed 64-bit key in the bigint namespace.
    Int64 {
        /// Signed key, preserving all two's-complement bits.
        key: i64,
    },
    /// Two signed 32-bit keys in the integer-pair namespace.
    Int32Pair {
        /// First key.
        first: i32,
        /// Second key.
        second: i32,
    },
}

/// A currently granted advisory lock; waiting requests are outside the fixed-snapshot profile.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AdvisoryHeld {
    /// Database-local advisory key including its namespace.
    pub key: AdvisoryKey,
    /// Logical owner; the current backend uses the context transaction identity.
    pub owner: String,
    /// Shared mode may coexist with shared holders; exclusive mode conflicts across owners.
    pub shared: bool,
}

/// Closed session-effect evidence; no runtime host process or database is consulted.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SessionEffectContext {
    /// All granted advisory ownership in the current database is supplied and fixed during evaluation.
    pub complete: bool,
    /// Current logical transaction/backend owner identity.
    pub transaction: String,
    /// Complete granted ownership, including conflicting session-level locks.
    pub held_advisory: Vec<AdvisoryHeld>,
}

/// One successful scalar value and its invocation-local observation delta.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ScalarEffectStep {
    /// Typed scalar result, including non-NULL void.
    pub value: Cell,
    /// New events only; inherited events remain owned by the caller's branch.
    pub effects: Vec<EffectEvent>,
}

/// Recognize effectful calls so planners never fold them as immutable functions.
pub fn is_session_effect(name: &str) -> bool {
    matches!(
        name,
        "pg_notify" | "pg_advisory_xact_lock" | "pg_advisory_xact_lock_shared"
    )
}

/// Validate closed ownership once before using absence of a conflict as evidence.
pub fn validate_session_effect_context(context: &SessionEffectContext) -> Result<()> {
    if !context.complete || context.transaction.is_empty() {
        return Err(error(
            "unsupported.session_effect_context",
            "Complete fixed ownership and transaction identity required",
        ));
    }
    if context.held_advisory.len() > 4096 {
        return Err(error(
            "resource.advisory_snapshot",
            "Advisory ownership exceeds capacity",
        ));
    }
    if context
        .held_advisory
        .iter()
        .any(|held| held.owner.is_empty())
    {
        return Err(error("invalid.advisory_owner", "Empty advisory owner"));
    }
    Ok(())
}

/// Evaluate an invocation without mutating Runtime or the supplied ownership/event prefix.
pub fn evaluate_session_effect(
    name: &str,
    args: &[Cell],
    context: &SessionEffectContext,
    prior_effects: &[EffectEvent],
) -> Result<ScalarEffectStep> {
    args.iter().try_for_each(Cell::validate)?;
    validate_session_effect_context(context)?;
    match name {
        "pg_notify" => notify(args),
        "pg_advisory_xact_lock" | "pg_advisory_xact_lock_shared" => {
            advisory(args, name.ends_with("_shared"), context, prior_effects)
        }
        _ => Err(error("unsupported.session_effect", name)),
    }
}

/// Notifications queue exact invocations; delivery and duplicate folding happen only at commit.
fn notify(args: &[Cell]) -> Result<ScalarEffectStep> {
    let [channel, payload] = args else {
        return Err(error("sql.42883", "pg_notify requires two text arguments"));
    };
    if channel.sql_type() != SqlType::Text || payload.sql_type() != SqlType::Text {
        return Err(error("sql.42883", "pg_notify requires text"));
    }
    let channel = if channel.is_null() {
        String::new()
    } else {
        crate::scalar::output(channel)?
    };
    let payload = if payload.is_null() {
        String::new()
    } else {
        crate::scalar::output(payload)?
    };
    if channel.is_empty() || channel.len() >= 64 || payload.len() >= 8000 {
        return Err(error(
            "sql.22023",
            "Invalid notification channel or payload byte length",
        ));
    }
    Ok(ScalarEffectStep {
        value: void()?,
        effects: vec![EffectEvent::Notification { channel, payload }],
    })
}

/// Strict advisory functions preserve NULL(void); known conflicting owners require a schedule model.
fn advisory(
    args: &[Cell],
    shared: bool,
    context: &SessionEffectContext,
    prior: &[EffectEvent],
) -> Result<ScalarEffectStep> {
    validate_signature(args)?;
    if args.iter().any(Cell::is_null) {
        return Ok(ScalarEffectStep {
            value: Cell::Null(SqlType::Void),
            effects: Vec::new(),
        });
    }
    let request = AdvisoryHeld {
        key: key(args)?,
        owner: context.transaction.clone(),
        shared,
    };
    let held = inherited_ownership(context, prior)?;
    let effects = acquisition_delta(request, &held)?;
    Ok(ScalarEffectStep {
        value: void()?,
        effects,
    })
}

/// A known conflict is not a successful acquisition, while reentrancy adds no duplicate ownership.
fn acquisition_delta(request: AdvisoryHeld, held: &[AdvisoryHeld]) -> Result<Vec<EffectEvent>> {
    if held.iter().any(|held| conflicts(&request, held)) {
        return Err(error(
            "unsupported.advisory_wait",
            "A known conflicting owner requires explicit scheduling/timeout semantics",
        ));
    }
    Ok(if held.contains(&request) {
        Vec::new()
    } else {
        vec![EffectEvent::AdvisoryLock { held: request }]
    })
}

/// Inherited events are branch-owned acquisitions by the same transaction, never external grants.
fn inherited_ownership(
    context: &SessionEffectContext,
    prior: &[EffectEvent],
) -> Result<Vec<AdvisoryHeld>> {
    let mut held = context.held_advisory.clone();
    for event in prior {
        if let EffectEvent::AdvisoryLock { held: acquired } = event {
            if acquired.owner != context.transaction {
                return Err(error(
                    "invalid.advisory_prefix",
                    "Prior acquisition belongs to another transaction",
                ));
            }
            held.push(acquired.clone());
        }
    }
    Ok(held)
}

/// Overload validation occurs before strict NULL handling.
fn validate_signature(args: &[Cell]) -> Result<()> {
    let valid = match args {
        [key] => key.sql_type() == SqlType::Int64,
        [first, second] => {
            first.sql_type() == SqlType::Int32 && second.sql_type() == SqlType::Int32
        }
        _ => false,
    };
    if !valid {
        return Err(error("sql.42883", "Advisory lock signature mismatch"));
    }
    Ok(())
}

/// Preserve signed values and the namespace chosen by overload resolution.
fn key(args: &[Cell]) -> Result<AdvisoryKey> {
    match args {
        [Cell::Int64(key)] => Ok(AdvisoryKey::Int64 { key: *key }),
        [Cell::Int32(first), Cell::Int32(second)] => Ok(AdvisoryKey::Int32Pair {
            first: *first,
            second: *second,
        }),
        _ => Err(error(
            "invalid.advisory_arguments",
            "Expected resolved non-NULL advisory arguments",
        )),
    }
}

/// Shared holders coexist; an owner may reacquire either mode without conflicting with itself.
fn conflicts(request: &AdvisoryHeld, held: &AdvisoryHeld) -> bool {
    request.key == held.key && request.owner != held.owner && !(request.shared && held.shared)
}

/// Non-NULL void has an empty text representation but is not an empty text scalar.
fn void() -> Result<Cell> {
    crate::scalar::input(&SqlType::Void, "")
}

/// Derive commit delivery from pending invocations, preserving first-occurrence order.
pub fn delivered_notifications(events: &[EffectEvent]) -> Vec<EffectEvent> {
    let mut output = Vec::new();
    for event in events {
        if matches!(event, EffectEvent::Notification { .. }) && !output.contains(event) {
            output.push(event.clone());
        }
    }
    output
}

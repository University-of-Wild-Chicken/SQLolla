//! Pure backend signal requests over a closed stable snapshot; this module never signals a process.
use crate::json::error;
use crate::{Cell, EffectEvent, Result, ScalarEffectStep, SqlType};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

/// One observed backend and the actor's independently supplied role privileges.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BackendSignalTarget {
    /// Process identity within the supplied PostgreSQL instance.
    pub pid: i32,
    /// Whether the target is owned by a superuser or has no ordinary role identity.
    pub superuser: bool,
    /// Whether the actor has privileges of the target's role.
    pub actor_has_role: bool,
    /// Explicit controlled-target assumption: an authorized request can be delivered by the OS.
    pub signal_capability: bool,
}

/// External assumptions required before absence or successful signaling becomes deterministic.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BackendSignalContext {
    /// Every potentially targetable PID in the selected server is accounted for.
    pub complete: bool,
    /// No target exits, starts or reuses a PID before its first signal request.
    pub stable_snapshot: bool,
    /// Calling backend identity; self-termination is outside this result protocol.
    pub actor_pid: i32,
    /// Whether the actor currently has superuser privileges.
    pub actor_superuser: bool,
    /// Whether the actor has privileges of pg_signal_backend.
    pub actor_signal_role: bool,
    /// Complete fixed backend inventory, independent of expected query results.
    pub targets: Vec<BackendSignalTarget>,
}

/// Require a closed stable inventory with valid distinct identities before using it as evidence.
pub fn validate_backend_signal_context(context: &BackendSignalContext) -> Result<()> {
    if !context.complete || !context.stable_snapshot {
        return Err(error(
            "unsupported.backend_snapshot",
            "Complete stable backend inventory required",
        ));
    }
    if context.actor_pid <= 0 || context.targets.iter().any(|target| target.pid <= 0) {
        return Err(error(
            "invalid.backend_pid",
            "Supplied backend identities must be positive",
        ));
    }
    if context.targets.len() > 4096 {
        return Err(error(
            "resource.backend_inventory",
            "Backend inventory exceeds capacity",
        ));
    }
    validate_identities(context)
}

/// An identity appears at most once; conflicting snapshots are invalid model inputs.
fn validate_identities(context: &BackendSignalContext) -> Result<()> {
    let ids = context
        .targets
        .iter()
        .map(|target| target.pid)
        .collect::<BTreeSet<_>>();
    if ids.len() != context.targets.len() {
        return Err(error(
            "invalid.backend_inventory",
            "Duplicate backend identity",
        ));
    }
    Ok(())
}

/// Return only a value and invocation delta; a true value means request delivery, not target exit.
pub fn evaluate_backend_signal(
    args: &[Cell],
    context: &BackendSignalContext,
    prior: &[EffectEvent],
) -> Result<ScalarEffectStep> {
    validate_backend_signal_context(context)?;
    signature(args)?;
    if args.iter().any(Cell::is_null) {
        return Ok(ScalarEffectStep {
            value: Cell::Null(SqlType::Bool),
            effects: vec![],
        });
    }
    let (pid, timeout) = arguments(args)?;
    if pid == context.actor_pid {
        return Err(error(
            "unsupported.backend_self_termination",
            "Self-termination closes the result connection",
        ));
    }
    let Some(target) = context.targets.iter().find(|target| target.pid == pid) else {
        return Ok(absent(pid));
    };
    present(target, timeout, context, prior)
}

/// Validate the resolved overload before strict NULL handling.
fn signature(args: &[Cell]) -> Result<()> {
    if args.len() != 2
        || args[0].sql_type() != SqlType::Int32
        || args[1].sql_type() != SqlType::Int64
    {
        return Err(error(
            "sql.42883",
            "pg_terminate_backend requires integer and bigint",
        ));
    }
    Ok(())
}

/// Refuse platform-width timeout conversions before applying PostgreSQL's negative-timeout error.
fn arguments(args: &[Cell]) -> Result<(i32, i32)> {
    let (Cell::Int32(pid), Cell::Int64(timeout)) = (&args[0], &args[1]) else {
        return Err(error(
            "invalid.backend_arguments",
            "Expected resolved non-NULL backend arguments",
        ));
    };
    let timeout = i32::try_from(*timeout).map_err(|_| {
        error(
            "unsupported.backend_timeout_width",
            "Only portable signed32 timeout values are modeled",
        )
    })?;
    if timeout < 0 {
        return Err(error("sql.22003", "\"timeout\" must not be negative"));
    }
    Ok((*pid, timeout))
}

/// Missing PIDs produce a warning even though their Boolean function result is false.
fn absent(pid: i32) -> ScalarEffectStep {
    ScalarEffectStep {
        value: Cell::Bool(false),
        effects: vec![EffectEvent::SqlWarning {
            sqlstate: "01000".into(),
            message: format!("PID {pid} is not a PostgreSQL backend process"),
        }],
    }
}

/// Permission checks precede wait/capability admission, matching PostgreSQL's call order.
fn present(
    target: &BackendSignalTarget,
    timeout: i32,
    context: &BackendSignalContext,
    prior: &[EffectEvent],
) -> Result<ScalarEffectStep> {
    let allowed = (context.actor_superuser || !target.superuser)
        && (context.actor_superuser || context.actor_signal_role || target.actor_has_role);
    if !allowed {
        return Err(error("sql.42501", "permission denied to terminate process"));
    }
    require_schedule(target, timeout, prior)?;
    Ok(ScalarEffectStep {
        value: Cell::Bool(true),
        effects: vec![EffectEvent::BackendTerminationRequested { pid: target.pid }],
    })
}

/// No fixed-snapshot return is guessed for a lifecycle race, wait or absent OS capability.
fn require_schedule(
    target: &BackendSignalTarget,
    timeout: i32,
    prior: &[EffectEvent],
) -> Result<()> {
    if timeout != 0 {
        return Err(error(
            "unsupported.backend_wait",
            "Positive timeout requires an explicit exit schedule",
        ));
    }
    if prior.iter().any(|event| matches!(event, EffectEvent::BackendTerminationRequested { pid } if *pid == target.pid)) {
        return Err(error("unsupported.backend_repeat_schedule", "Repeated target needs an explicit process-lifecycle schedule"));
    }
    if !target.signal_capability {
        return Err(error(
            "unsupported.backend_signal_capability",
            "Signal delivery capability is not supplied",
        ));
    }
    Ok(())
}

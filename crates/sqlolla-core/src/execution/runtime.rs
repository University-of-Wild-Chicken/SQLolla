//! Statement-local deterministic work accounting shared by correlated and ordinary evaluation.
use crate::json::error;
use crate::*;
use std::cell::Cell;
use std::rc::Rc;

/// Maximum charged relational/scalar visits in one complete statement evaluation.
pub const MAX_EXECUTION_STEPS: usize = 100_000;

/// Explicit immutable semantic context plus a local, shared exhaustion counter.
pub(super) struct Runtime<'a> {
    context: &'a ExecutionContext,
    remaining: Rc<Cell<usize>>,
    inherited: Vec<EffectEvent>,
}

impl<'a> Runtime<'a> {
    /// Start a fresh budget; nested queries must reuse this runtime instead of creating another.
    pub(super) fn new(context: &'a ExecutionContext) -> Self {
        Self {
            context,
            remaining: Rc::new(Cell::new(MAX_EXECUTION_STEPS)),
            inherited: Vec::new(),
        }
    }

    /// Fork immutable semantic input before a child runs; only deterministic fuel is shared.
    pub(super) fn fork(&self, delta: &[EffectEvent]) -> Runtime<'a> {
        Runtime {
            context: self.context,
            remaining: self.remaining.clone(),
            inherited: super::rel_effects::merge(&self.inherited, delta),
        }
    }

    /// Read the complete preceding branch trace; operation results must contain only new events.
    pub(super) fn inherited_effects(&self) -> &[EffectEvent] {
        &self.inherited
    }

    /// Refuse the entire analysis when work would exceed capacity; no partial result is published.
    pub(super) fn charge(&self) -> Result<()> {
        let next = self.remaining.get().checked_sub(1).ok_or_else(|| {
            error(
                "resource.execution_steps",
                "Complete execution exceeds100000chargedsteps",
            )
        })?;
        self.remaining.set(next);
        Ok(())
    }
}

impl std::ops::Deref for Runtime<'_> {
    type Target = ExecutionContext;
    /// Read semantic context without granting mutation of supplied session/table/lock evidence.
    fn deref(&self) -> &Self::Target {
        self.context
    }
}

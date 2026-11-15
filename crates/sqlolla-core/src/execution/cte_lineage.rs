//! Branch-local occurrence certificates recover only demonstrably required read-CTE observations.
mod closure;
mod membership;
mod mutation;
#[cfg(test)]
mod tests;
mod trace;
use super::*;
use std::collections::{BTreeMap, BTreeSet};

/// Hidden incoming occurrence identities correspond positionally to the original output rows.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct Trace {
    origins: Vec<String>,
    rows: Vec<Vec<usize>>,
}

/// Each job records only evidence needed for backward demand, not another database snapshot.
#[derive(Clone, Debug, PartialEq, Eq)]
struct Observation {
    rows: Bag,
    affected: u64,
    effects: Vec<EffectEvent>,
    prefix: Vec<EffectEvent>,
    trace: Option<Trace>,
}

/// Unproved consumers are discharged per complete branch; no shared mutable proof state exists.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct Evidence {
    required: BTreeSet<String>,
    proven: BTreeSet<String>,
    observations: BTreeMap<usize, Observation>,
}

impl Evidence {
    /// Structural certificates remain the fast path; runtime evidence addresses only missing demand.
    pub(super) fn new(plan: &WithPlan) -> Result<Self> {
        let demand = cte_consumption::demand(plan)?;
        Ok(Self {
            required: demand.required,
            proven: demand.proven,
            observations: BTreeMap::new(),
        })
    }

    /// Recovery never turns an incomplete error prefix into a successful full-demand certificate.
    pub(super) fn error_admission(&self) -> Result<()> {
        if self.required.is_empty() {
            return Ok(());
        }
        Err(error(
            "unsupported.cte_consumer_error",
            "Uncertified read-CTE demand on an error branch",
        ))
    }

    /// Store successful original observations after private trace columns have been removed.
    pub(super) fn record(
        &mut self,
        index: usize,
        value: &StatementSuccess,
        trace: Option<Trace>,
        prefix: &[EffectEvent],
    ) {
        if self.required.is_empty() {
            return;
        }
        self.observations.insert(
            index,
            Observation {
                rows: value.rows.clone(),
                affected: value.affected,
                effects: value.effects.clone(),
                prefix: prefix.to_vec(),
                trace,
            },
        );
    }

    /// Evaluate a supported query once with hidden identities; effect callbacks are never replayed.
    pub(super) fn evaluate(
        &self,
        plan: &StatementPlan,
        db: &Database,
        context: &Runtime<'_>,
    ) -> Result<Vec<(StatementOutcome, Option<Trace>)>> {
        if !self.required.is_empty() {
            if let StatementPlan::Query(query) = plan {
                if trace::supported(query) {
                    return trace::evaluate(query, db, context);
                }
            }
        }
        Ok(super::evaluate_resolved(plan, db, context)?
            .outcomes
            .into_iter()
            .map(|value| (value, None))
            .collect())
    }

    /// No outcome is exposed until every missing observable producer has a conditional proof.
    pub(super) fn verify(
        &self,
        plan: &WithPlan,
        db: &Database,
        context: &Runtime<'_>,
    ) -> Result<()> {
        if self.required.is_empty() {
            return Ok(());
        }
        closure::verify(self, plan, db, context)
    }
}

/// Recover the exact job observation; absence is a compiler invariant failure, never empty evidence.
fn observed(evidence: &Evidence, index: usize) -> Result<&Observation> {
    evidence
        .observations
        .get(&index)
        .ok_or_else(|| error("invalid.cte_lineage", "Missing job observation"))
}

/// Initialization ownership is idempotent; ordered event occurrences cannot be erased as duplicates.
fn no_novel_events(value: &Observation, context: &Runtime<'_>) -> bool {
    value.effects.iter().all(|event| {
        matches!(event, EffectEvent::TableLock { .. })
            && context.inherited_effects().contains(event)
    })
}

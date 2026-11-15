//! A changed target certifies a source occurrence only when that match is unique by identity.
use super::*;

/// Immutable mutation inputs retain the original branch prefix during pure lineage checks.
struct Consumer<'a> {
    target: &'a MutationTarget,
    source: &'a Plan,
    predicate: &'a BoundExpr,
    observed: &'a Observation,
    db: &'a Database,
}

/// Only successful UPDATE/DELETE with explicit pure sources have a forced-match profile.
pub(super) fn coverage(
    plan: &StatementPlan,
    observed: &Observation,
    db: &Database,
    context: &Runtime<'_>,
) -> Result<BTreeMap<String, BTreeSet<usize>>> {
    let (target, source, predicate) = match plan {
        StatementPlan::Update(plan) => (&plan.target, plan.source.as_ref(), &plan.predicate),
        StatementPlan::Delete(plan) => (&plan.target, plan.source.as_ref(), &plan.predicate),
        _ => return Ok(BTreeMap::new()),
    };
    let Some(source) = source else {
        return membership::coverage(target, predicate, observed, db, context);
    };
    Consumer {
        target,
        source,
        predicate,
        observed,
        db,
    }
    .coverage(context)
}

impl Consumer<'_> {
    /// Every lineage alternative must force the same occurrences, not one favorable trace.
    fn coverage(&self, context: &Runtime<'_>) -> Result<BTreeMap<String, BTreeSet<usize>>> {
        if !self.supported()? {
            return Ok(BTreeMap::new());
        }
        let traced = lock_lineage::augment(self.source, self.db, context)?;
        let mut common = None;
        for value in relational::query(&traced.plan, self.db, context)? {
            let value = value.map_err(|_| {
                error(
                    "unsupported.cte_consumer_error",
                    "Source lineage has an error branch",
                )
            })?;
            rel_effects::require_pure(&value)?;
            intersect(
                &mut common,
                self.matches(&traced.origins, &value.rows, context)?,
            );
        }
        Ok(common.unwrap_or_default())
    }

    /// Pure scalar callbacks inspect the original prefix without hiding another query demand.
    fn supported(&self) -> Result<bool> {
        Ok(trace::supported(self.source)
            && !lock_demand::ordered(self.source)?
            && !lock_demand::contains(self.source)?
            && !scalar_effects::effectful(self.predicate)
            && trace::scalar_supported(self.predicate))
    }

    /// Targets require one matching source identity, even for duplicate-valued source rows.
    fn matches(
        &self,
        origins: &[String],
        rows: &Bag,
        context: &Runtime<'_>,
    ) -> Result<BTreeMap<String, BTreeSet<usize>>> {
        let mut selected = Vec::new();
        for old in super::super::mutation::original(self.target, self.db)? {
            let found = self.matching(old, rows, context)?;
            if found.len() > 1 {
                return Ok(BTreeMap::new());
            }
            selected.extend(found);
        }
        if selected.len() as u64 != self.observed.affected {
            return Err(error(
                "invalid.cte_lineage",
                "Changed-target count differs from original execution",
            ));
        }
        Ok(trace::coverage(&Trace {
            origins: origins.to_vec(),
            rows: selected,
        }))
    }

    /// Test source occurrences under the same target row and original inherited context.
    fn matching(&self, old: &Row, rows: &Bag, context: &Runtime<'_>) -> Result<Vec<Vec<usize>>> {
        let width = self.source.schema.len();
        let mut found = Vec::new();
        for row in rows {
            context.charge()?;
            let scope = old.iter().chain(&row[..width]).cloned().collect();
            if !super::super::mutation::selected(self.predicate, &scope, self.db, context)? {
                continue;
            }
            found.push(
                row[width..]
                    .iter()
                    .map(trace::ordinal)
                    .collect::<Result<Vec<_>>>()?,
            );
            if found.len() > 1 {
                break;
            }
        }
        Ok(found)
    }
}

/// Intersect provenance across complete alternatives without merging distinct row identities.
pub(super) fn intersect(
    common: &mut Option<BTreeMap<String, BTreeSet<usize>>>,
    next: BTreeMap<String, BTreeSet<usize>>,
) {
    let Some(prior) = common else {
        *common = Some(next);
        return;
    };
    for (table, ids) in prior.iter_mut() {
        if let Some(other) = next.get(table) {
            ids.retain(|id| other.contains(id));
        } else {
            ids.clear();
        }
    }
}

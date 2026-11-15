//! Necessary positive membership contributes a source identity only through one TRUE SQL comparison.
use super::*;

/// A pure consumer reuses its original predicate, snapshot and prefix; only provenance is replayed.
struct Member<'a> {
    target: &'a MutationTarget,
    predicate: &'a BoundExpr,
    query: &'a Plan,
    left: &'a BoundExpr,
    observed: &'a Observation,
    db: &'a Database,
}

/// Optional or negated membership cannot prove that one particular producer occurrence was needed.
pub(super) fn coverage(
    target: &MutationTarget,
    predicate: &BoundExpr,
    observed: &Observation,
    db: &Database,
    context: &Runtime<'_>,
) -> Result<BTreeMap<String, BTreeSet<usize>>> {
    let Some((query, left)) = necessary(predicate) else {
        return Ok(BTreeMap::new());
    };
    let member = Member {
        target,
        predicate,
        query,
        left,
        observed,
        db,
    };
    if !member.supported()? {
        return Ok(BTreeMap::new());
    }
    member.coverage(context)
}

/// Only direct IN and necessary AND conjuncts propagate the unique positive witness obligation.
fn necessary(expression: &BoundExpr) -> Option<(&Plan, &BoundExpr)> {
    match &expression.node {
        ExprNode::Subquery {
            kind: SubqueryKind::In,
            query,
            left: Some(left),
            op: Some(BinaryOp::Eq),
        } if trace::scalar_supported(left) => Some((query, left)),
        ExprNode::Binary {
            op: BinaryOp::And,
            left,
            right,
        } => match (necessary(left), necessary(right)) {
            (Some(member), None) if trace::scalar_supported(right) => Some(member),
            (None, Some(member)) if trace::scalar_supported(left) => Some(member),
            _ => None,
        },
        _ => None,
    }
}

impl Member<'_> {
    /// Correlation, scalar effects and partial relational stages require another consumption proof.
    fn supported(&self) -> Result<bool> {
        Ok(self.query.schema.len() == 1
            && trace::supported(self.query)
            && !lock_demand::ordered(self.query)?
            && !lock_demand::contains(self.query)?
            && !scalar_effects::effectful(self.predicate))
    }

    /// Every possible pure query trace must certify the same necessary producer identities.
    fn coverage(&self, context: &Runtime<'_>) -> Result<BTreeMap<String, BTreeSet<usize>>> {
        let traced = lock_lineage::augment(self.query, self.db, context)?;
        let mut common = None;
        for outcome in relational::query(&traced.plan, self.db, context)? {
            let value = outcome.map_err(|_| {
                error(
                    "unsupported.cte_consumer_error",
                    "Membership lineage has an error branch",
                )
            })?;
            rel_effects::require_pure(&value)?;
            mutation::intersect(
                &mut common,
                self.witnesses(&traced.origins, &value.rows, context)?,
            );
        }
        Ok(common.unwrap_or_default())
    }

    /// Changed target rows require a unique TRUE match, never merely an affected-count coincidence.
    fn witnesses(
        &self,
        origins: &[String],
        rows: &Bag,
        context: &Runtime<'_>,
    ) -> Result<BTreeMap<String, BTreeSet<usize>>> {
        let mut selected = Vec::new();
        for old in super::super::mutation::original(self.target, self.db)? {
            if !super::super::mutation::selected(self.predicate, old, self.db, context)? {
                continue;
            }
            let matches = self.matches(old, rows, context)?;
            if matches.len() != 1 {
                return Ok(BTreeMap::new());
            }
            selected.extend(matches);
        }
        if selected.len() as u64 != self.observed.affected {
            return Err(error(
                "invalid.cte_lineage",
                "Membership target count differs from original execution",
            ));
        }
        Ok(trace::coverage(&Trace {
            origins: origins.to_vec(),
            rows: selected,
        }))
    }

    /// SQL equality handles numeric representation and UNKNOWN; transport equality is never used.
    fn matches(&self, old: &Row, rows: &Bag, context: &Runtime<'_>) -> Result<Vec<Vec<usize>>> {
        let left = scalar(self.left, old, self.db, context)?;
        let mut found = Vec::new();
        for row in rows {
            context.charge()?;
            if subquery::scalar_compare(BinaryOp::Eq, left.clone(), row[0].clone())?
                != Cell::Bool(true)
            {
                continue;
            }
            found.push(
                row[1..]
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

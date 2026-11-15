//! FROM/USING preserves target occurrence counts and refuses arbitrary source representatives.
use super::*;

/// UPDATE alternatives share the same original target snapshot.
pub(super) fn update(
    plan: &UpdatePlan,
    db: &Database,
    context: &Runtime<'_>,
) -> Result<Vec<StatementOutcome>> {
    let action = JoinedAction {
        target: &plan.target,
        predicate: &plan.predicate,
        returning: &plan.returning,
        assignments: Some(&plan.assignments),
    };
    action.execute(plan.source.as_ref().unwrap(), db, context)
}

/// DELETE removes each matching target once, independently of USING multiplicity.
pub(super) fn delete(
    plan: &DeletePlan,
    db: &Database,
    context: &Runtime<'_>,
) -> Result<Vec<StatementOutcome>> {
    let action = JoinedAction {
        target: &plan.target,
        predicate: &plan.predicate,
        returning: &plan.returning,
        assignments: None,
    };
    action.execute(plan.source.as_ref().unwrap(), db, context)
}

/// Shared joined-action inputs keep target identity separate from the expression row scope.
struct JoinedAction<'a> {
    target: &'a MutationTarget,
    predicate: &'a BoundExpr,
    returning: &'a Returning,
    assignments: Option<&'a [Assignment]>,
}

impl JoinedAction<'_> {
    /// Preserve every source alternative and classify only explicit SQL rejections as outcomes.
    fn execute(
        &self,
        source: &Plan,
        db: &Database,
        context: &Runtime<'_>,
    ) -> Result<Vec<StatementOutcome>> {
        if super::lock_demand::contains(source)? {
            return Err(error(
                "unsupported.mutation_lock_demand",
                "Joined mutation needs demand-driven nested lock evaluation",
            ));
        }
        let mut outputs = Vec::new();
        for outcome in relational::query(source, db, context)? {
            let value = match outcome {
                Err(_) => {
                    return Err(error(
                        "unsupported.mutation_source_error_order",
                        "Joined mutation demand may avoid an eager source expression error",
                    ));
                }
                Ok(value) => {
                    super::rel_effects::require_pure(&value)?;
                    mutation::classify(self.apply(&value.rows, db, context))?
                }
            };
            push(&mut outputs, value, context)?;
        }
        Ok(outputs)
    }

    /// Buffer all row changes and RETURNING projections until poststate constraints pass.
    fn apply(
        &self,
        source: &Bag,
        db: &Database,
        context: &Runtime<'_>,
    ) -> Result<StatementOutcome> {
        let before = mutation::original(self.target, db)?;
        let mut candidate = before.clone();
        let mut removed = Vec::new();
        let mut output = Vec::new();
        let mut affected = 0;
        for (index, old) in before.iter().enumerate() {
            if let Some(scope) = self.matching(old, source, db, context)? {
                self.change(index, &scope, &mut candidate, &mut removed, db, context)?;
                let returned = self.returned(index, &scope, &candidate);
                mutation::returning(self.returning, &returned, &mut output, db, context)?;
                affected += 1;
            }
        }
        for index in removed.into_iter().rev() {
            candidate.remove(index);
        }
        let tag = if self.assignments.is_some() {
            "UPDATE"
        } else {
            "DELETE"
        };
        mutation::finish(self.target, candidate, output, affected, tag, db, context)
    }

    /// Distinct source scopes remain uncovered rather than choosing the first physical join row.
    fn matching(
        &self,
        old: &Row,
        source: &Bag,
        db: &Database,
        context: &Runtime<'_>,
    ) -> Result<Option<Row>> {
        let mut chosen = None;
        for row in source {
            let scope = old.iter().chain(row).cloned().collect();
            if !mutation::selected(self.predicate, &scope, db, context)? {
                continue;
            }
            if chosen.as_ref().is_some_and(|value| value != &scope) {
                return Err(error(
                    "unsupported.mutation_source_choices",
                    "Distinct matching source rows require complete update/returning alternatives",
                ));
            }
            chosen = Some(scope);
        }
        Ok(chosen)
    }

    /// UPDATE assignments are simultaneous; DELETE records one physical target ordinal.
    fn change(
        &self,
        index: usize,
        scope: &Row,
        candidate: &mut Bag,
        removed: &mut Vec<usize>,
        db: &Database,
        context: &Runtime<'_>,
    ) -> Result<()> {
        let Some(assignments) = self.assignments else {
            removed.push(index);
            return Ok(());
        };
        let mut next = candidate[index].clone();
        for assignment in assignments {
            let value = scalar(&assignment.expression, scope, db, context)?;
            next[assignment.column] =
                scalar::assignment_cast(&value, &self.target.schema[assignment.column].ty)?;
        }
        mutation::check_row(&next, &self.target.schema)?;
        candidate[index] = next;
        let metadata = crate::context::mutation_metadata(self.target, context)?;
        crate::constraints::immediate_unique(candidate, metadata, &self.target.schema, context)
    }

    /// RETURNING sees updated target columns followed by the unchanged chosen FROM scope.
    fn returned(&self, index: usize, scope: &Row, candidate: &Bag) -> Row {
        if self.assignments.is_none() {
            return scope.clone();
        }
        candidate[index]
            .iter()
            .chain(scope.iter().skip(self.target.schema.len()))
            .cloned()
            .collect()
    }
}

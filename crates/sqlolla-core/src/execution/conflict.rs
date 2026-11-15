//! Unique-conflict resolution preserves proposed-row checks, EXCLUDED scope and affected identity.
use super::index_arbiter::{infer as arbiters, Arbiter};
use super::*;
use std::collections::BTreeSet;

/// Inference must succeed even when the INSERT source happens to contain no rows.
pub(super) fn validate(
    plan: &InsertPlan,
    metadata: &TableSemantics,
    context: &ExecutionContext,
) -> Result<()> {
    if let Some(rule) = &plan.conflict {
        arbiters(rule, metadata, &plan.target.schema, context)?;
    }
    Ok(())
}

/// One candidate owns occurrence identity and its branch-local physical event journal.
pub(super) struct Candidate<'a> {
    pub(super) rows: &'a mut Bag,
    pub(super) touched: &'a mut BTreeSet<usize>,
    pub(super) effects: &'a mut Vec<EffectEvent>,
}

/// Apply one proposal or return a skipped occurrence without increasing affected counts.
pub(super) fn apply(
    plan: &InsertPlan,
    proposed: Row,
    candidate: &mut Candidate<'_>,
    db: &Database,
    context: &Runtime<'_>,
) -> Result<Option<(usize, Row)>> {
    let Some(rule) = &plan.conflict else {
        return insert(plan, proposed, candidate, db, context);
    };
    let metadata = checked_proposal(plan, &proposed, context)?;
    let keys = arbiters(rule, metadata, &plan.target.schema, context)?;
    let matched = conflict_index(&proposed, candidate.rows, &keys, &rule.action, context)?;
    let Some(index) = matched else {
        return insert(plan, proposed, candidate, db, context);
    };
    resolve(plan, rule, (index, proposed), candidate, db, context)
}

/// Fresh insertion records its stable appended slot before user-visible canonical sorting.
fn insert(
    plan: &InsertPlan,
    proposed: Row,
    candidate: &mut Candidate<'_>,
    db: &Database,
    context: &Runtime<'_>,
) -> Result<Option<(usize, Row)>> {
    let index = candidate.rows.len();
    tuple_effects::record(
        &plan.target,
        index,
        TupleAction::Insert,
        &proposed,
        candidate.effects,
        db,
        context,
    )?;
    candidate.touched.insert(index);
    candidate.rows.push(proposed.clone());
    Ok(Some((index, proposed)))
}

/// DO NOTHING has the same observation whichever conflicting unique key is encountered first.
fn conflict_index(
    proposed: &Row,
    rows: &Bag,
    keys: &[Arbiter<'_>],
    action: &ConflictActionPlan,
    context: &ExecutionContext,
) -> Result<Option<usize>> {
    find_conflict(
        proposed,
        rows,
        keys,
        matches!(action, ConflictActionPlan::Nothing),
        context,
    )
}

/// NOT NULL and CHECK validation precedes unique-conflict skipping.
fn checked_proposal<'a>(
    plan: &InsertPlan,
    row: &Row,
    context: &'a Runtime<'_>,
) -> Result<&'a TableSemantics> {
    let metadata = crate::context::mutation_metadata(&plan.target, context)?;
    crate::constraints::checks(&vec![row.clone()], metadata, &plan.target.schema, context)?;
    Ok(metadata)
}

/// The conflict rule selects skip or update without changing the proposed row's identity.
fn resolve(
    plan: &InsertPlan,
    rule: &ConflictPlan,
    proposal: (usize, Row),
    candidate: &mut Candidate<'_>,
    db: &Database,
    context: &Runtime<'_>,
) -> Result<Option<(usize, Row)>> {
    match &rule.action {
        ConflictActionPlan::Nothing => Ok(None),
        ConflictActionPlan::Update {
            assignments,
            predicate,
        } => {
            let update = ConflictUpdate {
                plan,
                assignments,
                predicate,
                db,
                context,
            };
            update.apply(proposal.0, proposal.1, candidate)
        }
    }
}

/// Shared immutable action context avoids mixing row identity with assignment evaluation.
struct ConflictUpdate<'a, 'b> {
    plan: &'a InsertPlan,
    assignments: &'a [Assignment],
    predicate: &'a BoundExpr,
    db: &'a Database,
    context: &'a Runtime<'b>,
}

/// Inferred keys must identify one occurrence; ambiguous physical targets remain unmodeled.
fn find_conflict(
    proposed: &Row,
    rows: &Bag,
    keys: &[Arbiter<'_>],
    skip: bool,
    context: &ExecutionContext,
) -> Result<Option<usize>> {
    let mut found = None;
    for (index, row) in rows.iter().enumerate() {
        for key in keys {
            if arbiter_equal(proposed, row, key, context)? {
                if skip {
                    return Ok(Some(index));
                }
                if found.is_some_and(|old| old != index) {
                    return Err(error(
                        "unsupported.conflict_arbiter_order",
                        "Different arbiters select different target occurrences",
                    ));
                }
                found = Some(index);
            }
        }
    }
    Ok(found)
}

/// Both candidate and existing row must satisfy a partial index before their keys can conflict.
fn arbiter_equal(
    left: &Row,
    right: &Row,
    key: &Arbiter<'_>,
    context: &ExecutionContext,
) -> Result<bool> {
    if !crate::unique_index::includes(key.predicate.as_ref(), left, context)?
        || !crate::unique_index::includes(key.predicate.as_ref(), right, context)?
    {
        return Ok(false);
    }
    key_equal(left, right, key.key)
}

/// Default uniqueness excludes NULL-bearing keys; NULLS NOT DISTINCT uses SQL duplicate equality.
fn key_equal(left: &Row, right: &Row, key: &KeyConstraint) -> Result<bool> {
    for index in &key.columns {
        let a = left
            .get(*index)
            .ok_or_else(|| error("invalid.constraint_columns", "Conflict column outside row"))?;
        let b = right
            .get(*index)
            .ok_or_else(|| error("invalid.constraint_columns", "Conflict column outside row"))?;
        if !key.nulls_not_distinct && (a.is_null() || b.is_null()) {
            return Ok(false);
        }
        if !crate::scalar::duplicate_eq(a, b)? {
            return Ok(false);
        }
    }
    Ok(true)
}

impl ConflictUpdate<'_, '_> {
    /// PostgreSQL rejects a previously affected occurrence before evaluating the update WHERE.
    fn apply(
        &self,
        index: usize,
        proposed: Row,
        candidate: &mut Candidate<'_>,
    ) -> Result<Option<(usize, Row)>> {
        if candidate.touched.contains(&index) {
            return Err(error("sql.21000", "ON CONFLICT cannot affect a row twice"));
        }
        let old = &candidate.rows[index];
        tuple_effects::record(
            &self.plan.target,
            index,
            TupleAction::ConflictLock,
            old,
            candidate.effects,
            self.db,
            self.context,
        )?;
        let scope = old.iter().chain(&proposed).cloned().collect();
        if scalar(self.predicate, &scope, self.db, self.context)? != Cell::Bool(true) {
            return Ok(None);
        }
        let row = self.assigned(old, &scope)?;
        self.publish(index, row, candidate)
    }

    /// A successful assignment produces a new physical version at the same candidate identity.
    fn publish(
        &self,
        index: usize,
        row: Row,
        candidate: &mut Candidate<'_>,
    ) -> Result<Option<(usize, Row)>> {
        tuple_effects::record(
            &self.plan.target,
            index,
            TupleAction::Update,
            &row,
            candidate.effects,
            self.db,
            self.context,
        )?;
        candidate.touched.insert(index);
        candidate.rows[index] = row.clone();
        Ok(Some((index, row)))
    }

    /// Simultaneous assignments all read the same old/excluded scope.
    fn assigned(&self, old: &Row, scope: &Row) -> Result<Row> {
        let mut row = old.clone();
        for assignment in self.assignments {
            let column = self
                .plan
                .target
                .schema
                .get(assignment.column)
                .ok_or_else(|| error("invalid.target_column", "Conflict target outside schema"))?;
            let value = scalar(&assignment.expression, scope, self.db, self.context)?;
            row[assignment.column] = crate::scalar::assignment_cast(&value, &column.ty)?;
        }
        super::mutation::check_row(&row, &self.plan.target.schema)?;
        Ok(row)
    }
}

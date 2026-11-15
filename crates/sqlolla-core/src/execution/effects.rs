//! Statement-level fixed-snapshot locks with complete result and held-lock observations.
use super::*;

/// Expose lock acquisition as a relational branch without losing ownership observations.
pub(super) fn relation(
    plan: &LockedQueryPlan,
    db: &Database,
    context: &Runtime<'_>,
) -> Result<Vec<RelationOutcome>> {
    Ok(execute(plan, db, context)?
        .outcomes
        .into_iter()
        .map(|outcome| match outcome {
            StatementOutcome::SqlError(failure) => Err(Box::new(failure)),
            StatementOutcome::Succeeded(value) => Ok(RelationResult {
                rows: value.rows,
                ordering: value.ordering,
                effects: value.effects,
            }),
        })
        .collect())
}

/// Evaluate a single-origin query and preserve every admissible lock/pagination outcome.
pub(super) fn execute(
    plan: &LockedQueryPlan,
    db: &Database,
    context: &Runtime<'_>,
) -> Result<OutcomeSet> {
    let locks = context.locks.as_ref().ok_or_else(|| {
        error(
            "unsupported.lock_context",
            "Locking needs an explicit fixed ownership snapshot",
        )
    })?;
    if !plan.joined_tables.is_empty() {
        return super::lock_join::execute(plan, db, context, locks);
    }
    if plan.limit == Some(0) {
        return Ok(OutcomeSet {
            outcomes: vec![acquire(
                plan,
                &Vec::new(),
                source_order(&plan.source),
                db,
                locks,
            )?],
        });
    }
    execute_source(plan, db, context, locks)
}

/// Evaluate every row identity and permitted source ordering after the LIMIT-zero boundary.
fn execute_source(
    plan: &LockedQueryPlan,
    db: &Database,
    context: &Runtime<'_>,
    locks: &LockContext,
) -> Result<OutcomeSet> {
    let source = super::lock_source::augment(&plan.source, &plan.table, db, context)?;
    let mut outcomes = Vec::new();
    for result in super::relational::query(&source, db, context)? {
        match result {
            Err(failure) => push(&mut outcomes, source_failure(plan, *failure)?, context)?,
            Ok(value) => {
                super::rel_effects::require_pure(&value)?;
                for rows in super::lock_orders::candidates(&value, plan, locks, context)? {
                    let outcome = acquire(plan, &rows, value.ordering, db, locks)?;
                    push(&mut outcomes, outcome, context)?;
                }
            }
        }
    }
    Ok(OutcomeSet { outcomes })
}

/// Eager source errors cannot establish whether a streaming locked page would reach that row.
fn source_failure(plan: &LockedQueryPlan, failure: SqlFailure) -> Result<StatementOutcome> {
    if plan.limit.is_some() || plan.offset > 0 {
        return Err(error(
            "unsupported.pagination_error_order",
            "Lock pagination needs streaming error alternatives",
        ));
    }
    Ok(StatementOutcome::SqlError(failure))
}

/// SQL NOWAIT failures survive as outcomes; unsupported waits stay outer coverage errors.
fn acquire(
    plan: &LockedQueryPlan,
    rows: &Bag,
    ordering: ResultOrdering,
    db: &Database,
    locks: &LockContext,
) -> Result<StatementOutcome> {
    let ordinals = rows
        .iter()
        .map(super::lock_orders::ordinal)
        .collect::<Result<Vec<_>>>()?;
    match select_locked_rows(&ordinals, plan, locks) {
        Ok(selected) => succeeded(plan, rows, ordering, db, selected, locks),
        Err(failure) => match execution_error(failure)? {
            Err(sql) => Ok(StatementOutcome::SqlError(*sql)),
            Ok(_) => Err(error(
                "invalid.lock_error",
                "Lock failure unexpectedly became success",
            )),
        },
    }
}

/// Return only selected visible cells while retaining OFFSET rows in held-lock evidence.
fn succeeded(
    plan: &LockedQueryPlan,
    source: &Bag,
    ordering: ResultOrdering,
    db: &Database,
    selected: LockSelection,
    locks: &LockContext,
) -> Result<StatementOutcome> {
    let mut rows = visible_rows(source, &selected.returned)?;
    if ordering == ResultOrdering::Bag {
        rows.sort();
    }
    let effects = lock_effects(plan, selected.held, locks);
    Ok(StatementOutcome::Succeeded(StatementSuccess {
        catalog: None,
        database: db.clone(),
        affected: rows.len() as u64,
        command_tag: format!("SELECT {}", rows.len()),
        rows,
        ordering,
        effects,
    }))
}

/// Report relation and touched-row ownership independently of the visible result projection.
fn lock_effects(plan: &LockedQueryPlan, held: Vec<usize>, locks: &LockContext) -> Vec<EffectEvent> {
    let mut effects = vec![EffectEvent::TableLock {
        table: plan.table.clone(),
        mode: "row_share".into(),
    }];
    let held: std::collections::BTreeSet<_> = held.into_iter().collect();
    effects.extend(held.into_iter().map(|occurrence| EffectEvent::RowLock {
        table: plan.table.clone(),
        occurrence,
        strength: held_strength(plan, occurrence, locks),
    }));
    effects
}

/// Existing stronger ownership is never downgraded by a weaker request in the same transaction.
fn held_strength(plan: &LockedQueryPlan, occurrence: usize, locks: &LockContext) -> RowLockMode {
    locks
        .held_rows
        .iter()
        .filter(|held| {
            held.table == plan.table
                && held.occurrence == occurrence
                && held.owner == locks.transaction
        })
        .map(|held| held.strength)
        .fold(plan.clause.strength, std::cmp::max)
}

/// LIMIT zero retains declared presentation ordering without executing source expressions.
fn source_order(plan: &Plan) -> ResultOrdering {
    match &plan.node {
        RelNode::Order { .. } => ResultOrdering::Ordered,
        RelNode::Project { input, .. } | RelNode::Filter { input, .. } => source_order(input),
        _ => ResultOrdering::Bag,
    }
}

/// Private lineage cells cannot leak into the SQL-visible row or change duplicate counts.
fn visible_rows(source: &Bag, selected: &[usize]) -> Result<Bag> {
    let mut rows = Vec::new();
    for ordinal in selected {
        let row = source
            .iter()
            .find(|row| super::lock_orders::ordinal(row).ok() == Some(*ordinal))
            .ok_or_else(|| error("invalid.lock_identity", "Selected occurrence is absent"))?;
        rows.push(row[..row.len() - 1].to_vec());
    }
    Ok(rows)
}

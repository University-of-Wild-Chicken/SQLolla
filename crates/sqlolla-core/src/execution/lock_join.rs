//! Complete unpaginated inner-join locks under a fixed nonconflicting ownership context.
use super::*;
use std::collections::BTreeSet;

/// Acquire every referenced base identity while preserving duplicate join result occurrences.
pub(super) fn execute(
    plan: &LockedQueryPlan,
    db: &Database,
    context: &Runtime<'_>,
    locks: &LockContext,
) -> Result<OutcomeSet> {
    let tables: Vec<_> = std::iter::once(plan.table.clone())
        .chain(plan.joined_tables.clone())
        .collect();
    validate(plan, &tables, locks)?;
    if plan.limit == Some(0) {
        let value = RelationResult {
            rows: vec![],
            ordering: ordering(&plan.source),
            effects: Vec::new(),
        };
        return Ok(OutcomeSet {
            outcomes: vec![success(plan, db, value, &tables, locks)?],
        });
    }
    source_outcomes(plan, db, context, locks, &tables)
}

/// Evaluate the traced source once and preserve every complete relational alternative.
fn source_outcomes(
    plan: &LockedQueryPlan,
    db: &Database,
    context: &Runtime<'_>,
    locks: &LockContext,
    tables: &[String],
) -> Result<OutcomeSet> {
    let traced = super::lock_lineage::augment(&plan.source, db, context)?;
    if traced.origins != tables {
        return Err(error(
            "invalid.lock_origins",
            "Bound and evaluated join origins differ",
        ));
    }
    let mut outcomes = Vec::new();
    for value in super::relational::query(&traced.plan, db, context)? {
        let outcome = match value {
            Err(failure) => StatementOutcome::SqlError(*failure),
            Ok(value) => success(plan, db, value, tables, locks)?,
        };
        push(&mut outcomes, outcome, context)?;
    }
    Ok(OutcomeSet { outcomes })
}

/// Partial acquisition on a conflicting joined row needs a future row-mark schedule model.
fn validate(plan: &LockedQueryPlan, tables: &[String], locks: &LockContext) -> Result<()> {
    for table in tables {
        crate::effects::validate_lock_context(locks, table)?;
    }
    if plan.offset > 0 || plan.limit.is_some_and(|n| n > 0) {
        return Err(error(
            "unsupported.lock_join_pagination",
            "Joined lock prefixes need partial-acquisition alternatives",
        ));
    }
    if plan.limit != Some(0)
        && locks.held_rows.iter().any(|held| {
            tables.contains(&held.table)
                && held.owner != locks.transaction
                && row_locks_conflict(plan.clause.strength, held.strength)
        })
    {
        return Err(error(
            "unsupported.lock_join_conflict",
            "Joined conflict acquisition needs ordered row-mark effects",
        ));
    }
    Ok(())
}

/// A result occurrence carries all base identities; duplicate ownership is represented once.
fn success(
    plan: &LockedQueryPlan,
    db: &Database,
    value: RelationResult,
    tables: &[String],
    locks: &LockContext,
) -> Result<StatementOutcome> {
    rel_effects::require_pure(&value)?;
    let mut held = BTreeSet::new();
    let mut rows = Vec::new();
    for row in value.rows {
        collect(&row, plan.schema.len(), tables, &mut held)?;
        rows.push(row[..plan.schema.len()].to_vec());
    }
    if value.ordering == ResultOrdering::Bag {
        rows.sort();
    }
    Ok(StatementOutcome::Succeeded(StatementSuccess {
        catalog: None,
        database: db.clone(),
        affected: rows.len() as u64,
        command_tag: format!("SELECT {}", rows.len()),
        rows,
        ordering: value.ordering,
        effects: ownership(tables, held, plan.clause.strength, locks),
    }))
}

/// Represent final touched ownership independently of duplicate visible join rows.
fn ownership(
    tables: &[String],
    held: BTreeSet<(String, usize)>,
    mode: RowLockMode,
    locks: &LockContext,
) -> Vec<EffectEvent> {
    let relations = tables.iter().map(|table| EffectEvent::TableLock {
        table: table.clone(),
        mode: "row_share".into(),
    });
    relations
        .chain(
            held.into_iter()
                .map(|(table, occurrence)| EffectEvent::RowLock {
                    strength: strength(&table, occurrence, mode, locks),
                    table,
                    occurrence,
                }),
        )
        .collect()
}

/// Reject any malformed private identity rather than assigning a visible row a guessed origin.
fn collect(
    row: &Row,
    width: usize,
    tables: &[String],
    held: &mut BTreeSet<(String, usize)>,
) -> Result<()> {
    if row.len() != width + tables.len() {
        return Err(error(
            "invalid.lock_identity",
            "Joined lineage width differs",
        ));
    }
    for (table, cell) in tables.iter().zip(&row[width..]) {
        let Cell::Int64(value) = cell else {
            return Err(error(
                "invalid.lock_identity",
                "Joined identity is not an ordinal",
            ));
        };
        let ordinal = usize::try_from(*value)
            .map_err(|_| error("invalid.lock_identity", "Negative joined ordinal"))?;
        held.insert((table.clone(), ordinal));
    }
    Ok(())
}

/// Reentrant weaker requests preserve an already held stronger mode on each physical origin.
fn strength(
    table: &str,
    occurrence: usize,
    requested: RowLockMode,
    locks: &LockContext,
) -> RowLockMode {
    locks
        .held_rows
        .iter()
        .filter(|held| {
            held.table == table && held.occurrence == occurrence && held.owner == locks.transaction
        })
        .map(|held| held.strength)
        .fold(requested, std::cmp::max)
}

/// LIMIT zero observes declared presentation ordering without evaluating child expressions.
fn ordering(plan: &Plan) -> ResultOrdering {
    match &plan.node {
        RelNode::Order { .. } => ResultOrdering::Ordered,
        RelNode::Project { input, .. } | RelNode::Filter { input, .. } => ordering(input),
        _ => ResultOrdering::Bag,
    }
}

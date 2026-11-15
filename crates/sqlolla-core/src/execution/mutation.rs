//! Atomic single-table candidates and buffered RETURNING observations.
use super::*;

/// Execute a typed mutation and preserve SQL rejection separately from engine failure.
pub(super) fn execute(
    plan: &StatementPlan,
    db: &Database,
    context: &Runtime<'_>,
) -> Result<OutcomeSet> {
    if context.tuples.is_some() && !matches!(plan, StatementPlan::Insert(_)) {
        return Err(error(
            "unsupported.tuple_mutation",
            "Tuple journal currently admits INSERT and conflict updates",
        ));
    }
    let target = match plan {
        StatementPlan::Insert(plan) => &plan.target,
        StatementPlan::Update(plan) => &plan.target,
        StatementPlan::Delete(plan) => &plan.target,
        _ => return Err(error("invalid.effect", "Expected mutation")),
    };
    crate::context::mutation_metadata(target, context)?;
    referential_admission(plan, target, context)?;
    let attempts = attempts(plan, db, context);
    match attempts {
        Ok(outcomes) => Ok(OutcomeSet { outcomes }),
        Err(failure) => match execution_error(failure)? {
            Err(failure) => Ok(OutcomeSet {
                outcomes: vec![StatementOutcome::SqlError(*failure)],
            }),
            Ok(_) => Err(error("invalid.effect_error", "Expected SQL failure")),
        },
    }
}

/// Effectful source joins use the same SQL-error channel as ordinary single-table writes.
fn attempts(
    plan: &StatementPlan,
    db: &Database,
    context: &Runtime<'_>,
) -> Result<Vec<StatementOutcome>> {
    match plan {
        StatementPlan::Insert(plan) => super::insert_effects::execute(plan, db, context),
        StatementPlan::Update(plan) if plan.source.is_some() => {
            super::joined_mutation::update(plan, db, context)
        }
        StatementPlan::Delete(plan) if plan.source.is_some() => {
            super::joined_mutation::delete(plan, db, context)
        }
        StatementPlan::Update(plan) => update(plan, db, context).map(|value| vec![value]),
        StatementPlan::Delete(plan) => delete(plan, db, context).map(|value| vec![value]),
        _ => Err(error("invalid.effect", "Expected mutation")),
    }
}

/// Read a complete target snapshot before computing any candidate modification.
pub(super) fn original<'a>(target: &MutationTarget, db: &'a Database) -> Result<&'a Bag> {
    let rows = db
        .get(&target.table)
        .ok_or_else(|| error("invalid.database", "Missing mutation target"))?;
    crate::evaluate::validate(rows, &target.schema)?;
    Ok(rows)
}

/// SQL predicates select only TRUE; typed FALSE and UNKNOWN retain the old occurrence.
pub(super) fn selected(
    predicate: &BoundExpr,
    row: &Row,
    db: &Database,
    context: &Runtime<'_>,
) -> Result<bool> {
    match scalar(predicate, row, db, context)? {
        Cell::Bool(value) => Ok(value),
        Cell::Null(SqlType::Bool) => Ok(false),
        _ => Err(error(
            "invalid.predicate_type",
            "Expected Boolean mutation predicate",
        )),
    }
}

/// Validate a fully computed row, distinguishing NOT NULL from an engine type mismatch.
pub(super) fn check_row(row: &Row, schema: &[Column]) -> Result<()> {
    for (value, column) in row.iter().zip(schema) {
        if value.is_null() && !column.nullable {
            return Err(crate::constraints::violation(
                "23502",
                None,
                Some(&column.name),
            ));
        }
    }
    crate::evaluate::validate(&vec![row.clone()], schema)
}

/// Buffer projected rows until the entire mutation succeeds.
pub(super) fn returning(
    output: &Returning,
    row: &Row,
    rows: &mut Bag,
    db: &Database,
    context: &Runtime<'_>,
) -> Result<()> {
    if !output.expressions.is_empty() {
        let projected = output
            .expressions
            .iter()
            .map(|expr| scalar(expr, row, db, context))
            .collect::<Result<Row>>()?;
        crate::evaluate::validate(&vec![projected.clone()], &output.schema)?;
        rows.push(projected);
    }
    Ok(())
}

/// Publish only the target table and retain every nontarget table exactly.
pub(super) fn finish(
    target: &MutationTarget,
    mut candidate: Bag,
    mut rows: Bag,
    affected: usize,
    tag: &str,
    db: &Database,
    context: &Runtime<'_>,
) -> Result<StatementOutcome> {
    candidate.sort();
    rows.sort();
    let mut database = db.clone();
    database.insert(target.table.clone(), candidate);
    let metadata = crate::context::mutation_metadata(target, context)?;
    crate::constraints::checks(&database[&target.table], metadata, &target.schema, context)?;
    crate::unique_index::validate(&database[&target.table], metadata, &target.schema, context)?;
    crate::constraints::database(&database, context)?;
    Ok(StatementOutcome::Succeeded(StatementSuccess {
        catalog: None,
        database,
        rows,
        ordering: ResultOrdering::Bag,
        affected: affected as u64,
        command_tag: format!("{tag} {affected}"),
        effects: Vec::new(),
    }))
}

/// Refuse unmodeled cascading actions before evaluating a parent-table write.
fn referential_admission(
    plan: &StatementPlan,
    target: &MutationTarget,
    context: &Runtime<'_>,
) -> Result<()> {
    for metadata in context.table_metadata.values() {
        for key in &metadata.foreign_keys {
            if key.referenced_table != target.table {
                continue;
            }
            let action = match plan {
                StatementPlan::Update(_) => key.on_update,
                StatementPlan::Delete(_) => key.on_delete,
                _ => continue,
            };
            if !matches!(
                action,
                ReferentialAction::NoAction | ReferentialAction::Restrict
            ) {
                return Err(error(
                    "unsupported.referential_action",
                    "Cascading parent writes require explicit action semantics",
                ));
            }
        }
    }
    Ok(())
}

/// Keep an error from one source alternative without discarding other source alternatives.
pub(super) fn classify(value: Result<StatementOutcome>) -> Result<StatementOutcome> {
    match value {
        Ok(value) => Ok(value),
        Err(failure) => match execution_error(failure)? {
            Err(failure) => Ok(StatementOutcome::SqlError(*failure)),
            Ok(_) => Err(error("invalid.effect_error", "Expected SQL failure")),
        },
    }
}

/// UPDATE applies all selected assignments to one old row before checking the candidate.
fn update(plan: &UpdatePlan, db: &Database, context: &Runtime<'_>) -> Result<StatementOutcome> {
    let before = original(&plan.target, db)?;
    check_rows(before.len(), context)?;
    let mut candidate = before.clone();
    let mut output = Vec::new();
    let mut affected = 0;
    for (index, old) in before.iter().enumerate() {
        if selected(&plan.predicate, old, db, context)? {
            let next = update_row(plan, old, db, context)?;
            returning(&plan.returning, &next, &mut output, db, context)?;
            affected += 1;
            candidate[index] = next;
            immediate_target(&candidate, &plan.target, context)?;
        }
    }
    finish(
        &plan.target,
        candidate,
        output,
        affected,
        "UPDATE",
        db,
        context,
    )
}

/// Immediate key checks use the exact mutation target's physical column schema.
fn immediate_target(rows: &Bag, target: &MutationTarget, context: &Runtime<'_>) -> Result<()> {
    let metadata = crate::context::mutation_metadata(target, context)?;
    crate::constraints::immediate_unique(rows, metadata, &target.schema, context)
}

/// Simultaneous assignments all read the old row, while checked writes populate a fresh row.
fn update_row(plan: &UpdatePlan, old: &Row, db: &Database, context: &Runtime<'_>) -> Result<Row> {
    let mut next = old.clone();
    for assignment in &plan.assignments {
        let column = plan
            .target
            .schema
            .get(assignment.column)
            .ok_or_else(|| error("invalid.target_column", "UPDATE target outside schema"))?;
        let value = scalar(&assignment.expression, old, db, context)?;
        next[assignment.column] = scalar::assignment_cast(&value, &column.ty)?;
    }
    check_row(&next, &plan.target.schema)?;
    Ok(next)
}

/// DELETE returns old selected values and frames every unselected occurrence.
fn delete(plan: &DeletePlan, db: &Database, context: &Runtime<'_>) -> Result<StatementOutcome> {
    let before = original(&plan.target, db)?;
    check_rows(before.len(), context)?;
    let mut candidate = Vec::new();
    let mut output = Vec::new();
    let mut affected = 0;
    for old in before {
        if selected(&plan.predicate, old, db, context)? {
            returning(&plan.returning, old, &mut output, db, context)?;
            affected += 1;
        } else {
            candidate.push(old.clone());
        }
    }
    finish(
        &plan.target,
        candidate,
        output,
        affected,
        "DELETE",
        db,
        context,
    )
}

//! Writable CTEs publish disjoint deltas atomically while every base read retains the original state.
use super::*;

/// Pending writes never replace the base snapshot used by later CTE expressions.
#[derive(Clone, Debug, PartialEq, Eq)]
struct Frame {
    reads: Database,
    final_state: Database,
    effects: Vec<EffectEvent>,
    presentation: Option<StatementSuccess>,
    evidence: cte_lineage::Evidence,
}

/// The scheduled position retains its statement and optional virtual relation identity together.
struct Job<'a> {
    index: usize,
    binding: Option<&'a CteStatement>,
    statement: &'a StatementPlan,
}

/// Execute every demanded binding with one shared budget and preserve all child alternatives.
pub(super) fn execute(plan: &WithPlan, db: &Database, context: &Runtime<'_>) -> Result<OutcomeSet> {
    constraint_admission(plan, context)?;
    let schedules = match super::cte_schedule::orders(plan, context) {
        Ok(schedules) => schedules,
        Err(failure) => return schedule_failure(failure),
    };
    let evidence = cte_lineage::Evidence::new(plan)?;
    let mut output = Vec::new();
    for order in schedules {
        for outcome in scheduled(plan, &order, db, &evidence, context)? {
            push(&mut output, outcome, context)?;
        }
    }
    Ok(OutcomeSet { outcomes: output })
}

/// A default-planning rejection is a zero-effect SQL outcome; coverage/resource errors still refuse.
fn schedule_failure(failure: Error) -> Result<OutcomeSet> {
    match execution_error(failure)? {
        Err(failure) => Ok(OutcomeSet {
            outcomes: vec![StatementOutcome::SqlError(*failure)],
        }),
        Ok(_) => Err(error(
            "invalid.cte_schedule_error",
            "Expected a classified SQL failure",
        )),
    }
}

/// One schedule owns its table frame, event prefix and final presentation independently.
fn scheduled(
    plan: &WithPlan,
    order: &[usize],
    db: &Database,
    evidence: &cte_lineage::Evidence,
    context: &Runtime<'_>,
) -> Result<Vec<StatementOutcome>> {
    let mut frames = vec![Frame {
        reads: db.clone(),
        final_state: db.clone(),
        effects: vec![],
        presentation: None,
        evidence: evidence.clone(),
    }];
    let mut output = Vec::new();
    for index in order {
        frames = step(plan, *index, frames, &mut output, context)?;
    }
    for frame in frames {
        push(&mut output, publish(plan, frame, context)?, context)?;
    }
    Ok(output)
}

/// Cross-target foreign-key visibility requires end-of-command trigger semantics beyond snapshots.
fn constraint_admission(plan: &WithPlan, context: &Runtime<'_>) -> Result<()> {
    let writes = crate::cte::dependencies(&StatementPlan::With(plan.clone()))?.writes;
    for table in &writes {
        if let Some(metadata) = context.table_metadata.get(table) {
            if metadata
                .foreign_keys
                .iter()
                .any(|key| writes.contains(&key.referenced_table))
            {
                return Err(error(
                    "unsupported.cte_constraint_interaction",
                    "Multiple CTE writes share foreign-key trigger dependencies",
                ));
            }
        }
    }
    Ok(())
}

/// A SQL-rejected branch has no published transactional table delta.
fn step(
    plan: &WithPlan,
    index: usize,
    frames: Vec<Frame>,
    failures: &mut Vec<StatementOutcome>,
    context: &Runtime<'_>,
) -> Result<Vec<Frame>> {
    let binding = plan.bindings.get(index);
    let statement = binding.map_or(plan.body.as_ref(), |binding| binding.statement.as_ref());
    let job = Job {
        index,
        binding,
        statement,
    };
    let mut next = Vec::new();
    for frame in frames {
        context.charge()?;
        let child = context.fork(&frame.effects);
        for (outcome, trace) in frame.evidence.evaluate(statement, &frame.reads, &child)? {
            if let Some(frame) = record_outcome(&job, outcome, trace, &frame, failures, context)? {
                push(&mut next, frame, context)?;
            }
        }
    }
    Ok(next)
}

/// Failed jobs terminate their schedule while successful jobs retain a separate pending table frame.
fn record_outcome(
    job: &Job<'_>,
    outcome: StatementOutcome,
    trace: Option<cte_lineage::Trace>,
    frame: &Frame,
    failures: &mut Vec<StatementOutcome>,
    context: &Runtime<'_>,
) -> Result<Option<Frame>> {
    match outcome {
        StatementOutcome::SqlError(failure) => {
            frame.evidence.error_admission()?;
            if job.binding.is_some() {
                read_error_admission(job.statement)?;
            }
            let outcome =
                rel_effects::statement(StatementOutcome::SqlError(failure), &frame.effects);
            push(failures, outcome, context)?;
            Ok(None)
        }
        StatementOutcome::Succeeded(value) => record_job(job, value, trace, frame).map(Some),
    }
}

/// Record a body observation without publishing tables before the remaining scheduled jobs finish.
fn record_job(
    job: &Job<'_>,
    mut value: StatementSuccess,
    trace: Option<cte_lineage::Trace>,
    frame: &Frame,
) -> Result<Frame> {
    let mut next = frame.clone();
    next.evidence
        .record(job.index, &value, trace, &frame.effects);
    if let Some(binding) = job.binding {
        return record(binding, value, &next);
    }
    merge_writes(job.statement, &value.database, &mut next.final_state)?;
    next.effects = super::rel_effects::merge(&next.effects, &value.effects);
    value.effects.clear();
    next.presentation = Some(value);
    Ok(next)
}

/// Eager evaluation of a read CTE cannot establish an error that its consumer might avoid.
fn read_error_admission(plan: &StatementPlan) -> Result<()> {
    if matches!(plan, StatementPlan::Query(_)) {
        return Err(error(
            "unsupported.cte_error_order",
            "Read CTE demand may avoid eager expression errors",
        ));
    }
    Ok(())
}

/// Only the RETURNING relation is visible to subsequent lexical bindings.
fn record(binding: &CteStatement, value: StatementSuccess, frame: &Frame) -> Result<Frame> {
    let mut next = frame.clone();
    next.reads
        .insert(binding.table_id.clone(), value.rows.clone());
    merge_writes(&binding.statement, &value.database, &mut next.final_state)?;
    next.effects = super::rel_effects::merge(&next.effects, &value.effects);
    Ok(next)
}

/// The final statement supplies presentation observations; pending writes supply atomic poststate.
fn publish(plan: &WithPlan, frame: Frame, context: &Runtime<'_>) -> Result<StatementOutcome> {
    frame.evidence.verify(plan, &frame.reads, context)?;
    let mut value = frame
        .presentation
        .ok_or_else(|| error("invalid.cte_schedule", "Final body was not executed"))?;
    crate::constraints::database(&frame.final_state, context)?;
    value.database = frame.final_state;
    value.effects = frame.effects;
    Ok(StatementOutcome::Succeeded(value))
}

/// Compiler-reserved virtual relations are never copied into externally visible table state.
fn merge_writes(plan: &StatementPlan, source: &Database, target: &mut Database) -> Result<()> {
    for table in crate::cte::dependencies(plan)?.writes {
        let rows = source
            .get(&table)
            .ok_or_else(|| error("invalid.cte_delta", "Missing writable CTE target"))?;
        target.insert(table, rows.clone());
    }
    Ok(())
}

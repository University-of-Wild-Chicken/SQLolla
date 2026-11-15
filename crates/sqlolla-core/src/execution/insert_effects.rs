//! Per-proposal insertion retains branch-local allocations through constraints and statement rollback.
use super::*;
use std::collections::BTreeSet;

/// One unpublished INSERT candidate owns its rows, output, touched identities and event delta.
#[derive(Clone, PartialEq, Eq)]
struct Frame {
    candidate: Bag,
    output: Bag,
    affected: usize,
    touched: BTreeSet<usize>,
    effects: Vec<EffectEvent>,
}

/// Literal VALUES is consumed one proposal at a time; relational sources expose complete alternatives.
pub(super) fn execute(
    plan: &InsertPlan,
    db: &Database,
    context: &Runtime<'_>,
) -> Result<Vec<StatementOutcome>> {
    let metadata = crate::context::mutation_metadata(&plan.target, context)?;
    conflict::validate(plan, metadata, context)?;
    insert_defaults::admit(plan, context)?;
    tuple_effects::admit(plan, db, context)?;
    match &plan.source.node {
        RelNode::Values { rows } => run(plan, &literal_rows(rows), &[], db, context),
        RelNode::EvaluateValues { rows } => run(plan, rows, &[], db, context),
        _ => relational_sources(plan, db, context),
    }
}

/// Already evaluated input cells retain positional identity without executing source expressions again.
fn literal_rows(rows: &Bag) -> Vec<Vec<BoundExpr>> {
    rows.iter()
        .map(|row| {
            row.iter()
                .cloned()
                .map(crate::expression::literal)
                .collect()
        })
        .collect()
}

/// Pure unordered sources enumerate proposal order; effectful streams need a represented consumed prefix.
fn relational_sources(
    plan: &InsertPlan,
    db: &Database,
    context: &Runtime<'_>,
) -> Result<Vec<StatementOutcome>> {
    let mut output = Vec::new();
    for source in relational::query(&plan.source, db, context)? {
        let value = match source {
            Err(failure) => {
                push(&mut output, StatementOutcome::SqlError(*failure), context)?;
                continue;
            }
            Ok(value) => value,
        };
        admit_source(&value)?;
        let orders = if value.ordering == ResultOrdering::Bag {
            ordering::permutations(value.rows, context)?
        } else {
            vec![value.rows]
        };
        for rows in orders {
            for outcome in run(plan, &literal_rows(&rows), &value.effects, db, context)? {
                push(&mut output, outcome, context)?;
            }
        }
    }
    Ok(output)
}

/// Eager source materialization cannot pretend to observe an effectful stream's failing prefix.
fn admit_source(value: &RelationResult) -> Result<()> {
    let ordered_events = value.effects.iter().any(|event| {
        !matches!(
            event,
            EffectEvent::TableLock { .. } | EffectEvent::RowLock { .. }
        )
    });
    if value.rows.len() > 1 && ordered_events {
        return Err(error(
            "unsupported.insert_stream_effects",
            "Effectful multirow sources require consumed-prefix execution",
        ));
    }
    Ok(())
}

/// Stop failed branches immediately while allowing other complete alternatives to continue.
fn run(
    plan: &InsertPlan,
    proposals: &[Vec<BoundExpr>],
    effects: &[EffectEvent],
    db: &Database,
    context: &Runtime<'_>,
) -> Result<Vec<StatementOutcome>> {
    let before = mutation::original(&plan.target, db)?.clone();
    let mut frames = vec![Frame {
        candidate: before,
        output: vec![],
        affected: 0,
        touched: BTreeSet::new(),
        effects: effects.to_vec(),
    }];
    let mut output = Vec::new();
    for (ordinal, proposal) in proposals.iter().enumerate() {
        frames = advance(plan, proposal, ordinal, frames, db, context, &mut output)?;
    }
    for frame in frames {
        push(&mut output, finish(plan, frame, db, context)?, context)?;
    }
    Ok(output)
}

/// Publication checks the complete candidate while preserving allocations if final checks fail.
fn finish(
    plan: &InsertPlan,
    frame: Frame,
    db: &Database,
    context: &Runtime<'_>,
) -> Result<StatementOutcome> {
    let runtime = context.fork(&frame.effects);
    let finished = mutation::finish(
        &plan.target,
        frame.candidate,
        frame.output,
        frame.affected,
        "INSERT 0",
        db,
        &runtime,
    );
    Ok(rel_effects::statement(
        mutation::classify(finished)?,
        &frame.effects,
    ))
}

/// Evaluate defaults against the preceding branch state before checking and applying this proposal.
fn advance(
    plan: &InsertPlan,
    source: &[BoundExpr],
    ordinal: usize,
    frames: Vec<Frame>,
    db: &Database,
    context: &Runtime<'_>,
    failures: &mut Vec<StatementOutcome>,
) -> Result<Vec<Frame>> {
    let mut next = Vec::new();
    for frame in frames {
        let runtime = context.fork(&frame.effects);
        let expressions = insert_defaults::expressions(plan, source, ordinal, &runtime)?;
        for value in rows_effects::values(&[expressions], db, &runtime)? {
            let value = match value {
                Err(failure) => {
                    record_failure(failures, *failure, &frame.effects, context)?;
                    continue;
                }
                Ok(value) => value,
            };
            if let Some(branch) = proposal(plan, &frame, value, db, context, failures)? {
                push(&mut next, branch, context)?;
            }
        }
    }
    Ok(next)
}

/// Apply one fully evaluated proposal and route its failure with the complete branch-local delta.
fn proposal(
    plan: &InsertPlan,
    prior: &Frame,
    value: RelationResult,
    db: &Database,
    context: &Runtime<'_>,
    failures: &mut Vec<StatementOutcome>,
) -> Result<Option<Frame>> {
    let mut branch = prior.clone();
    branch.effects = rel_effects::merge(&branch.effects, &value.effects);
    let [row] = value.rows.as_slice() else {
        return Err(error("invalid.insert_tuple", "Expected one proposal row"));
    };
    if let Err(failure) = write(plan, row.clone(), &mut branch, db, context) {
        let outcome = mutation::classify(Err(failure))?;
        push(
            failures,
            rel_effects::statement(outcome, &branch.effects),
            context,
        )?;
        return Ok(None);
    }
    Ok(Some(branch))
}

/// Proposed-row constraints precede arbiter conflict resolution and publication of RETURNING.
fn write(
    plan: &InsertPlan,
    row: Row,
    frame: &mut Frame,
    db: &Database,
    context: &Runtime<'_>,
) -> Result<()> {
    mutation::check_row(&row, &plan.target.schema)?;
    let metadata = crate::context::mutation_metadata(&plan.target, context)?;
    crate::constraints::checks(&vec![row.clone()], metadata, &plan.target.schema, context)?;
    let result = conflict::apply(
        plan,
        row,
        &mut conflict::Candidate {
            rows: &mut frame.candidate,
            touched: &mut frame.touched,
            effects: &mut frame.effects,
        },
        db,
        context,
    )?;
    check_rows(frame.candidate.len(), context)?;
    crate::constraints::immediate_unique(&frame.candidate, metadata, &plan.target.schema, context)?;
    if let Some((index, row)) = result {
        output(plan, index, &row, frame, db, context)?;
    }
    Ok(())
}

/// RETURNING reads the new row version only after immediate row/index checks have succeeded.
fn output(
    plan: &InsertPlan,
    index: usize,
    row: &Row,
    frame: &mut Frame,
    db: &Database,
    context: &Runtime<'_>,
) -> Result<()> {
    let scope = tuple_effects::returning_row(
        &plan.returning,
        row,
        &plan.target.table,
        index,
        &frame.effects,
        context,
    )?;
    let runtime = context.fork(&frame.effects);
    mutation::returning(&plan.returning, &scope, &mut frame.output, db, &runtime)?;
    frame.affected += 1;
    Ok(())
}

/// A failed scalar proposal retains allocations already made by earlier proposals in this branch.
fn record_failure(
    output: &mut Vec<StatementOutcome>,
    failure: SqlFailure,
    prior: &[EffectEvent],
    context: &Runtime<'_>,
) -> Result<()> {
    push(
        output,
        rel_effects::statement(StatementOutcome::SqlError(failure), prior),
        context,
    )
}

//! Projection and qualification retain every branch's explicit event delta across row occurrences.
use super::*;

/// Partial row output and events belong to one candidate execution order only.
#[derive(Clone, PartialEq, Eq)]
struct Frame {
    rows: Bag,
    effects: Vec<EffectEvent>,
}

/// A tuple expression list retains local effects independently of its preceding row prefix.
#[derive(Clone, PartialEq, Eq)]
struct Tuple {
    values: Row,
    effects: Vec<EffectEvent>,
}

/// Effectful unordered inputs enumerate each occurrence order before evaluating row expressions.
pub(super) fn project(
    value: RelationResult,
    expressions: &[BoundExpr],
    db: &Database,
    context: &Runtime<'_>,
) -> Result<Vec<RelationOutcome>> {
    let mut output = Vec::new();
    for rows in orders(&value, expressions, context)? {
        let mut frames = vec![Frame {
            rows: vec![],
            effects: vec![],
        }];
        for row in rows {
            frames = advance(frames, expressions, &row, db, context, &mut output)?;
        }
        publish(frames, value.ordering, &mut output, context)?;
    }
    Ok(output)
}

/// VALUES evaluates its explicitly supplied expression rows without reusing another branch's state.
pub(super) fn values(
    rows: &[Vec<BoundExpr>],
    db: &Database,
    context: &Runtime<'_>,
) -> Result<Vec<RelationOutcome>> {
    let mut frames = vec![Frame {
        rows: vec![],
        effects: vec![],
    }];
    let mut output = Vec::new();
    for expressions in rows {
        frames = advance(frames, expressions, &vec![], db, context, &mut output)?;
    }
    publish(frames, ResultOrdering::Bag, &mut output, context)?;
    Ok(output)
}

/// Multiple value alternatives are expanded before committing the next projected occurrence.
fn advance(
    frames: Vec<Frame>,
    expressions: &[BoundExpr],
    row: &Row,
    db: &Database,
    context: &Runtime<'_>,
    errors: &mut Vec<RelationOutcome>,
) -> Result<Vec<Frame>> {
    let mut output = Vec::new();
    for frame in frames {
        let child = context.fork(&frame.effects);
        for tuple in tuples(expressions, row, db, &child)? {
            match tuple {
                Err(failure) => push(
                    errors,
                    rel_effects::inherit(Err(failure), &frame.effects),
                    context,
                )?,
                Ok(tuple) => {
                    let mut rows = frame.rows.clone();
                    rows.push(tuple.values);
                    check_rows(rows.len(), context)?;
                    let effects = rel_effects::merge(&frame.effects, &tuple.effects);
                    push(&mut output, Frame { rows, effects }, context)?;
                }
            }
        }
    }
    Ok(output)
}

/// Projection positions have a declared left-to-right evaluation profile and exact positional output.
fn tuples(
    expressions: &[BoundExpr],
    row: &Row,
    db: &Database,
    context: &Runtime<'_>,
) -> Result<Vec<std::result::Result<Tuple, Box<SqlFailure>>>> {
    let mut output = vec![Ok(Tuple {
        values: vec![],
        effects: vec![],
    })];
    for expression in expressions {
        let mut next = Vec::new();
        for tuple in output {
            for value in tuple_step(tuple, expression, row, db, context)? {
                push(&mut next, value, context)?;
            }
        }
        output = next;
    }
    Ok(output)
}

/// Each output expression sees all earlier projected positions in this one branch.
fn tuple_step(
    tuple: std::result::Result<Tuple, Box<SqlFailure>>,
    expression: &BoundExpr,
    row: &Row,
    db: &Database,
    context: &Runtime<'_>,
) -> Result<Vec<std::result::Result<Tuple, Box<SqlFailure>>>> {
    let Ok(tuple) = tuple else {
        return Ok(vec![tuple]);
    };
    let child = context.fork(&tuple.effects);
    let mut output = Vec::new();
    for value in scalar_observations(expression, row, db, &child)? {
        let value = match value {
            Err(failure) => tuple_failure(failure, &tuple.effects),
            Ok(value) => {
                let mut values = tuple.values.clone();
                values.push(value.value);
                let effects = rel_effects::merge(&tuple.effects, &value.effects);
                Ok(Tuple { values, effects })
            }
        };
        push(&mut output, value, context)?;
    }
    Ok(output)
}

/// Statement failure retains the tuple's nontransactional prefix without publishing partial rows.
fn tuple_failure(
    mut failure: Box<SqlFailure>,
    prior: &[EffectEvent],
) -> std::result::Result<Tuple, Box<SqlFailure>> {
    failure.effects = rel_effects::rollback(&rel_effects::merge(prior, &failure.effects));
    Err(failure)
}

/// SQL bags permit alternate physical input orders when those orders affect external observations.
fn orders(
    value: &RelationResult,
    expressions: &[BoundExpr],
    context: &Runtime<'_>,
) -> Result<Vec<Bag>> {
    if value.ordering == ResultOrdering::Bag && expressions.iter().any(scalar_effects::effectful) {
        return ordering::permutations(value.rows.clone(), context);
    }
    Ok(vec![value.rows.clone()])
}

/// Complete row alternatives retain their local event trace independently of result bag ordering.
fn publish(
    frames: Vec<Frame>,
    ordering: ResultOrdering,
    output: &mut Vec<RelationOutcome>,
    context: &Runtime<'_>,
) -> Result<()> {
    for frame in frames {
        push(
            output,
            rel_effects::inherit(result(frame.rows, ordering), &frame.effects),
            context,
        )?;
    }
    Ok(())
}

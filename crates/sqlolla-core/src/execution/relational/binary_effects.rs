//! UNION alternatives sequence operand deltas before evaluating the next operand.
use super::*;

/// Independent UNION operands preserve both complete evaluation orders, never a shared allocation log.
pub(super) fn evaluate(
    plan: &Plan,
    left: &Plan,
    right: &Plan,
    db: &Database,
    context: &Runtime<'_>,
) -> Result<Vec<RelationOutcome>> {
    if !matches!(
        plan.node,
        RelNode::Set {
            op: SetOp::Union,
            ..
        }
    ) {
        return Err(error(
            "unsupported.binary_effect_order",
            "Only complete UNION operand consumption is modeled",
        ));
    }
    let mut output = Vec::new();
    for reverse in [false, true] {
        for value in ordered(plan, left, right, reverse, db, context)? {
            push(&mut output, value, context)?;
        }
    }
    Ok(output)
}

/// Errors stop this schedule while retaining the preceding operand's nontransactional events.
fn ordered(
    plan: &Plan,
    left: &Plan,
    right: &Plan,
    reverse: bool,
    db: &Database,
    context: &Runtime<'_>,
) -> Result<Vec<RelationOutcome>> {
    let first = if reverse { right } else { left };
    let second = if reverse { left } else { right };
    let mut output = Vec::new();
    for first in query(first, db, context)? {
        let Ok(first) = first else {
            push(&mut output, first, context)?;
            continue;
        };
        let child = context.fork(&first.effects);
        for second in query(second, db, &child)? {
            let value = match second {
                Err(failure) => rel_effects::inherit(Err(failure), &first.effects),
                Ok(second) => {
                    compose(plan, (left, right), (&first, &second), reverse, db, context)?
                }
            };
            push(&mut output, value, context)?;
        }
    }
    Ok(output)
}

/// Logical operand positions remain unchanged even when the event execution order reverses.
fn compose(
    plan: &Plan,
    inputs: (&Plan, &Plan),
    values: (&RelationResult, &RelationResult),
    reverse: bool,
    db: &Database,
    context: &Runtime<'_>,
) -> Result<RelationOutcome> {
    let effects = rel_effects::merge(&values.0.effects, &values.1.effects);
    let (left, right) = if reverse {
        (values.1, values.0)
    } else {
        values
    };
    capacity(&plan.node, left.rows.len(), right.rows.len(), context)?;
    let node = binary_node(
        &plan.node,
        constant(inputs.0, left.rows.clone()),
        constant(inputs.1, right.rows.clone()),
    )?;
    let child = context.fork(&effects);
    Ok(rel_effects::inherit(
        grounded(plan, node, db, ResultOrdering::Bag, &child)?,
        &effects,
    ))
}

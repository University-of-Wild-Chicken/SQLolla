//! Ordinary scalar children compose explicit deltas in the admitted expression evaluation order.
use super::*;

/// A partially grounded expression retains exactly its already evaluated child observations.
#[derive(Clone, PartialEq, Eq)]
struct Frame {
    expression: BoundExpr,
    effects: Vec<EffectEvent>,
}

/// Materialize immediate children before evaluating a pure operator or one explicit effect callback.
pub(super) fn evaluate(
    expr: &BoundExpr,
    row: &Row,
    db: &Database,
    context: &Runtime<'_>,
) -> Result<Vec<ScalarOutcome>> {
    let mut children = Vec::new();
    crate::rewrite::scalar_children(&mut expr.clone(), &mut |child| {
        children.push(child.clone());
        Ok(())
    })?;
    let mut frames = vec![Frame {
        expression: expr.clone(),
        effects: vec![],
    }];
    let mut output = Vec::new();
    for (index, child) in children.iter().enumerate() {
        frames = advance(frames, child, index, row, db, context, &mut output)?;
    }
    for frame in frames {
        let runtime = context.fork(&frame.effects);
        let value = invoke(&frame.expression, row, db, &runtime)?;
        push(&mut output, prepend(value, &frame.effects), context)?;
    }
    Ok(output)
}

/// Explicit session calls consume the inherited branch prefix and return invocation-local events.
fn invoke(
    expr: &BoundExpr,
    row: &Row,
    db: &Database,
    context: &Runtime<'_>,
) -> Result<ScalarOutcome> {
    if let ExprNode::Call { name, args } = &expr.node {
        if crate::is_session_effect(name)
            || sequence_calls::handles(name)
            || name == "pg_terminate_backend"
        {
            let values = args
                .iter()
                .map(|arg| subquery::scalar(arg, row, db, context))
                .collect::<Result<Vec<_>>>()?;
            return match effect_call(name, &values, context) {
                Ok(value) => Ok(Ok(value)),
                Err(failure) => failure_outcome(failure),
            };
        }
    }
    classify(subquery::scalar(expr, row, db, context))
}

/// Dedicated pure transition handlers receive the complete inherited state before invocation.
fn effect_call(name: &str, args: &[Cell], context: &Runtime<'_>) -> Result<ScalarObservation> {
    if sequence_calls::handles(name) {
        return sequence_calls::evaluate(name, args, context);
    }
    if name == "pg_terminate_backend" {
        return backend(args, context);
    }
    let specification = context.session_effects.as_ref().ok_or_else(|| {
        error(
            "unsupported.session_effect_context",
            "Explicit session effect context required",
        )
    })?;
    let value =
        crate::evaluate_session_effect(name, args, specification, context.inherited_effects())?;
    Ok(ScalarObservation {
        value: value.value,
        effects: value.effects,
    })
}

/// A signal request is a retained external observation, never an immediate process-state mutation.
fn backend(args: &[Cell], context: &Runtime<'_>) -> Result<ScalarObservation> {
    let specification = context.backend_signals.as_ref().ok_or_else(|| {
        error(
            "unsupported.backend_context",
            "Explicit backend signal context required",
        )
    })?;
    let value = crate::evaluate_backend_signal(args, specification, context.inherited_effects())?;
    Ok(ScalarObservation {
        value: value.value,
        effects: value.effects,
    })
}

/// Every child receives its own preceding state, never the events from another alternative.
fn advance(
    frames: Vec<Frame>,
    child: &BoundExpr,
    index: usize,
    row: &Row,
    db: &Database,
    context: &Runtime<'_>,
    errors: &mut Vec<ScalarOutcome>,
) -> Result<Vec<Frame>> {
    let mut output = Vec::new();
    for frame in frames {
        let runtime = context.fork(&frame.effects);
        for value in super::evaluate(child, row, db, &runtime)? {
            match value {
                Err(failure) => push(errors, prepend(Err(failure), &frame.effects), context)?,
                Ok(value) => {
                    let mut next = frame.clone();
                    replace(&mut next.expression, index, value.value)?;
                    next.effects = rel_effects::merge(&frame.effects, &value.effects);
                    push(&mut output, next, context)?;
                }
            }
        }
    }
    Ok(output)
}

/// Replacing one immediate child preserves every other typed position and evaluation boundary.
fn replace(expr: &mut BoundExpr, selected: usize, value: Cell) -> Result<()> {
    let mut index = 0;
    crate::rewrite::scalar_children(expr, &mut |child| {
        if index == selected {
            *child = crate::expression::literal(value.clone());
        }
        index += 1;
        Ok(())
    })
}

//! Pure branch composition passes inherited effects before evaluating every reachable child.
mod children;
mod conditional;
mod query;
#[cfg(test)]
mod tests;
use super::*;

/// Evaluate all complete scalar branches while validating each successful output annotation.
pub(super) fn evaluate(
    expr: &BoundExpr,
    row: &Row,
    db: &Database,
    context: &Runtime<'_>,
) -> Result<Vec<ScalarOutcome>> {
    context.charge()?;
    let outcomes = dispatch(expr, row, db, context)?;
    for value in outcomes.iter().filter_map(|value| value.as_ref().ok()) {
        if value.value.sql_type() != expr.ty || (value.value.is_null() && !expr.nullable) {
            return Err(error(
                "invalid.expression_type",
                "Scalar branch annotation mismatch",
            ));
        }
    }
    Ok(outcomes)
}

/// Conditional families decide reachability before ordinary immediate children are evaluated.
fn dispatch(
    expr: &BoundExpr,
    row: &Row,
    db: &Database,
    context: &Runtime<'_>,
) -> Result<Vec<ScalarOutcome>> {
    match &expr.node {
        ExprNode::Case {
            branches,
            otherwise,
        } => conditional::case(branches, otherwise, row, db, context),
        ExprNode::Coalesce(args) => conditional::coalesce(args, &expr.ty, row, db, context),
        ExprNode::Binary {
            op: BinaryOp::And | BinaryOp::Or,
            ..
        } => conditional::boolean(expr, row, db, context),
        ExprNode::Subquery { .. } if effectful(expr) => query::evaluate(expr, row, db, context),
        ExprNode::RowCompare { .. } if effectful(expr) => Err(error(
            "unsupported.row_comparison_effects",
            "Effectful lexicographic operands need explicit demand semantics",
        )),
        ExprNode::RowCompare { .. } | ExprNode::Subquery { .. } => {
            Ok(vec![classify(subquery::scalar(expr, row, db, context))?])
        }
        _ => children::evaluate(expr, row, db, context),
    }
}

/// Legacy consumers may use a scalar only when no semantic event or alternative is discarded.
pub(super) fn pure(
    expr: &BoundExpr,
    row: &Row,
    db: &Database,
    context: &Runtime<'_>,
) -> Result<Cell> {
    let outcomes = evaluate(expr, row, db, context)?;
    let [outcome] = outcomes.as_slice() else {
        return Err(error(
            "unsupported.scalar_alternatives",
            "Consumer requires branch propagation",
        ));
    };
    match outcome {
        Ok(value) if value.effects.is_empty() => Ok(value.value.clone()),
        Ok(_) => Err(error(
            "unsupported.scalar_effect_consumer",
            "Consumer cannot discard effects",
        )),
        Err(failure) => Err(Error {
            code: format!("sql.{}", failure.sqlstate),
            message: "Scalar SQL failure".into(),
            sql: Some(failure.clone()),
        }),
    }
}

/// Promote only classified SQL failures and retain engine/admission refusals in the outer channel.
fn classify(value: Result<Cell>) -> Result<ScalarOutcome> {
    match value {
        Ok(value) => Ok(Ok(ScalarObservation {
            value,
            effects: vec![],
        })),
        Err(failure) => failure_outcome(failure),
    }
}

/// Convert one fallible effect invocation without losing its classified SQL diagnostics.
fn failure_outcome(failure: Error) -> Result<ScalarOutcome> {
    match execution_error(failure)? {
        Err(failure) => Ok(Err(failure)),
        Ok(_) => Err(error("invalid.scalar_error", "Expected SQL failure")),
    }
}

/// Prepend only the current operation's delta; inherited Runtime events remain outside its result.
fn prepend(mut value: ScalarOutcome, prior: &[EffectEvent]) -> ScalarOutcome {
    match &mut value {
        Ok(value) => value.effects = rel_effects::merge(prior, &value.effects),
        Err(failure) => {
            failure.effects = rel_effects::rollback(&rel_effects::merge(prior, &failure.effects))
        }
    }
    value
}

/// Bound and deduplicate complete observations, never just their returned scalar values.
fn append(
    output: &mut Vec<ScalarOutcome>,
    values: Vec<ScalarOutcome>,
    prior: &[EffectEvent],
    context: &Runtime<'_>,
) -> Result<()> {
    for value in values {
        push(output, prepend(value, prior), context)?;
    }
    Ok(())
}

/// Detect known external calls structurally; data values never decide whether effects are modeled.
pub(super) fn effectful(expr: &BoundExpr) -> bool {
    if let ExprNode::Call { name, .. } = &expr.node {
        if crate::function_registry::builtin(name)
            .is_some_and(|spec| spec.effect != crate::function_registry::Effect::None)
        {
            return true;
        }
    }
    let mut found = false;
    if let ExprNode::Subquery { query, .. } = &expr.node {
        let _ = crate::rewrite::relation(&mut (**query).clone(), &mut |child| {
            found |= effectful(child);
            Ok(())
        });
    }
    let _ = crate::rewrite::scalar_children(&mut expr.clone(), &mut |child| {
        found |= effectful(child);
        Ok(())
    });
    found
}

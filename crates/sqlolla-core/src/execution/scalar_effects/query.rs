//! Effect-bearing subqueries preserve deltas only at independently modeled consumption boundaries.
use super::*;

/// Fully consumed arrays and at-most-one-row scalar results expose each complete child observation.
pub(super) fn evaluate(
    expr: &BoundExpr,
    row: &Row,
    db: &Database,
    context: &Runtime<'_>,
) -> Result<Vec<ScalarOutcome>> {
    let ExprNode::Subquery { kind, query, .. } = &expr.node else {
        unreachable!()
    };
    if !matches!(kind, SubqueryKind::Scalar | SubqueryKind::Array) {
        return Err(error(
            "unsupported.subquery_effect_demand",
            "Quantified/existence effects need streaming demand",
        ));
    }
    let mut query = (**query).clone();
    crate::rewrite::relation(&mut query, &mut |value| {
        subquery::specialize(value, row, 0, context)
    })?;
    let mut output = Vec::new();
    for value in relational::query(&query, db, context)? {
        let outcome = decoded(expr, (*kind, &query), value, row, db, context)?;
        push(&mut output, outcome, context)?;
    }
    Ok(output)
}

/// Cardinality rejection cannot publish effects from rows PostgreSQL might never consume.
fn decode(
    expr: &BoundExpr,
    kind: SubqueryKind,
    mut value: RelationResult,
    row: &Row,
    db: &Database,
    context: &Runtime<'_>,
) -> Result<ScalarOutcome> {
    if kind == SubqueryKind::Scalar && value.rows.len() > 1 {
        return Err(error(
            "unsupported.subquery_effect_demand",
            "Multiple scalar rows require a streamed failure prefix",
        ));
    }
    let effects = std::mem::take(&mut value.effects);
    let child = context.fork(&effects);
    Ok(prepend(
        classify(subquery::decode(expr, value, row, db, &child))?,
        &effects,
    ))
}

/// Syntactic single-row input bounds error evaluation without guessing from physical database rows.
fn single_source(plan: &Plan) -> bool {
    match &plan.node {
        RelNode::Values { rows } => rows.len() <= 1,
        RelNode::EvaluateValues { rows } => rows.len() <= 1,
        RelNode::Project { input, .. }
        | RelNode::Filter { input, .. }
        | RelNode::Order { input, .. }
        | RelNode::Unordered { input } => single_source(input),
        _ => false,
    }
}

/// Error propagation is valid only when the query's admitted demand consumes its complete input.
fn decoded(
    expr: &BoundExpr,
    source: (SubqueryKind, &Plan),
    value: RelationOutcome,
    row: &Row,
    db: &Database,
    context: &Runtime<'_>,
) -> Result<ScalarOutcome> {
    match value {
        Err(failure) if source.0 == SubqueryKind::Array || single_source(source.1) => {
            Ok(Err(failure))
        }
        Err(_) => Err(error(
            "unsupported.subquery_effect_demand",
            "Scalar error requires bounded consumption evidence",
        )),
        Ok(value) => decode(expr, source.0, value, row, db, context),
    }
}

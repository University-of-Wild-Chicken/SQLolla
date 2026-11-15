//! A hidden-column instrumented query is the original evaluation, not a second effectful replay.
use super::*;

/// Only the existing checked lineage operators preserve incoming occurrence positions.
pub(super) fn supported(plan: &Plan) -> bool {
    let children = match &plan.node {
        RelNode::Scan { .. } => true,
        RelNode::Project { input, .. }
        | RelNode::Filter { input, .. }
        | RelNode::Order { input, .. }
        | RelNode::Unordered { input } => supported(input),
        RelNode::Product { left, right }
        | RelNode::Join {
            kind: JoinKind::Inner,
            left,
            right,
            ..
        } => supported(left) && supported(right),
        _ => false,
    };
    children && no_subquery(plan)
}

/// Scalar subqueries need scope-aware lineage rather than guessed hidden-column shifts.
pub(super) fn no_subquery(plan: &Plan) -> bool {
    let mut permitted = true;
    let result = crate::rewrite::relation_expressions(&mut plan.clone(), &mut |expression| {
        permitted &= scalar_supported(expression);
        Ok(())
    });
    result.is_ok() && permitted
}

/// Inspect all expression children without invoking callbacks or resolving runtime values.
pub(super) fn scalar_supported(expression: &BoundExpr) -> bool {
    if matches!(
        expression.node,
        ExprNode::Subquery { .. } | ExprNode::OuterColumn { .. }
    ) {
        return false;
    }
    let mut valid = true;
    let _ = crate::rewrite::scalar_children(&mut expression.clone(), &mut |child| {
        valid &= scalar_supported(child);
        Ok(())
    });
    valid
}

/// Execute instrumented callbacks once and separate visible rows from private incoming identities.
pub(super) fn evaluate(
    plan: &Plan,
    db: &Database,
    context: &Runtime<'_>,
) -> Result<Vec<(StatementOutcome, Option<Trace>)>> {
    let traced = lock_lineage::augment(plan, db, context)?;
    let mut output = Vec::new();
    for mut outcome in
        super::super::evaluate_resolved(&StatementPlan::Query(traced.plan), db, context)?.outcomes
    {
        let trace = match &mut outcome {
            StatementOutcome::Succeeded(value) => {
                Some(separate(value, plan.schema.len(), &traced.origins)?)
            }
            StatementOutcome::SqlError(_) => None,
        };
        output.push((outcome, trace));
    }
    Ok(output)
}

/// Hidden identities are validated before truncation so malformed provenance cannot become evidence.
fn separate(value: &mut StatementSuccess, width: usize, origins: &[String]) -> Result<Trace> {
    let mut identities = Vec::new();
    for row in &mut value.rows {
        if row.len() != width + origins.len() {
            return Err(error("invalid.cte_lineage", "Traced result width changed"));
        }
        identities.push(
            row[width..]
                .iter()
                .map(ordinal)
                .collect::<Result<Vec<_>>>()?,
        );
        row.truncate(width);
    }
    Ok(Trace {
        origins: origins.to_vec(),
        rows: identities,
    })
}

/// Every hidden identity is a finite original bag position, never a value-based surrogate.
pub(super) fn ordinal(value: &Cell) -> Result<usize> {
    match value {
        Cell::Int64(value) if *value >= 0 => usize::try_from(*value)
            .map_err(|_| error("resource.cte_lineage", "Occurrence identity overflow")),
        _ => Err(error(
            "invalid.cte_lineage",
            "Expected a nonnegative occurrence identity",
        )),
    }
}

/// Each returned occurrence forces all of its contributing origins; repeated origin slots stay distinct.
pub(super) fn coverage(trace: &Trace) -> BTreeMap<String, BTreeSet<usize>> {
    let mut result = BTreeMap::<String, BTreeSet<usize>>::new();
    for row in &trace.rows {
        for (origin, occurrence) in trace.origins.iter().zip(row) {
            result
                .entry(origin.clone())
                .or_default()
                .insert(*occurrence);
        }
    }
    result
}

//! Grounded relational operators evaluate row expressions with explicit database/runtime context.
use super::*;

/// Children are already materialized by relational lifting; scalar subqueries keep shared fuel.
pub(super) fn eval(plan: &Plan, db: &Database, context: &Runtime<'_>) -> Result<Bag> {
    let rows = match &plan.node {
        RelNode::Filter { input, predicate } => {
            filter(crate::eval(input, db)?, predicate, db, context)?
        }
        RelNode::Project { input, expressions } => {
            project(crate::eval(input, db)?, expressions, db, context)?
        }
        RelNode::EvaluateValues { rows } => values(rows, db, context)?,
        RelNode::Join {
            kind,
            left,
            right,
            on,
        } => join(*kind, left, right, on, db, context)?,
        _ => crate::eval(plan, db)?,
    };
    crate::evaluate::validate(&rows, &plan.schema)?;
    Ok(rows)
}

/// FALSE and UNKNOWN both discard occurrences; predicate errors remain explicit.
fn filter(rows: Bag, predicate: &BoundExpr, db: &Database, context: &Runtime<'_>) -> Result<Bag> {
    let mut output = Vec::new();
    for row in rows {
        if scalar(predicate, &row, db, context)? == Cell::Bool(true) {
            output.push(row);
        }
    }
    Ok(output)
}

/// Projection preserves occurrence count and resolves nested queries for each input row.
fn project(
    rows: Bag,
    expressions: &[BoundExpr],
    db: &Database,
    context: &Runtime<'_>,
) -> Result<Bag> {
    rows.iter()
        .map(|row| {
            expressions
                .iter()
                .map(|expr| scalar(expr, row, db, context))
                .collect()
        })
        .collect()
}

/// VALUES keeps its explicit row boundaries while deferring scalar evaluation until execution.
fn values(rows: &[Vec<BoundExpr>], db: &Database, context: &Runtime<'_>) -> Result<Bag> {
    rows.iter()
        .map(|row| {
            row.iter()
                .map(|expr| scalar(expr, &vec![], db, context))
                .collect()
        })
        .collect()
}

/// Join matching tracks occurrences independently, including duplicate-valued rows.
fn join(
    kind: JoinKind,
    left: &Plan,
    right: &Plan,
    on: &BoundExpr,
    db: &Database,
    context: &Runtime<'_>,
) -> Result<Bag> {
    let lhs = crate::eval(left, db)?;
    let rhs = crate::eval(right, db)?;
    let mut output = Vec::new();
    let mut matched = vec![false; rhs.len()];
    for a in &lhs {
        let mut found = false;
        for (index, b) in rhs.iter().enumerate() {
            let row = a.iter().chain(b).cloned().collect();
            if scalar(on, &row, db, context)? == Cell::Bool(true) {
                output.push(row);
                found = true;
                matched[index] = true;
            }
        }
        if !found && matches!(kind, JoinKind::Left | JoinKind::Full) {
            output.push(a.iter().cloned().chain(nulls(right)).collect());
        }
    }
    extend_right(kind, left, &rhs, &matched, &mut output);
    Ok(output)
}

/// Append each unmatched right occurrence exactly once under right/full preservation.
fn extend_right(kind: JoinKind, left: &Plan, rows: &Bag, matched: &[bool], output: &mut Bag) {
    if matches!(kind, JoinKind::Right | JoinKind::Full) {
        for (index, row) in rows.iter().enumerate() {
            if !matched[index] {
                output.push(nulls(left).into_iter().chain(row.iter().cloned()).collect());
            }
        }
    }
}

/// Outer join padding carries each exact declared SQL family.
fn nulls(plan: &Plan) -> Row {
    plan.schema
        .iter()
        .map(|column| Cell::Null(column.ty.clone()))
        .collect()
}

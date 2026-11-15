//! Relational composition lifts existing bag semantics over every finite alternative.
use super::distinct_on::evaluate as distinct_on;
use super::set_returning::evaluate as expand_sets;
use super::*;
mod binary_effects;

/// Evaluate complete alternatives recursively, preserving SQL errors and ordered rows.
pub(super) fn query(
    plan: &Plan,
    db: &Database,
    context: &Runtime<'_>,
) -> Result<Vec<RelationOutcome>> {
    context.charge()?;
    super::lock_demand::admit(plan)?;
    let outcomes = dispatch(plan, db, context)?;
    for value in outcomes.iter().filter_map(|outcome| outcome.as_ref().ok()) {
        crate::evaluate::validate(&value.rows, &plan.schema)?;
    }
    Ok(outcomes)
}

/// Dispatch preserves each operator's separate consumption and ordering contract.
fn dispatch(plan: &Plan, db: &Database, context: &Runtime<'_>) -> Result<Vec<RelationOutcome>> {
    match &plan.node {
        RelNode::EvaluateValues { rows } => super::rows_effects::values(rows, db, context),
        RelNode::Locked(plan) => super::effects::relation(plan, db, context),
        RelNode::DistinctOn { input, keys } => distinct_on(input, keys, db, context),
        RelNode::ExpandSets { input, arrays } => expand_sets(input, arrays, db, context),
        RelNode::Window { input, spec } => super::window::evaluate(input, spec, db, context),
        RelNode::Aggregate { .. } => grouped(plan, db, context),
        RelNode::Order { input, keys } => super::ordering::order(input, keys, db, context),
        RelNode::Slice {
            input,
            offset,
            limit,
        } => super::ordering::slice(input, *offset, *limit, db, context),
        RelNode::Unordered { input } => unordered(input, db, context),
        RelNode::Filter { input, .. }
        | RelNode::Project { input, .. }
        | RelNode::Distinct { input } => unary(plan, input, db, context),
        RelNode::Product { left, right }
        | RelNode::Join { left, right, .. }
        | RelNode::Set { left, right, .. } => binary(plan, left, right, db, context),
        _ => Ok(vec![native(plan, db, ResultOrdering::Bag, context)?]),
    }
}

/// Destructure the aggregate relation once while retaining its declared result schema.
fn grouped(plan: &Plan, db: &Database, context: &Runtime<'_>) -> Result<Vec<RelationOutcome>> {
    let RelNode::Aggregate {
        input,
        groups,
        aggregates,
    } = &plan.node
    else {
        return Err(error(
            "invalid.aggregate_plan",
            "Expected aggregate relation",
        ));
    };
    super::grouping::evaluate(plan, input, groups, aggregates, db, context)
}

/// Run one grounded legacy operator and retain explicitly typed SQL errors.
fn native(
    plan: &Plan,
    db: &Database,
    ordering: ResultOrdering,
    context: &Runtime<'_>,
) -> Result<RelationOutcome> {
    match super::grounded::eval(plan, db, context) {
        Ok(rows) => {
            check_rows(rows.len(), context)?;
            Ok(result(rows, ordering))
        }
        Err(failure) => execution_error(failure),
    }
}

/// Derived and CTE references expose bags while retaining all selected alternatives.
fn unordered(input: &Plan, db: &Database, context: &Runtime<'_>) -> Result<Vec<RelationOutcome>> {
    let mut outputs = Vec::new();
    for outcome in query(input, db, context)? {
        let value = outcome.map(|mut value| {
            value.rows.sort();
            value.ordering = ResultOrdering::Bag;
            value
        });
        push(&mut outputs, value, context)?;
    }
    Ok(outputs)
}

/// Lift a unary operator over every input alternative without dropping an error branch.
fn unary(
    plan: &Plan,
    input: &Plan,
    db: &Database,
    context: &Runtime<'_>,
) -> Result<Vec<RelationOutcome>> {
    if let RelNode::Project { expressions, .. } = &plan.node {
        return project(input, expressions, db, context);
    }
    let mut outputs = Vec::new();
    for outcome in query(input, db, context)? {
        let value = match outcome {
            Err(failure) => Err(failure),
            Ok(value) => {
                let node = unary_node(&plan.node, constant(input, value.rows))?;
                let order = if matches!(plan.node, RelNode::Distinct { .. }) {
                    ResultOrdering::Bag
                } else {
                    value.ordering
                };
                super::rel_effects::inherit(
                    grounded(plan, node, db, order, context)?,
                    &value.effects,
                )
            }
        };
        push(&mut outputs, value, context)?;
    }
    Ok(outputs)
}

/// A projection's first expression sees upstream events before producing its own local delta.
fn project(
    input: &Plan,
    expressions: &[BoundExpr],
    db: &Database,
    context: &Runtime<'_>,
) -> Result<Vec<RelationOutcome>> {
    let mut output = Vec::new();
    for value in query(input, db, context)? {
        if let Ok(value) = &value {
            projection_order(value, expressions)?;
        }
        let values = super::rel_effects::transform(value, context, |value, child| {
            super::rows_effects::project(value, expressions, db, child)
        })?;
        for value in values {
            push(&mut output, value, context)?;
        }
    }
    Ok(output)
}

/// Eager nested multirow producers cannot substitute for per-row interleaving of ordered effects.
fn projection_order(value: &RelationResult, expressions: &[BoundExpr]) -> Result<()> {
    let ordered = value.effects.iter().any(|event| {
        !matches!(
            event,
            EffectEvent::TableLock { .. }
                | EffectEvent::RowLock { .. }
                | EffectEvent::AdvisoryLock { .. }
        )
    });
    if value.rows.len() > 1 && ordered && expressions.iter().any(super::scalar_effects::effectful) {
        return Err(error(
            "unsupported.projection_effect_interleaving",
            "Competing multirow projections require per-row event scheduling",
        ));
    }
    Ok(())
}

/// Substitute a fully enumerated child with a typed occurrence bag.
fn constant(template: &Plan, rows: Bag) -> Box<Plan> {
    Box::new(Plan {
        schema: template.schema.clone(),
        node: RelNode::Values { rows },
    })
}

/// Reuse the checked legacy operator implementation after removing nondeterministic children.
fn unary_node(node: &RelNode, input: Box<Plan>) -> Result<RelNode> {
    match node {
        RelNode::Filter { predicate, .. } => Ok(RelNode::Filter {
            input,
            predicate: predicate.clone(),
        }),
        RelNode::Project { expressions, .. } => Ok(RelNode::Project {
            input,
            expressions: expressions.clone(),
        }),
        RelNode::Distinct { .. } => Ok(RelNode::Distinct { input }),
        _ => Err(error("invalid.operator", "Expected unary operator")),
    }
}

/// Lift a binary operator over the complete product of child behavior sets.
fn binary(
    plan: &Plan,
    left: &Plan,
    right: &Plan,
    db: &Database,
    context: &Runtime<'_>,
) -> Result<Vec<RelationOutcome>> {
    if super::lock_demand::ordered(left)? || super::lock_demand::ordered(right)? {
        return binary_effects::evaluate(plan, left, right, db, context);
    }
    let a = query(left, db, context)?;
    let mut outputs = Vec::new();
    for lhs in &a {
        let child = match lhs {
            Ok(value) => context.fork(&value.effects),
            Err(_) => context.fork(&[]),
        };
        for rhs in query(right, db, &child)? {
            let value = combine(plan, left, right, lhs, &rhs, db, context)?;
            push(&mut outputs, value, context)?;
        }
    }
    Ok(outputs)
}

/// Refuse excessive candidate capacity before constructing Cartesian products.
fn capacity(node: &RelNode, left: usize, right: usize, context: &Runtime<'_>) -> Result<()> {
    let count = match node {
        RelNode::Product { .. } => left.checked_mul(right),
        RelNode::Join { .. } => left
            .checked_mul(right)
            .and_then(|n| n.checked_add(left + right)),
        _ => left.checked_add(right),
    }
    .ok_or_else(|| error("resource.row_count", "Occurrence capacity overflow"))?;
    check_rows(count, context)
}

/// Materialize one pair without evaluating a failed child as a successful empty relation.
fn combine(
    plan: &Plan,
    left: &Plan,
    right: &Plan,
    lhs: &RelationOutcome,
    rhs: &RelationOutcome,
    db: &Database,
    context: &Runtime<'_>,
) -> Result<RelationOutcome> {
    super::rel_effects::parallel(lhs, rhs)?;
    let (Ok(lhs), Ok(rhs)) = (lhs, rhs) else {
        return Ok(Err(lhs
            .as_ref()
            .err()
            .or(rhs.as_ref().err())
            .unwrap()
            .clone()));
    };
    capacity(&plan.node, lhs.rows.len(), rhs.rows.len(), context)?;
    let node = binary_node(
        &plan.node,
        constant(left, lhs.rows.clone()),
        constant(right, rhs.rows.clone()),
    )?;
    let effects = super::rel_effects::merge(&lhs.effects, &rhs.effects);
    Ok(super::rel_effects::inherit(
        grounded(plan, node, db, ResultOrdering::Bag, context)?,
        &effects,
    ))
}

/// Evaluate a materialized operator while preserving its original output contract.
fn grounded(
    plan: &Plan,
    node: RelNode,
    db: &Database,
    order: ResultOrdering,
    context: &Runtime<'_>,
) -> Result<RelationOutcome> {
    native(
        &Plan {
            schema: plan.schema.clone(),
            node,
        },
        db,
        order,
        context,
    )
}

/// Preserve the exact binary operator and predicate while grounding its children.
fn binary_node(node: &RelNode, left: Box<Plan>, right: Box<Plan>) -> Result<RelNode> {
    match node {
        RelNode::Product { .. } => Ok(RelNode::Product { left, right }),
        RelNode::Join { kind, on, .. } => Ok(RelNode::Join {
            kind: *kind,
            left,
            right,
            on: on.clone(),
        }),
        RelNode::Set { op, all, .. } => Ok(RelNode::Set {
            op: *op,
            all: *all,
            left,
            right,
        }),
        _ => Err(error("invalid.operator", "Expected binary operator")),
    }
}

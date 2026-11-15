//! Predicate-filtered unique enforcement and conservative deterministic index inference.
use crate::json::error;
use crate::*;

/// Bind declared index predicates against physical columns; caller supplies reparsed SQL evidence.
pub(crate) fn predicate(
    index: &UniqueIndex,
    schema: &[Column],
    context: &ExecutionContext,
) -> Result<Option<BoundExpr>> {
    let Some(predicate) = &index.predicate else {
        return Ok(None);
    };
    let parser = predicate.parser.as_ref().ok_or_else(|| {
        error(
            "unsupported.index_binding",
            "Index predicate parser evidence missing",
        )
    })?;
    let expression = bind_metadata_functions(parser, schema, context.functions.as_ref())?;
    if expression.ty != SqlType::Bool {
        return Err(error("sql.42804", "Index predicate must be Boolean"));
    }
    crate::function_registry::require_immutable(&expression, "unsupported.index_immutability")?;
    Ok(Some(expression))
}

/// Evaluate a partial index's TRUE-only membership, preserving expression errors.
pub(crate) fn includes(
    predicate: Option<&BoundExpr>,
    row: &Row,
    context: &ExecutionContext,
) -> Result<bool> {
    match predicate {
        None => Ok(true),
        Some(predicate) => {
            Ok(crate::execution::session::evaluate(predicate, row, context)? == Cell::Bool(true))
        }
    }
}

/// Enforce every declared index on its member occurrences using SQL key and NULL equivalence.
pub(crate) fn validate(
    rows: &Bag,
    metadata: &TableSemantics,
    schema: &[Column],
    context: &ExecutionContext,
) -> Result<()> {
    for index in &metadata.unique_indexes {
        validate_one(rows, index, schema, context)?;
    }
    Ok(())
}

/// Check one ordered physical index without grouping it behind primary/unique constraints.
pub(crate) fn validate_one(
    rows: &Bag,
    index: &UniqueIndex,
    schema: &[Column],
    context: &ExecutionContext,
) -> Result<()> {
    let predicate = predicate(index, schema, context)?;
    let mut members = Vec::new();
    for row in rows {
        if includes(predicate.as_ref(), row, context)? {
            members.push(row.clone());
        }
    }
    crate::constraints::unique(&members, &index.key, false)
}

/// Exact conjunction inclusion proves implication; unresolved general implication remains refused.
pub(crate) fn implied(target: Option<&BoundExpr>, predicate: Option<&BoundExpr>) -> Result<bool> {
    let Some(predicate) = predicate else {
        return Ok(true);
    };
    if predicate.node == ExprNode::Literal(Cell::Bool(true)) {
        return Ok(true);
    }
    let Some(target) = target else {
        return Ok(false);
    };
    let mut antecedent = Vec::new();
    let mut consequent = Vec::new();
    conjuncts(target, &mut antecedent);
    conjuncts(predicate, &mut consequent);
    if consequent.iter().all(|value| antecedent.contains(value)) {
        return Ok(true);
    }
    Err(error(
        "unsupported.index_implication",
        "Partial-index inference requires a separately verified predicate implication",
    ))
}

/// Flatten only SQL AND; OR, NULL-sensitive comparisons and function bodies remain intact.
fn conjuncts<'a>(expression: &'a BoundExpr, output: &mut Vec<&'a BoundExpr>) {
    if let ExprNode::Binary {
        op: BinaryOp::And,
        left,
        right,
    } = &expression.node
    {
        conjuncts(left, output);
        conjuncts(right, output);
    } else {
        output.push(expression);
    }
}

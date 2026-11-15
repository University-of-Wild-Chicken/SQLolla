//! Infer unique arbiters from declared constraints and separately identified indexes.
use super::*;
use std::collections::BTreeSet;

/// One uniqueness rule carries a bound membership predicate for its physical target schema.
pub(super) struct Arbiter<'a> {
    pub key: &'a KeyConstraint,
    pub predicate: Option<BoundExpr>,
}

/// Infer all covered arbiters before evaluating the INSERT source, including empty sources.
pub(super) fn infer<'a>(
    rule: &ConflictPlan,
    metadata: &'a TableSemantics,
    schema: &[Column],
    context: &ExecutionContext,
) -> Result<Vec<Arbiter<'a>>> {
    let mut keys = constraints(rule, metadata);
    for index in &metadata.unique_indexes {
        if !matches_key(&rule.target, &index.key, true) {
            continue;
        }
        let predicate = crate::unique_index::predicate(index, schema, context)?;
        if matches!(rule.target, ConflictTarget::Any)
            || crate::unique_index::implied(target_predicate(&rule.target), predicate.as_ref())?
        {
            keys.push(Arbiter {
                key: &index.key,
                predicate,
            });
        }
    }
    validate_inference(&keys, &rule.target)?;
    let order = &metadata.unique_enforcement_order;
    if !order.is_empty() {
        keys.sort_by_key(|key| order.iter().position(|name| name == &key.key.name));
    }
    Ok(keys)
}

/// Ordinary constraints cover all rows and cannot be mistaken for standalone index names.
fn constraints<'a>(rule: &ConflictPlan, metadata: &'a TableSemantics) -> Vec<Arbiter<'a>> {
    metadata
        .primary_key
        .iter()
        .chain(&metadata.unique)
        .filter(|key| matches_key(&rule.target, key, false))
        .map(|key| Arbiter {
            key,
            predicate: None,
        })
        .collect()
}

/// An index name is not a constraint name; key inference ignores spelling order and duplicates.
fn matches_key(target: &ConflictTarget, key: &KeyConstraint, index_only: bool) -> bool {
    match target {
        ConflictTarget::Any => true,
        ConflictTarget::Constraint(name) => !index_only && &key.name == name,
        ConflictTarget::Columns(columns) | ConflictTarget::PredicateColumns { columns, .. } => {
            columns.iter().collect::<BTreeSet<_>>() == key.columns.iter().collect()
        }
    }
}

/// Inference predicates constrain arbiter eligibility rather than filtering candidate rows.
fn target_predicate(target: &ConflictTarget) -> Option<&BoundExpr> {
    match target {
        ConflictTarget::PredicateColumns { predicate, .. } => Some(predicate),
        _ => None,
    }
}

/// Keep missing named constraints separate from absent index inference and deferred arbiters.
fn validate_inference(keys: &[Arbiter<'_>], target: &ConflictTarget) -> Result<()> {
    if keys.is_empty() && !matches!(target, ConflictTarget::Any) {
        let code = if matches!(target, ConflictTarget::Constraint(_)) {
            "42704"
        } else {
            "42P10"
        };
        return Err(inference_error(
            code,
            "No matching unique constraint or index",
        ));
    }
    if keys.iter().any(|key| key.key.deferrable) {
        return Err(inference_error(
            "55000",
            "Deferrable arbiter is not supported by ON CONFLICT",
        ));
    }
    Ok(())
}

/// SQLSTATE alone cannot establish which PostgreSQL preparation boundary produced an error.
fn inference_error(code: &str, message: &str) -> Error {
    let mut failure = error(&format!("sql.{code}"), message);
    failure.sql = Some(Box::new(SqlFailure {
        sqlstate: code.into(),
        phase: FailurePhase::Unspecified,
        constraint_name: None,
        column_name: None,
        exposed_rows: vec![],
        effects: vec![],
    }));
    failure
}

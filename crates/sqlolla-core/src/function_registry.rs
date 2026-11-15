//! Shared function identity, volatility and implicit-call conversion admission.
use crate::json::error;
use crate::*;
mod signatures;
#[cfg(test)]
mod tests;
pub(crate) use signatures::{builtin_signatures, implicit_cast, nominal};

/// The pinned bootstrap inventory prevents undeclared builtin collisions from looking absent.
pub(crate) fn builtin_name(name: &str) -> bool {
    include_str!("function_registry/pg16_builtin_names.txt")
        .lines()
        .any(|known| known == name)
}

/// PostgreSQL volatility is separate from whether the evaluator has an implementation.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Volatility {
    /// Result depends only on its arguments within the declared scalar profile.
    Immutable,
    /// Result requires an explicit statement/session snapshot.
    Stable,
    /// Invocation order and effects may change the result.
    Volatile,
}

/// Observable effect family, independent of a function's result type.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Effect {
    /// No external mutation is performed.
    None,
    /// Nontransactional sequence allocation/state.
    Sequence,
    /// Transactional notification queue.
    Notification,
    /// Advisory lock ownership.
    Advisory,
    /// External backend process state.
    Backend,
}

/// One capability record composes scalar NULL/body admission with context/effect requirements.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct BuiltinSpec {
    /// Required temporal stability of a function family.
    pub volatility: Volatility,
    /// Observable effect family.
    pub effect: Effect,
    /// Scalar implementation metadata, absent for context/effect handlers.
    pub scalar: Option<scalar::BuiltinBehavior>,
}

/// Recognize function identities without confusing a known signature with an executable body.
pub(crate) fn builtin(name: &str) -> Option<BuiltinSpec> {
    let scalar = scalar::builtin_behavior(name).or_else(|| internal_behavior(name));
    let (volatility, effect) = match name {
        "nextval" | "setval" => (Volatility::Volatile, Effect::Sequence),
        "currval" | "lastval" => (Volatility::Volatile, Effect::None),
        "pg_notify" => (Volatility::Volatile, Effect::Notification),
        "pg_advisory_xact_lock" | "pg_try_advisory_xact_lock" | "pg_advisory_xact_lock_shared" => {
            (Volatility::Volatile, Effect::Advisory)
        }
        "pg_terminate_backend" => (Volatility::Volatile, Effect::Backend),
        "now" | "current_timestamp" | "current_schema" | "current_database" | "version"
        | "current_setting" | "pg_backend_pid" | "to_regclass" => {
            (Volatility::Stable, Effect::None)
        }
        "concat" | "concat_ws" | "array_to_string" | "json_build_array" | "json_build_object"
        | "jsonb_build_array" | "jsonb_build_object" => (Volatility::Stable, Effect::None),
        _ if scalar.is_some() => (Volatility::Immutable, Effect::None),
        _ => return None,
    };
    Some(BuiltinSpec {
        volatility,
        effect,
        scalar,
    })
}

/// Compiler-owned operation names never become SQL-callable through the bootstrap-name gate.
fn internal_behavior(name: &str) -> Option<scalar::BuiltinBehavior> {
    match name {
        "__sqlolla_regex" | "__sqlolla_array_element" | "__sqlolla_json_access" => {
            Some(scalar::BuiltinBehavior {
                null_policy: scalar::NullPolicy::Strict,
                implemented: true,
            })
        }
        "__sqlolla_array_slice" => Some(scalar::BuiltinBehavior {
            null_policy: scalar::NullPolicy::Custom,
            implemented: true,
        }),
        _ => None,
    }
}

/// Insert the implicit system schema only when the caller did not explicitly position it.
pub(crate) fn search_path(path: &[String]) -> Vec<&str> {
    let mut result: Vec<_> = path.iter().map(String::as_str).collect();
    if !result.contains(&"pg_catalog") {
        result.insert(0, "pg_catalog");
    }
    result
}

/// Immutable metadata bodies must be independent of context even when no rows are present.
pub(crate) fn require_immutable(expression: &BoundExpr, code: &str) -> Result<()> {
    let mut copy = expression.clone();
    immutable_tree(&mut copy, code)
}

/// Traverse one owned temporary copy; admission never mutates the caller's typed expression.
fn immutable_tree(expression: &mut BoundExpr, code: &str) -> Result<()> {
    if !immutable_operation(expression) {
        return Err(error(
            code,
            "Expression is not an admitted immutable local scalar",
        ));
    }
    crate::rewrite::scalar_children(expression, &mut |child| immutable_tree(child, code))
}

/// Classify local operations before descending so lazy branches cannot hide session dependence.
pub(crate) fn immutable_operation(expression: &BoundExpr) -> bool {
    match &expression.node {
        ExprNode::Context { .. } | ExprNode::Subquery { .. } | ExprNode::OuterColumn { .. } => {
            false
        }
        ExprNode::Call { name, args } => immutable_call(name, args),
        ExprNode::Cast { arg } | ExprNode::AssignmentCast { arg } => {
            immutable_cast(&arg.ty, &expression.ty)
        }
        ExprNode::Arithmetic { op, left, right } => {
            !(matches!(op.as_str(), "add" | "subtract")
                && left.ty == SqlType::Timestamptz
                && right.ty == SqlType::Interval)
        }
        _ => true,
    }
}

/// Type-sensitive extract overloads remain stable for timestamps with time zones.
pub(crate) fn immutable_call(name: &str, args: &[BoundExpr]) -> bool {
    let Some(spec) = builtin(name) else {
        return false;
    };
    spec.volatility == Volatility::Immutable
        && spec.effect == Effect::None
        && spec.scalar.is_some_and(|body| body.implemented)
        && !(matches!(name, "extract" | "date_part")
            && args.iter().any(|arg| arg.ty == SqlType::Timestamptz))
}

/// Strictness describes this operator only; children must still pass ordinary planning first.
pub(crate) fn strict_operation(expression: &BoundExpr) -> bool {
    match &expression.node {
        ExprNode::Call { name, .. } => builtin(name)
            .and_then(|spec| spec.scalar)
            .is_some_and(|body| body.null_policy == scalar::NullPolicy::Strict),
        ExprNode::Binary { op, .. } => !matches!(op, BinaryOp::And | BinaryOp::Or),
        ExprNode::Not(_)
        | ExprNode::Negate(_)
        | ExprNode::Arithmetic { .. }
        | ExprNode::Cast { .. }
        | ExprNode::AssignmentCast { .. }
        | ExprNode::IntervalCast { .. }
        | ExprNode::Pattern { .. } => true,
        _ => false,
    }
}

/// Text/temporal and nominal-input casts may depend on DateStyle, time zone or catalog state.
fn immutable_cast(source: &SqlType, target: &SqlType) -> bool {
    if nominal(source) == nominal(target) {
        return true;
    }
    let temporal = |ty: &SqlType| {
        matches!(
            ty,
            SqlType::Date | SqlType::Timestamp | SqlType::Timestamptz | SqlType::Interval
        )
    };
    !(temporal(source)
        || temporal(target)
        || matches!(source, SqlType::Enum { .. } | SqlType::Regclass)
        || matches!(target, SqlType::Enum { .. } | SqlType::Regclass))
}

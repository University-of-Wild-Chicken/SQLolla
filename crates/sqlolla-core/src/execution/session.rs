//! Substitute supplied stable context after planning, without reclassifying runtime errors.
use crate::json::error;
use crate::*;

/// Evaluate schema-default or CHECK expressions using the same context contract.
pub(crate) fn evaluate(expr: &BoundExpr, row: &Row, context: &ExecutionContext) -> Result<Cell> {
    super::scalar(expr, row, &Database::new(), &super::Runtime::new(context))
}

/// Missing context is a coverage refusal; an explicitly absent setting is a SQL observation.
pub(super) fn resolve(expr: &BoundExpr, context: &ExecutionContext) -> Result<Cell> {
    let ExprNode::Context { key, missing_ok } = &expr.node else {
        return Err(error(
            "invalid.context_expression",
            "Expected a context leaf",
        ));
    };
    let value = context
        .settings
        .get(key)
        .ok_or_else(|| error("unsupported.context_value", key))?;
    if value.is_null() && !*missing_ok && key.starts_with("setting:") {
        return Err(error("sql.42704", "Explicitly absent session setting"));
    }
    if value.sql_type() != expr.ty {
        return Err(error("invalid.context_type", key));
    }
    Ok(value.clone())
}

/// Resolve supplied relation identity; non-NULL backend signaling remains an explicit effect refusal.
pub(super) fn function(name: &str, args: &[Cell], context: &ExecutionContext) -> Result<Cell> {
    match name {
        "to_regclass" => crate::catalog_lookup::resolve(&args[0], context.catalog_lookup.as_ref()),
        "pg_terminate_backend" if args.iter().any(Cell::is_null) => Ok(Cell::Null(SqlType::Bool)),
        "pg_terminate_backend" => Err(error(
            "unsupported.backend_termination",
            "Backend signaling requires its effect contract",
        )),
        _ => Err(error("invalid.context_call", "Unknown contextual function")),
    }
}

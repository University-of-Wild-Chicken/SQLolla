//! Strict JSON access and finite adapter-input validation.
use crate::{Error, Result};
use serde_json::Value;

/// Construct a structured, deterministic core diagnostic.
pub(crate) fn error(code: &str, message: impl Into<String>) -> Error {
    Error {
        code: code.into(),
        message: message.into(),
        sql: None,
    }
}

/// Require a string field without coercing JSON types.
pub(crate) fn text<'a>(node: &'a Value, key: &str) -> Result<&'a str> {
    node.get(key)
        .and_then(Value::as_str)
        .ok_or_else(|| error("invalid.ast", format!("Missing string field {key}")))
}

/// Require an array field without accepting omitted information.
pub(crate) fn array<'a>(node: &'a Value, key: &str) -> Result<&'a Vec<Value>> {
    node.get(key)
        .and_then(Value::as_array)
        .ok_or_else(|| error("invalid.ast", format!("Missing array field {key}")))
}

/// Read an optional text field while rejecting nontext payloads.
pub(crate) fn optional(node: &Value, key: &str) -> Result<Option<String>> {
    match node.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::String(value)) => Ok(Some(value.clone())),
        _ => Err(error(
            "invalid.ast",
            format!("Invalid optional string {key}"),
        )),
    }
}

/// Require a Boolean field; absent flags must never become false implicitly.
pub(crate) fn boolean(node: &Value, key: &str) -> Result<bool> {
    node.get(key)
        .and_then(Value::as_bool)
        .ok_or_else(|| error("invalid.ast", format!("Missing boolean field {key}")))
}

/// Reject unexpected node fields so a newer feature cannot disappear silently.
pub(crate) fn fields(node: &Value, allowed: &[&str]) -> Result<()> {
    let object = node
        .as_object()
        .ok_or_else(|| error("invalid.ast", "Expected object"))?;
    if allowed.iter().any(|key| !object.contains_key(*key)) {
        return Err(error("invalid.ast", "Required AST field is missing"));
    }
    for key in object.keys() {
        if !allowed.contains(&key.as_str()) {
            return Err(error(
                "unsupported.ast_field",
                format!("Unexpected field {key}"),
            ));
        }
    }
    Ok(())
}

/// Limit nested adapter input before recursive binding to prevent stack exhaustion.
pub(crate) fn bounded(node: &Value, depth: usize, budget: &mut usize) -> Result<()> {
    if depth > 64 || *budget == 0 {
        return Err(error(
            "unsupported.ast_size",
            "AST depth or node budget exceeded",
        ));
    }
    *budget -= 1;
    match node {
        Value::Array(items) => {
            for item in items {
                bounded(item, depth + 1, budget)?;
            }
        }
        Value::Object(items) => {
            for item in items.values() {
                bounded(item, depth + 1, budget)?;
            }
        }
        _ => (),
    }
    Ok(())
}

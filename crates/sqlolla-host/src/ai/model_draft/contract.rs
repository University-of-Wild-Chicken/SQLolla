//! Strict provider response schema for the model-drafting operation.
use super::*;

/// A closed object requires every listed field, including explicit nulls for inapplicable rule fields.
fn object(properties: Value) -> Value {
    let required: Vec<_> = properties
        .as_object()
        .expect("schema properties")
        .keys()
        .collect();
    json!({"type":"object","additionalProperties":false,"properties":properties,"required":required})
}

/// Bound list sizes at the provider and independently again after extraction.
fn list(items: Value, max: usize) -> Value {
    json!({"type":"array","items":items,"maxItems":max})
}

/// Build the four scalar families whose values can be entered in the simple schema editor.
fn scalar_type() -> Value {
    json!({"type":"string","enum":["int32","int64","text","bool"]})
}

/// Return a schema with the exact prompt identity and no alternate provider output fields.
pub(super) fn schema(digest: &str) -> Value {
    let column =
        object(json!({"name":{"type":"string"},"ty":scalar_type(),"nullable":{"type":"boolean"}}));
    let relation = object(
        json!({"name":{"type":"string"},"columns":list(column,32),"bound":{"type":"integer"}}),
    );
    let cell = object(json!({"ty":scalar_type(),"value":{"type":"string"}}));
    let rule = object(
        json!({"kind":{"type":"string","enum":["no_sql_error","max_rows","only_value","no_value"]},
        "column":{"type":["integer","null"]},"max":{"type":["integer","null"]},
        "value":{"anyOf":[cell.clone(),{"type":"null"}]}}),
    );
    object(
        json!({"context_sha256":{"type":"string","enum":[digest]},"relations":list(relation,12),
        "values":list(cell,32),"rule":rule,"sql":{"type":"string"},"assumptions":list(json!({"type":"string"}),8)}),
    )
}

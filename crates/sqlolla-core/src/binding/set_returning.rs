//! Lift target-list UNNEST into a typed relation before scalar projection and DISTINCT.
use super::*;
use serde_json::json;

/// Compiler-owned qualifier cannot be spelled in valid PostgreSQL identifier text.
const PREFIX: &str = "\0sqlolla.srf:";

impl Binder<'_> {
    /// Rewrite each sibling set call into one private typed column, retaining the original source.
    pub(super) fn expanded_sets(&self, node: &Value, input: Plan) -> Result<(Value, Plan)> {
        if !contains(&node["items"]) {
            return Ok((node.clone(), input));
        }
        let mut arrays = Vec::new();
        let mut rewritten = node.clone();
        rewritten["items"] = self.set_children(&node["items"], &input.schema, &mut arrays)?;
        let schema = expanded_schema(&input.schema, &arrays)?;
        Ok((
            rewritten,
            Plan {
                schema,
                node: RelNode::ExpandSets {
                    input: Box::new(input),
                    arrays,
                },
            },
        ))
    }

    /// Query scopes are opaque; forbidden conditional/aggregate placement remains a SQL error.
    fn set_children(
        &self,
        node: &Value,
        source: &[Column],
        arrays: &mut Vec<BoundExpr>,
    ) -> Result<Value> {
        if !contains(node) {
            return Ok(node.clone());
        }
        if is_unnest(node) {
            return self.set_reference(node, source, arrays);
        }
        validate_placement(node)?;
        match node {
            Value::Array(values) => values
                .iter()
                .map(|value| self.set_children(value, source, arrays))
                .collect(),
            Value::Object(values) => {
                let mut result = serde_json::Map::new();
                for (key, value) in values {
                    result.insert(key.clone(), self.set_children(value, source, arrays)?);
                }
                Ok(Value::Object(result))
            }
            _ => Ok(node.clone()),
        }
    }

    /// A set argument is an array expression, never an already expanded or nested set call.
    fn set_reference(
        &self,
        node: &Value,
        source: &[Column],
        arrays: &mut Vec<BoundExpr>,
    ) -> Result<Value> {
        fields(node, &["tag", "name", "args"])?;
        let [argument] = array(node, "args")?.as_slice() else {
            return Err(error(
                "sql.42883",
                "Target-list unnest needs one array argument",
            ));
        };
        if contains(argument) {
            return Err(error(
                "unsupported.srf_nesting",
                "Nested set-returning calls",
            ));
        }
        let argument = self.expressions(source).bind(argument, None)?;
        if !matches!(argument.ty, SqlType::Array(_)) {
            return Err(error("sql.42883", "unnest requires an array"));
        }
        let qualifier = format!("{PREFIX}{}", arrays.len());
        arrays.push(argument);
        Ok(json!({"tag":"column","name":"unnest","qualifier":qualifier,"schema":null}))
    }
}

/// Private array element columns retain their exact nominal type and nullable padding.
fn expanded_schema(source: &[Column], arrays: &[BoundExpr]) -> Result<Vec<Column>> {
    let mut schema = source.to_vec();
    for (index, array) in arrays.iter().enumerate() {
        let SqlType::Array(element) = &array.ty else {
            return Err(error("invalid.srf_type", "Expected array type"));
        };
        schema.push(Column {
            name: "unnest".into(),
            ty: *element.clone(),
            nullable: true,
            qualifier: Some(format!("{PREFIX}{index}")),
            schema: None,
        });
    }
    Ok(schema)
}

/// PostgreSQL rejects current-scope set calls inside conditional and aggregate arguments.
fn validate_placement(node: &Value) -> Result<()> {
    if matches!(
        node["tag"].as_str(),
        Some("case" | "coalesce" | "aggregate" | "filtered_aggregate")
    ) {
        return Err(error(
            "sql.0A000",
            "Set-returning function in conditional or aggregate expression",
        ));
    }
    Ok(())
}

/// Only builtin or explicitly pg_catalog-qualified UNNEST names denote this set operator.
fn is_unnest(node: &Value) -> bool {
    node["tag"] == "call"
        && (node["name"] == json!(["unnest"]) || node["name"] == json!(["pg_catalog", "unnest"]))
}

/// Discover only the current scalar projection scope, without entering nested queries.
pub(super) fn contains(node: &Value) -> bool {
    if matches!(
        node.get("tag").and_then(Value::as_str),
        Some(
            "query"
                | "scalar_subquery"
                | "exists"
                | "in_query"
                | "array_query"
                | "quantified_query"
        )
    ) {
        return false;
    }
    is_unnest(node)
        || match node {
            Value::Array(values) => values.iter().any(contains),
            Value::Object(values) => values.values().any(contains),
            _ => false,
        }
}

/// Wildcards expose original source columns and never compiler-owned expansion positions.
pub(super) fn visible(schema: &[Column]) -> &[Column] {
    let length = schema
        .iter()
        .position(|column| {
            column
                .qualifier
                .as_ref()
                .is_some_and(|name| name.starts_with("\0sqlolla."))
        })
        .unwrap_or(schema.len());
    &schema[..length]
}

/// Internal storage-order expansion affects pagination without promising presentation order.
pub(super) fn presentation(plan: Plan, body: &Value, orders: &[Value]) -> Plan {
    if orders.is_empty() && contains(&body["items"]) {
        Plan {
            schema: plan.schema.clone(),
            node: RelNode::Unordered {
                input: Box::new(plan),
            },
        }
    } else {
        plan
    }
}

/// ORDER BY/SRF evaluation timing is admitted only after its separate sequencing contract.
pub(super) fn reject_ordered(node: &Value) -> Result<()> {
    if contains(&node["items"]) {
        return Err(error(
            "unsupported.srf_order",
            "Set-returning projection with ORDER BY",
        ));
    }
    Ok(())
}

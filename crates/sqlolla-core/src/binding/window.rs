//! Lift current-scope ranking calls before projection while preserving exact partition/order identity.
use super::*;
use serde_json::json;

/// Hidden qualifiers cannot appear in valid PostgreSQL identifier text.
const PREFIX: &str = "\0sqlolla.window:";

/// Window slots are local compilation state and share one complete specification.
struct Windows<'a, 'b> {
    binder: &'a Binder<'b>,
    source: &'a [Column],
    spec: Option<WindowSpec>,
}

impl Binder<'_> {
    /// Expand SELECT and ORDER windows jointly so repeated references use the same tie choice.
    pub(super) fn expanded_windows(
        &self,
        node: &Value,
        input: Plan,
        orders: &[Value],
    ) -> Result<(Value, Plan, Vec<Value>)> {
        if !contains(&node["items"]) && !orders.iter().any(contains) {
            return Ok((node.clone(), input, orders.to_vec()));
        }
        reject_mixed_stage(&input.schema)?;
        let mut windows = Windows {
            binder: self,
            source: &input.schema,
            spec: None,
        };
        let mut rewritten = node.clone();
        rewritten["items"] = windows.replace(&node["items"])?;
        let orders = orders
            .iter()
            .map(|value| windows.replace(value))
            .collect::<Result<Vec<_>>>()?;
        let spec = windows
            .spec
            .ok_or_else(|| error("invalid.window", "Detected window produced no slot"))?;
        Ok((rewritten, window_plan(input, spec), orders))
    }
}

/// A separate checked stage order is required before combining row expansion and window ranking.
fn reject_mixed_stage(columns: &[Column]) -> Result<()> {
    if columns.iter().any(|column| {
        column
            .qualifier
            .as_ref()
            .is_some_and(|value| value.starts_with("\0sqlolla.srf:"))
    }) {
        return Err(error(
            "unsupported.window_srf",
            "Mixed set-returning/window evaluation requires a separate stage contract",
        ));
    }
    Ok(())
}

/// Ranking preserves the source prefix and appends only compiler-private integral slots.
fn window_plan(input: Plan, spec: WindowSpec) -> Plan {
    Plan {
        schema: window_schema(&input.schema, &spec),
        node: RelNode::Window {
            input: Box::new(input),
            spec,
        },
    }
}

impl Windows<'_, '_> {
    /// Query-bearing expressions remain opaque so inner ranking uses its own relation and scope.
    fn replace(&mut self, node: &Value) -> Result<Value> {
        if !contains(node) {
            return Ok(node.clone());
        }
        if node["tag"] == "window_call" {
            return self.reference(node);
        }
        match node {
            Value::Array(values) => values.iter().map(|value| self.replace(value)).collect(),
            Value::Object(values) => {
                let mut result = serde_json::Map::new();
                for (key, value) in values {
                    result.insert(key.clone(), self.replace(value)?);
                }
                Ok(Value::Object(result))
            }
            _ => Ok(node.clone()),
        }
    }

    /// Equal bound window specifications share slots; differing specifications fail closed.
    fn reference(&mut self, node: &Value) -> Result<Value> {
        fields(node, &["tag", "function", "window"])?;
        let function = function(&node["function"])?;
        let spec = self.specification(&node["window"])?;
        let current = self.spec.get_or_insert_with(|| spec.clone());
        if current.partition_by != spec.partition_by || current.order_by != spec.order_by {
            return Err(error(
                "unsupported.window_specifications",
                "Multiple distinct window orderings require correlated sort choices",
            ));
        }
        let index = if let Some(index) = current
            .functions
            .iter()
            .position(|value| *value == function)
        {
            index
        } else {
            current.functions.push(function);
            current.functions.len() - 1
        };
        Ok(
            json!({"tag":"column","name":function_name(function),"qualifier":format!("{PREFIX}{index}"),"schema":null}),
        )
    }

    /// Window keys bind against input columns without SELECT aliases or ordinal rewriting.
    fn specification(&self, node: &Value) -> Result<WindowSpec> {
        fields(node, &["name", "partition_by", "order_by", "frame"])?;
        if !node["name"].is_null() || !node["frame"].is_null() {
            return Err(error(
                "unsupported.window_frame",
                "Named windows and explicit frames are not admitted",
            ));
        }
        if contains(node) {
            return Err(error("sql.42P20", "Nested window call"));
        }
        let partition_by = array(node, "partition_by")?
            .iter()
            .map(|value| self.binder.expressions(self.source).bind(value, None))
            .collect::<Result<Vec<_>>>()?;
        let order_by = array(node, "order_by")?
            .iter()
            .map(|value| self.order_key(value))
            .collect::<Result<Vec<_>>>()?;
        Ok(WindowSpec {
            partition_by,
            order_by,
            functions: vec![],
        })
    }

    /// Numeric window-order expressions are constants, unlike top-level ORDER BY ordinals.
    fn order_key(&self, node: &Value) -> Result<OrderKey> {
        fields(node, &["expression", "direction", "nulls"])?;
        let expression = self
            .binder
            .expressions(self.source)
            .bind(&node["expression"], None)?;
        let (direction, nulls_first) = super::extended::ordering_policy(node)?;
        Ok(OrderKey {
            expression,
            direction,
            nulls_first,
        })
    }
}

/// Detect only current-scope calls; subquery bodies get their own binding pass.
pub(super) fn contains(node: &Value) -> bool {
    match node.get("tag").and_then(Value::as_str) {
        Some("window_call") => return true,
        Some(
            "query" | "scalar_subquery" | "exists" | "in_query" | "quantified_query"
            | "array_query",
        ) => return false,
        _ => (),
    }
    match node {
        Value::Array(values) => values.iter().any(contains),
        Value::Object(values) => values.values().any(contains),
        _ => false,
    }
}

/// Only zero-argument builtin ranking names denote this explicitly modeled window family.
fn function(node: &Value) -> Result<WindowFunction> {
    fields(node, &["tag", "name", "args"])?;
    if node["tag"] != "call" || !array(node, "args")?.is_empty() {
        return Err(error(
            "unsupported.window_function",
            "Only zero-argument ranking calls are admitted",
        ));
    }
    let names = array(node, "name")?
        .iter()
        .map(|value| {
            value
                .as_str()
                .ok_or_else(|| error("invalid.ast", "Window function identifier"))
        })
        .collect::<Result<Vec<_>>>()?;
    let name = match names.as_slice() {
        [name] | ["pg_catalog", name] => *name,
        _ => {
            return Err(error(
                "unsupported.window_function",
                "Custom window function",
            ));
        }
    };
    function_kind(name)
}

/// Builtin ranking names have stable integral result families.
fn function_kind(name: &str) -> Result<WindowFunction> {
    match name {
        "row_number" => Ok(WindowFunction::RowNumber),
        "rank" => Ok(WindowFunction::Rank),
        "dense_rank" => Ok(WindowFunction::DenseRank),
        _ => Err(error("unsupported.window_function", name)),
    }
}

/// Hidden scalar slots retain PostgreSQL's public output name while qualifiers suppress wildcard exposure.
fn window_schema(input: &[Column], spec: &WindowSpec) -> Vec<Column> {
    let mut columns = input.to_vec();
    for (index, function) in spec.functions.iter().enumerate() {
        columns.push(Column {
            name: function_name(*function).into(),
            ty: SqlType::Int64,
            nullable: false,
            qualifier: Some(format!("{PREFIX}{index}")),
            schema: None,
        });
    }
    columns
}

/// Keep implicit result labels independent from Rust enum serialization.
fn function_name(function: WindowFunction) -> &'static str {
    match function {
        WindowFunction::RowNumber => "row_number",
        WindowFunction::Rank => "rank",
        WindowFunction::DenseRank => "dense_rank",
    }
}

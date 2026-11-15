//! Deterministic PostgreSQL output labels are distinct from internal expression identifiers.
use super::*;

/// Explicit projection aliases are handled by the caller; anonymous scalars use PostgreSQL labels.
pub(crate) fn output_label(node: &Value, bound: &BoundExpr) -> String {
    label(node, bound).unwrap_or_else(|| "?column?".into())
}

/// Derive names from expression syntax and already-bound scalar-subquery output metadata.
fn label(node: &Value, bound: &BoundExpr) -> Option<String> {
    let bound = unwrapped(node, bound);
    match node.get("tag")?.as_str()? {
        "column" => node["name"].as_str().map(str::to_owned),
        "aggregate" => node["fn"].as_str().map(str::to_owned),
        "count_all" => Some("count".into()),
        "filtered_aggregate" => label(&node["aggregate"], bound),
        "call" | "named_call" => node["name"].as_array()?.last()?.as_str().map(str::to_owned),
        "coalesce" | "greatest" | "least" | "substring" | "extract" | "exists" => {
            node["tag"].as_str().map(str::to_owned)
        }
        "null_if" => Some("nullif".into()),
        "array" | "array_query" => Some("array".into()),
        "context_value" => node["value"].as_str().map(str::to_owned),
        "scalar_subquery" => subquery_label(bound),
        "case" => Some(case_label(node, bound)),
        "cast" => Some(cast_label(node, bound)),
        "subscript" => subscript_label(node, bound),
        _ => None,
    }
}

/// Subscripting inherits the base expression's original output name, including ARRAY and casts.
fn subscript_label(node: &Value, bound: &BoundExpr) -> Option<String> {
    match &bound.node {
        ExprNode::Call { args, .. } => label(&node["base"], args.first()?),
        _ => None,
    }
}

/// Contextual coercion does not rename the original expression.
fn unwrapped<'a>(node: &Value, mut bound: &'a BoundExpr) -> &'a BoundExpr {
    if node["tag"] != "cast" {
        while let ExprNode::Cast { arg } | ExprNode::AssignmentCast { arg } = &bound.node {
            bound = arg;
        }
    }
    bound
}

/// Scalar subqueries expose their first selected output label, including nested aliases.
fn subquery_label(bound: &BoundExpr) -> Option<String> {
    if let ExprNode::Subquery { query, .. } = &bound.node {
        query.schema.first().map(|column| column.name.clone())
    } else {
        None
    }
}

/// CASE inherits only an original ELSE name; anonymous cast labels remain weak.
fn case_label(node: &Value, bound: &BoundExpr) -> String {
    if let ExprNode::Case { otherwise, .. } = &bound.node {
        if let Some(name) = original_label(&node["else"], otherwise) {
            return name;
        }
    }
    "case".into()
}

/// An explicit cast preserves a named argument; otherwise it exposes the canonical type name.
fn cast_label(node: &Value, bound: &BoundExpr) -> String {
    if let ExprNode::Cast { arg } = &bound.node {
        if let Some(name) = original_label(&node["arg"], arg) {
            return name;
        }
    }
    type_label(&bound.ty)
}

/// A nested cast's generated type label is weak; only an original named expression survives outer casts.
fn original_label(node: &Value, bound: &BoundExpr) -> Option<String> {
    let bound = unwrapped(node, bound);
    if node["tag"] == "cast" {
        let child = match &bound.node {
            ExprNode::Cast { arg } => arg.as_ref(),
            _ => bound,
        };
        return original_label(&node["arg"], child);
    }
    if let ExprNode::Case { otherwise, .. } = &bound.node {
        if node["tag"] == "case" {
            return original_label(&node["else"], otherwise);
        }
    }
    if let ExprNode::Call { args, .. } = &bound.node {
        if node["tag"] == "subscript" {
            return original_label(&node["base"], args.first()?);
        }
    }
    label(node, bound)
}

/// Built-in type labels match PostgreSQL's internal names, with array dimensions excluded.
fn type_label(ty: &SqlType) -> String {
    let name = match ty {
        SqlType::Int16 => "int2",
        SqlType::Int32 => "int4",
        SqlType::Int64 => "int8",
        SqlType::Bool => "bool",
        SqlType::Float64 => "float8",
        SqlType::Numeric { .. } => "numeric",
        SqlType::Text => "text",
        SqlType::Varchar { .. } => "varchar",
        SqlType::Char { .. } | SqlType::Bpchar => "bpchar",
        SqlType::Name => "name",
        SqlType::Oid => "oid",
        SqlType::Xid => "xid",
        SqlType::Regclass => "regclass",
        SqlType::InternalChar => "char",
        SqlType::Bytea => "bytea",
        SqlType::Date => "date",
        SqlType::Timestamp => "timestamp",
        SqlType::Timestamptz => "timestamptz",
        SqlType::Interval => "interval",
        SqlType::Jsonb => "jsonb",
        SqlType::Json => "json",
        SqlType::Void => "void",
        SqlType::Bit { .. } => "bit",
        SqlType::Array(element) => return type_label(element),
        SqlType::Enum { name, .. } => return name.rsplit('.').next().unwrap_or(name).into(),
    };
    name.into()
}

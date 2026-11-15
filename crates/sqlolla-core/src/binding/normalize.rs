//! Strict structural v2 adaptation; unimplemented modifiers are rejected before lowering.
use super::*;
use serde_json::json;

/// Complete outer protocol keys, independent of any admitted semantic feature.
const REPORT_FIELDS: &[&str] = &[
    "protocol",
    "parser_commit",
    "assurance",
    "semantic_preservation",
    "result",
];
/// Every query modifier must survive parsing or produce an explicit refusal.
const QUERY_FIELDS: &[&str] = &[
    "tag",
    "body",
    "ctes",
    "recursive",
    "order_by",
    "limit",
    "offset",
    "offset_first",
    "locking",
];

/// Validate complete syntax-only provenance before consuming an extended AST.
pub(super) fn validate_report(report: &Value) -> Result<()> {
    bounded(report, 0, &mut 20_000)?;
    fields(report, REPORT_FIELDS)?;
    if text(report, "protocol")? != "sqlolla.parser.v2"
        || text(report, "parser_commit")? != "ad15c585155ff7fc2c5fc4c9746905e4959ff5eb"
        || text(report, "assurance")? != "syntax-only"
        || boolean(report, "semantic_preservation")?
    {
        return Err(error(
            "unsupported.protocol",
            "Invalid parser identity or assurance",
        ));
    }
    fields(&report["result"], &["parse_status", "export_status", "ast"])?;
    if text(&report["result"], "parse_status")? != "parsed"
        || text(&report["result"], "export_status")? != "exported"
    {
        return Err(error(
            "unsupported.parser_result",
            "Parser did not export the statement",
        ));
    }
    Ok(())
}

/// Preserve query modifiers explicitly while admitting only modeled lexical CTEs.
pub(super) fn query(node: &Value) -> Result<Value> {
    fields(node, QUERY_FIELDS)?;
    if text(node, "tag")? != "query" {
        return Err(error("unsupported.statement", "Expected query"));
    }
    if boolean(node, "recursive")? {
        return Err(error("unsupported.recursive", "Recursive CTE"));
    }
    if !array(node, "locking")?.is_empty() {
        return Err(error(
            "unsupported.locking",
            "Lock effects require a context model",
        ));
    }
    boolean(node, "offset_first")?;
    let body = json!({"tag":"modified_query", "body":query_body(node)?,
        "order_by":array(node,"order_by")?, "limit":node["limit"], "offset":node["offset"]});
    let ctes = array(node, "ctes")?
        .iter()
        .map(cte)
        .collect::<Result<Vec<_>>>()?;
    if ctes.is_empty() {
        Ok(body)
    } else {
        Ok(json!({"tag":"with","ctes":ctes,"body":body}))
    }
}

/// Aggregates used only by ORDER BY still create a grouping scope.
fn query_body(node: &Value) -> Result<Value> {
    let mut value = body(&node["body"])?;
    if value["tag"] == "select" && super::grouping::contains_aggregate(&node["order_by"]) {
        value["tag"] = json!("grouped_select");
        value["group_by"] = json!([]);
        value["having"] = Value::Null;
    }
    Ok(value)
}

/// Retain a CTE's exact declared names and recursively complete query modifiers.
fn cte(node: &Value) -> Result<Value> {
    fields(node, &["name", "columns", "query"])?;
    Ok(
        json!({"name":text(node,"name")?,"columns":array(node,"columns")?,"query":query(&node["query"])?}),
    )
}

/// Preserve admitted body fields and refuse grouping until its evaluator is available.
fn body(node: &Value) -> Result<Value> {
    match text(node, "tag")? {
        "select" => select(node),
        "values" => {
            fields(node, &["tag", "rows"])?;
            Ok(node.clone())
        }
        "set_operation" => {
            fields(node, &["tag", "op", "all", "left", "right"])?;
            Ok(
                json!({"tag":"set_operation","op":text(node,"op")?,"all":boolean(node,"all")?,
                "left":query(&node["left"])?,"right":query(&node["right"])?}),
            )
        }
        tag => Err(error("unsupported.query", tag)),
    }
}

/// Grouping and DISTINCT ON cannot disappear when adapting the common SELECT shape.
fn select(node: &Value) -> Result<Value> {
    fields(
        node,
        &[
            "tag",
            "items",
            "source",
            "where",
            "distinct",
            "group_by",
            "having",
            "distinct_on",
        ],
    )?;
    if !array(node, "distinct_on")?.is_empty() {
        return distinct_select(node);
    }
    if !array(node, "group_by")?.is_empty()
        || !node["having"].is_null()
        || super::grouping::contains_aggregate(&node["items"])
    {
        return Ok(json!({"tag":"grouped_select","items":array(node,"items")?,
            "source":source(&node["source"])?,"where":node["where"],"distinct":boolean(node,"distinct")?,
            "group_by":array(node,"group_by")?,"having":node["having"]}));
    }
    Ok(
        json!({"tag":"select","items":array(node,"items")?,"source":source(&node["source"])?,
        "where":node["where"],"distinct":boolean(node,"distinct")?}),
    )
}

/// Preserve DISTINCT ON keys without erasing unsupported grouping interaction.
fn distinct_select(node: &Value) -> Result<Value> {
    boolean(node, "distinct")?;
    if !array(node, "group_by")?.is_empty()
        || !node["having"].is_null()
        || super::grouping::contains_aggregate(&node["items"])
    {
        return Err(error(
            "unsupported.distinct_on_grouping",
            "Grouped DISTINCT ON needs aggregate-scope binding",
        ));
    }
    Ok(
        json!({"tag":"distinct_on_select","items":array(node,"items")?,
        "source":source(&node["source"])?,"where":node["where"],"distinct_on":array(node,"distinct_on")?}),
    )
}

/// Normalize only structurally represented FROM families; no SQL string parsing occurs.
pub(super) fn source(node: &Value) -> Result<Value> {
    if node.is_null() {
        return Ok(Value::Null);
    }
    match text(node, "tag")? {
        "table" => {
            fields(node, &["tag", "name", "alias", "columns", "schema"])?;
            let mut value = node.clone();
            value["tag"] = json!("extended_table");
            Ok(value)
        }
        "derived" => derived(node),
        "join" => join(node),
        tag => Err(error("unsupported.source", tag)),
    }
}

/// LATERAL requires explicit correlation and cannot be treated as an ordinary derived table.
fn derived(node: &Value) -> Result<Value> {
    fields(node, &["tag", "query", "alias", "columns", "lateral"])?;
    if boolean(node, "lateral")? {
        return Err(error("unsupported.lateral", "LATERAL"));
    }
    if !array(&node["query"], "locking")?.is_empty() {
        return Ok(json!({"tag":"locked_derived","query":node["query"],
            "alias":node["alias"],"columns":array(node,"columns")?}));
    }
    Ok(
        json!({"tag":"extended_derived","query":query(&node["query"])?,
        "alias":node["alias"],"columns":array(node,"columns")?}),
    )
}

/// CROSS/ON joins retain their conditions; USING/NATURAL are never rewritten by omission.
fn join(node: &Value) -> Result<Value> {
    fields(node, &["tag", "kind", "left", "right", "condition"])?;
    let condition = &node["condition"];
    let left = source(&node["left"])?;
    let right = source(&node["right"])?;
    match text(condition, "tag")? {
        "none" if text(node, "kind")? == "cross" => {
            fields(condition, &["tag"])?;
            Ok(json!({"tag":"product","left":left,"right":right}))
        }
        "on" => {
            fields(condition, &["tag", "expression"])?;
            Ok(
                json!({"tag":"join","kind":text(node,"kind")?,"left":left,"right":right,"on":condition["expression"]}),
            )
        }
        tag => Err(error("unsupported.join_condition", tag)),
    }
}

//! Propose an independent database model from a prompt, with its bounded analysis context explicit.
use super::*;
use serde::Serialize;
use sha2::{Digest, Sha256};
use sqlolla::pipeline::Request;
use std::collections::BTreeMap;

mod contract;
#[cfg(test)]
mod tests;

const INSTRUCTIONS: &str = "Create a small editable PostgreSQL 16 analysis model for the user's description. Return a schema, finite sample values, one business rule, one SQL statement, and brief assumptions. Use lower-case identifiers and types int32, int64, text or bool. Include modeled values for each column type and positive row bounds for input tables. Use only declared tables. Rule columns are zero-based; use values and counts appropriate to the user's task. Write int32/int64 values as decimal strings and bools as lowercase true/false. Use null in fields that do not apply to the chosen rule. No tools are available. Local checks decide compilation.";

/// Only the user-authored prompt and the sharing decision enter this route.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct ModelInput {
    instruction: String,
    share_prompt: bool,
}

/// A bounded query and its proposed analysis inputs must conform to the declared schema.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Proposed {
    context_sha256: String,
    relations: Vec<Relation>,
    values: Vec<TypedValue>,
    rule: Rule,
    sql: String,
    assumptions: Vec<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Relation {
    name: String,
    columns: Vec<Column>,
    bound: usize,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Column {
    name: String,
    ty: ScalarType,
    nullable: bool,
}

#[derive(Clone, Copy, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "snake_case")]
enum ScalarType {
    Int32,
    Int64,
    Text,
    Bool,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct TypedValue {
    ty: ScalarType,
    value: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Rule {
    kind: String,
    column: Option<usize>,
    max: Option<usize>,
    value: Option<TypedValue>,
}

/// Compile the exact proposed catalog and SQL locally before allowing client-side staging.
pub(super) async fn generate(
    State(state): State<HostState>,
    headers: HeaderMap,
    Json(input): Json<ModelInput>,
) -> Result<Json<Value>> {
    let (body, digest) = packet(&input)?;
    let byot = Byot::from_headers(&headers)?;
    let output = provider::complete(byot, body).await?;
    let proposed = parse(output, &digest)?;
    let request = request(&proposed)?;
    let compiled = draft::compile(&state, request.clone(), &proposed.sql).await?;
    let (provider, model) = byot.identity();
    Ok(Json(
        json!({"schema":"sqlolla.ai.model_draft.v1","status":"ok","provider":provider,"model":model,
        "authority":"untrusted_assistance","sql":proposed.sql,"assumptions":proposed.assumptions,
        "context_sha256":digest,"model_request":request,"validation":draft::assessment(&compiled)}),
    ))
}

/// Carry only the prompt to the provider after explicit consent.
fn packet(input: &ModelInput) -> Result<(provider::Completion, String)> {
    if !input.share_prompt {
        return Err(failure(
            "ai.model_consent",
            "Allow sharing your prompt to draft a model",
        ));
    }
    output::bounded_text(&input.instruction, 4096)?;
    let digest = format!("{:x}", Sha256::digest(input.instruction.as_bytes()));
    let body = provider::Completion {
        instructions: INSTRUCTIONS.into(),
        user: json!({"context_sha256":digest,"instruction":input.instruction}),
        schema_name: "sqlolla_model_draft_v1",
        schema: contract::schema(&digest),
    };
    Ok((body, digest))
}

/// Verify identity, capacity and plain text independently of the response schema.
fn parse(value: Value, digest: &str) -> Result<Proposed> {
    let proposed: Proposed = serde_json::from_value(value).map_err(|_| invalid())?;
    if proposed.context_sha256 != digest
        || proposed.assumptions.len() > 8
        || !(1..=12).contains(&proposed.relations.len())
        || proposed.values.len() > 32
    {
        return Err(invalid());
    }
    output::bounded_text(&proposed.sql, 16_384)?;
    for assumption in &proposed.assumptions {
        output::bounded_text(assumption, 512)?;
    }
    Ok(proposed)
}

/// Convert typed proposed values and relation metadata into the canonical offline request.
fn request(draft: &Proposed) -> Result<Request> {
    let tables = relations(&draft.relations)?;
    let bounds: BTreeMap<_, _> = draft
        .relations
        .iter()
        .map(|relation| (&relation.name, relation.bound))
        .collect();
    let has_type = |column: &Column| draft.values.iter().any(|value| value.ty == column.ty);
    if !draft
        .relations
        .iter()
        .flat_map(|relation| &relation.columns)
        .all(has_type)
    {
        return Err(failure(
            "ai.model_domain",
            "The model needs at least one value for each declared column type",
        ));
    }
    let domain = draft.values.iter().map(cell).collect::<Result<Vec<_>>>()?;
    let value = json!({"id":"browser-ai-model","sql":draft.sql,"mode":"analyze",
        "catalog":{"search_path":["public"],"tables":tables},"parameters":{},
        "bounds":bounds,"domain":domain,"property":property(&draft.rule)?,"include_model":true});
    serde_json::from_value(value).map_err(|_| invalid())
}

/// Validate unique table/column identifiers before building the closed catalog.
fn relations(relations: &[Relation]) -> Result<BTreeMap<&str, Value>> {
    let mut tables = BTreeMap::new();
    for relation in relations {
        if !identifier(&relation.name)
            || !(1..=8).contains(&relation.bound)
            || !(1..=32).contains(&relation.columns.len())
        {
            return Err(invalid());
        }
        let mut names = std::collections::BTreeSet::new();
        if !relation
            .columns
            .iter()
            .all(|column| identifier(&column.name) && names.insert(&column.name))
        {
            return Err(invalid());
        }
        let table = json!({"schema":"public","name":relation.name,"columns":relation.columns});
        if tables.insert(relation.name.as_str(), table).is_some() {
            return Err(invalid());
        }
    }
    Ok(tables)
}

/// Keep generated identifiers readable and inside PostgreSQL's ordinary identifier length.
fn identifier(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 63
        && name
            .bytes()
            .next()
            .is_some_and(|b| b.is_ascii_lowercase() || b == b'_')
        && name
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_')
}

/// Convert exact decimal strings without floating-point roundoff.
fn cell(typed: &TypedValue) -> Result<Value> {
    let value = match typed.ty {
        ScalarType::Int32 => json!(typed.value.parse::<i32>().map_err(|_| invalid())?),
        ScalarType::Int64 => json!(typed.value.parse::<i64>().map_err(|_| invalid())?),
        ScalarType::Bool => json!(typed.value.parse::<bool>().map_err(|_| invalid())?),
        ScalarType::Text => {
            output::bounded_text(&typed.value, 512)?;
            json!(typed.value)
        }
    };
    Ok(json!({"type":typed.ty,"value":value}))
}

/// Require exactly the fields associated with the proposed property.
fn property(rule: &Rule) -> Result<Value> {
    match (rule.kind.as_str(), rule.column, rule.max, &rule.value) {
        ("no_sql_error", None, None, None) => Ok(json!({"kind":"no_sql_error"})),
        ("max_rows", None, Some(max), None) => Ok(json!({"kind":"max_rows","max":max})),
        ("only_value" | "no_value", Some(column), None, Some(value)) => {
            Ok(json!({"kind":rule.kind,"column":column,"value":cell(value)?}))
        }
        _ => Err(invalid()),
    }
}

/// Provider diagnostics never include its raw proposal.
fn invalid() -> AiError {
    failure(
        "ai.model_draft_invalid",
        "The proposed model has invalid schema, values or rule",
    )
}

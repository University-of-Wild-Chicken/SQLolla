//! Refine the same user intent using canonical review cases and deterministic promotion gates.
use super::*;
use sha2::{Digest, Sha256};
use std::collections::BTreeSet;

/// Only archive identities and human intent enter here; model-visible evidence is reloaded locally.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct RefinementInput {
    bundle_id: String,
    model_revision_sha256: String,
    feedback: Vec<Value>,
    instruction: String,
    redact_values: bool,
    #[serde(default)]
    feedback_sha256: Option<String>,
}

/// At least one decisive case is required before any provider request can be issued.
pub(super) async fn refine(
    State(state): State<HostState>,
    headers: HeaderMap,
    Json(input): Json<RefinementInput>,
) -> Result<Json<Value>> {
    admission(&input)?;
    let prepared = canonical(&state, &input, None).await?;
    if prepared["status"] != "ok" {
        return Ok(Json(prepared));
    }
    expected_digest(&input, &prepared)?;
    let provider_input = provider_input(&input, &prepared)?;
    let prompt = packet(&input, &prepared)?;
    let byot = Byot::from_headers(&headers)?;
    let document = provider::request(byot, &provider_input, &prompt, true).await?;
    let draft = output::validate(document, &provider_input, &prompt, true)?;
    let sql = draft
        .sql
        .as_deref()
        .ok_or_else(|| failure("ai.proposal_sql", "SQL omitted"))?;
    let checked = canonical(&state, &input, Some((sql, &prepared))).await?;
    let mut result = output::envelope(byot.provider, &provider_input, &prompt, &draft, true);
    result["schema"] = json!("sqlolla.ai.refinement.v1");
    result["feedback_sha256"] = prepared["feedback_sha256"].clone();
    result["references"] = prepared["references"].clone();
    result["validation"] = checked;
    Ok(Json(result))
}

/// Human uncertainty stays in the snapshot but cannot alone authorize another generation.
fn admission(input: &RefinementInput) -> Result<()> {
    if input.feedback.len() > 256 {
        return Err(failure(
            "ai.feedback_limit",
            "Complete review snapshot exceeds 256 cases",
        ));
    }
    if !input.feedback.iter().any(decisive) {
        return Err(failure(
            "ai.feedback_required",
            "Choose at least one Yes or No case; Unsure alone cannot regenerate",
        ));
    }
    if input.redact_values || input.instruction.trim().is_empty() || input.instruction.len() > 4096
    {
        return Err(failure(
            "ai.refinement_consent",
            "Refinement requires the retained prompt and explicit unredacted case context",
        ));
    }
    Ok(())
}

/// JSON admission is deliberately narrow; the canonical worker validates the complete reference shape.
fn decisive(value: &Value) -> bool {
    matches!(
        value["disposition"].as_str(),
        Some("accepted_use_case" | "unintended_behavior")
    )
}

/// Both stages use the same immutable base and complete active snapshot, with no browser-supplied facts.
fn command(input: &RefinementInput, candidate: Option<(&str, &Value)>) -> Value {
    let mut value = json!({"action":"refinement.context", "bundle_id":input.bundle_id,
        "model_revision_sha256":input.model_revision_sha256,"feedback":input.feedback});
    if let Some((sql, prepared)) = candidate {
        value["action"] = json!("refinement.check");
        value["sql"] = json!(sql);
        value["feedback_sha256"] = prepared["feedback_sha256"].clone();
    }
    value
}

/// Refusals never become a ready candidate, and canonical diagnostics contain no authentication data.
async fn canonical(
    state: &HostState,
    input: &RefinementInput,
    candidate: Option<(&str, &Value)>,
) -> Result<Value> {
    let bytes = serde_json::to_vec(&command(input, candidate)).unwrap();
    let result = state.dispatch(bytes).await.map_err(|_| {
        failure(
            "ai.refinement_unavailable",
            "Canonical refinement validation could not complete",
        )
    })?;
    validate_result(input, &result, candidate)?;
    Ok(result)
}

/// Canonical refusals remain inspectable diagnostics, never a provider-generated readiness claim.
fn validate_result(
    input: &RefinementInput,
    result: &Value,
    candidate: Option<(&str, &Value)>,
) -> Result<()> {
    if result["status"] != "ok" {
        return Ok(());
    }
    if result["bundle_id"] != input.bundle_id
        || result["model_revision_sha256"] != input.model_revision_sha256
    {
        return Err(failure(
            "ai.refinement_identity",
            "Refinement returned a different source revision",
        ));
    }
    if let Some((_, prepared)) = candidate
        && result["feedback_sha256"] != prepared["feedback_sha256"]
    {
        return Err(failure(
            "ai.refinement_stale",
            "Review requirements changed during generation",
        ));
    }
    Ok(())
}

/// An optional browser preview digest must identify the exact canonical snapshot about to be sent.
fn expected_digest(input: &RefinementInput, prepared: &Value) -> Result<()> {
    if input
        .feedback_sha256
        .as_ref()
        .is_some_and(|hash| prepared["feedback_sha256"] != *hash)
    {
        return Err(failure(
            "ai.refinement_stale",
            "Review requirements differ from the previewed snapshot",
        ));
    }
    Ok(())
}

/// Reuse the strict output validator with an explicit case anchor; citations remain packet-scoped.
fn provider_input(input: &RefinementInput, prepared: &Value) -> Result<Input> {
    let anchor = prepared["packet"]["feedback"]
        .as_array()
        .and_then(|items| items.iter().find(|item| decisive(item)))
        .and_then(|item| item["instance_id"].as_str())
        .ok_or_else(|| {
            failure(
                "ai.refinement_context",
                "No canonical decisive case is available",
            )
        })?;
    let value = Input {
        bundle_id: input.bundle_id.clone(),
        instance_id: anchor.into(),
        model_revision_sha256: input.model_revision_sha256.clone(),
        redact_values: false,
        question: String::new(),
        instruction: input.instruction.clone(),
    };
    validate(&value, true)?;
    Ok(value)
}

/// Hash source, all bins and unchanged user intent together; no silent context truncation is allowed.
fn packet(input: &RefinementInput, prepared: &Value) -> Result<context::Prompt> {
    let references = references(prepared)?;
    let value = json!({"task":"feedback_refinement","feedback_sha256":prepared["feedback_sha256"],
        "original_instruction":input.instruction,"canonical":prepared["packet"],
        "requirements":"Preserve Yes observations on their original inputs. Exclude No observations on those inputs. Unsure is advisory, not a constraint. Preserve the exact output schema, catalog, parameters, property, bounds, domain and execution settings. Do not weaken requirements or claim success; deterministic checks decide readiness."});
    let bytes = serde_json::to_vec(&value).unwrap();
    if bytes.len() > 131_072 {
        return Err(failure(
            "ai.context_size",
            "Complete refinement context exceeds 128 KiB",
        ));
    }
    Ok(context::Prompt {
        value,
        hash: format!("{:x}", Sha256::digest(&bytes)),
        references,
    })
}

/// Canonical JSON Pointers must resolve before the model receives them as available citations.
fn references(prepared: &Value) -> Result<BTreeSet<String>> {
    let items = prepared["references"]
        .as_array()
        .ok_or_else(|| failure("ai.refinement_context", "Canonical references are missing"))?;
    if items.len() > 512 {
        return Err(failure(
            "ai.reference_limit",
            "Complete citation inventory exceeds 512 references",
        ));
    }
    let mut result = BTreeSet::new();
    for item in items {
        let reference = item
            .as_str()
            .filter(|value| prepared["packet"].pointer(value).is_some())
            .ok_or_else(|| {
                failure(
                    "ai.refinement_reference",
                    "Canonical reference does not resolve",
                )
            })?;
        result.insert(reference.to_owned());
    }
    Ok(result)
}

#[cfg(test)]
mod tests;

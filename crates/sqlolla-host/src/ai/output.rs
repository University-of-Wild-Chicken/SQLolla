//! Provider output remains untrusted text; IDs, payload sizes and references are independently checked.
use super::*;
use serde::Serialize;

/// One prose claim cites exact canonical fact identifiers without acquiring proof authority.
#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Claim {
    pub text: String,
    pub fact_references: Vec<String>,
}
/// A strict assistant document cannot carry replacement properties, bounds, context or solver status.
#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Draft {
    instance_id: String,
    model_revision_sha256: String,
    context_sha256: String,
    title: String,
    claims: Vec<Claim>,
    assumptions: Vec<String>,
    pub sql: Option<String>,
}

/// Validate transport identity and every cited fact, without pretending to prove natural-language truth.
pub(super) fn validate(
    value: Value,
    input: &Input,
    prompt: &context::Prompt,
    proposal: bool,
) -> Result<Draft> {
    let draft: Draft = serde_json::from_value(value).map_err(|_| {
        failure(
            "ai.output_shape",
            "Assistant output has an invalid structure",
        )
    })?;
    identity(&draft, input, prompt)?;
    contents(&draft, prompt)?;
    match (&draft.sql, proposal) {
        (Some(sql), true) => bounded_text(sql, 16_384)?,
        (None, false) => (),
        _ => {
            return Err(failure(
                "ai.output_kind",
                "Assistant output changed the requested operation",
            ));
        }
    }
    Ok(draft)
}

/// Bind assistant text to the exact selected evidence and prompt bytes.
fn identity(draft: &Draft, input: &Input, prompt: &context::Prompt) -> Result<()> {
    if draft.instance_id != input.instance_id
        || draft.model_revision_sha256 != input.model_revision_sha256
        || draft.context_sha256 != prompt.hash
    {
        return Err(failure(
            "ai.output_identity",
            "Assistant output does not match the supplied evidence",
        ));
    }
    Ok(())
}

/// Validate text budgets and each canonical fact citation.
fn contents(draft: &Draft, prompt: &context::Prompt) -> Result<()> {
    bounded_text(&draft.title, 160)?;
    if draft.claims.len() > 8 || draft.assumptions.len() > 8 {
        return Err(failure(
            "ai.output_size",
            "Too many assistant claims or assumptions",
        ));
    }
    for claim in &draft.claims {
        valid_claim(claim, prompt)?;
    }
    for assumption in &draft.assumptions {
        bounded_text(assumption, 512)?;
    }
    Ok(())
}

/// Reject invented or empty citation lists without assigning truth authority to the prose.
fn valid_claim(claim: &Claim, prompt: &context::Prompt) -> Result<()> {
    bounded_text(&claim.text, 1200)?;
    if claim.fact_references.is_empty()
        || claim.fact_references.len() > 8
        || claim
            .fact_references
            .iter()
            .any(|item| !prompt.references.contains(item))
    {
        return Err(failure(
            "ai.output_reference",
            "Assistant claim cites an absent fact",
        ));
    }
    Ok(())
}

/// Plain text rejects NUL and control characters that could obscure UI or log boundaries.
pub(super) fn bounded_text(value: &str, limit: usize) -> Result<()> {
    if value.trim().is_empty()
        || value.len() > limit
        || value
            .chars()
            .any(|ch| ch.is_control() && !matches!(ch, '\n' | '\r' | '\t'))
    {
        return Err(failure(
            "ai.output_text",
            "Assistant text exceeds the allowed text profile",
        ));
    }
    Ok(())
}

/// Fixed host-owned labels keep model prose distinct from deterministic evidence and safety claims.
pub(super) fn envelope(
    input: &Input,
    prompt: &context::Prompt,
    draft: &Draft,
    proposal: bool,
) -> Value {
    json!({"schema":if proposal {"sqlolla.ai.proposal.v1"} else {"sqlolla.ai.explanation.v1"},
        "status":"ok","provider":"openai","model":MODEL,"bundle_id":input.bundle_id,
        "instance_id":input.instance_id,"model_revision_sha256":input.model_revision_sha256,
        "context_sha256":prompt.hash,"redact_values":input.redact_values,"document":draft,
        "authority":"untrusted_assistance","claim":"Fact references are checked for existence; prose and business intent are not proved. Bounded results do not establish universal safety."})
}

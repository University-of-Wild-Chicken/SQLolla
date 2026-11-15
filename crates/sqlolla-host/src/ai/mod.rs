//! Optional untrusted assistance over reloaded canonical evidence; no application database is contacted.
mod context;
mod output;
mod provider;
mod refinement;
mod schema;
#[cfg(test)]
mod tests;
use crate::state::HostState;
use axum::{
    Json, Router,
    extract::State,
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    routing::post,
};
use serde::Deserialize;
use serde_json::{Value, json};

/// Every live production request uses this exact model with no provider or model fallback.
pub const MODEL: &str = "gpt-6.1-sol";
const ENDPOINT: &str = "https://api.openai.com/v1/responses";
type Result<T> = std::result::Result<T, AiError>;

/// A provider failure never carries response bodies, authorization headers or credentials outward.
#[derive(Debug)]
pub struct AiError {
    code: &'static str,
    message: &'static str,
    status: StatusCode,
}
impl IntoResponse for AiError {
    /// Render only fixed diagnostic strings controlled by this host.
    fn into_response(self) -> Response {
        (
            self.status,
            Json(json!({"status":"error","code":self.code,"message":self.message})),
        )
            .into_response()
    }
}

/// Construct a stable sanitized admission/provider diagnostic.
fn failure(code: &'static str, message: &'static str) -> AiError {
    AiError {
        code,
        message,
        status: StatusCode::BAD_REQUEST,
    }
}

/// Browser requests identify retained evidence rather than supplying model-visible facts.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Input {
    bundle_id: String,
    instance_id: String,
    model_revision_sha256: String,
    redact_values: bool,
    #[serde(default)]
    question: String,
    #[serde(default)]
    instruction: String,
}

/// Expose assistance only behind the host's origin, nonce, body and admission middleware.
pub fn router() -> Router<HostState> {
    Router::new()
        .route("/api/ai/explain", post(explain))
        .route("/api/ai/propose", post(proposal_requires_feedback))
        .route("/api/ai/refine", post(refinement::refine))
}

/// Explain one exact saved instance without mutating solver evidence or review feedback.
async fn explain(
    State(state): State<HostState>,
    headers: HeaderMap,
    Json(input): Json<Input>,
) -> Result<Json<Value>> {
    run(state, headers, input).await.map(Json)
}

/// Legacy unlabelled regeneration cannot bypass the decisive-feedback admission gate.
async fn proposal_requires_feedback() -> AiError {
    failure(
        "ai.feedback_required",
        "Label at least one case Yes or No, then use feedback-driven refinement",
    )
}

/// Release the pipeline worker during network I/O; keys exist only in this invocation's request.
async fn run(state: HostState, headers: HeaderMap, input: Input) -> Result<Value> {
    validate(&input, false)?;
    let key = credential(&headers)?;
    let evidence = context::load(&state, &input).await?;
    let prompt = context::build(&evidence, input.redact_values)?;
    let document = provider::request(key, &input, &prompt, false).await?;
    let draft = output::validate(document, &input, &prompt, false)?;
    Ok(output::envelope(&input, &prompt, &draft, false))
}

/// IDs, prompts and redaction are admission inputs, never instructions to change immutable context.
fn validate(input: &Input, proposal: bool) -> Result<()> {
    validate_ids(input)?;
    validate_prompt(input, proposal)
}

/// Admit only fixed-width canonical hash identifiers.
fn validate_ids(input: &Input) -> Result<()> {
    for id in [
        &input.bundle_id,
        &input.instance_id,
        &input.model_revision_sha256,
    ] {
        if id.len() != 64 || !id.bytes().all(|byte| byte.is_ascii_hexdigit()) {
            return Err(failure(
                "ai.identity",
                "Expected canonical evidence identifiers",
            ));
        }
    }
    Ok(())
}

/// Enforce request-kind and disclosure boundaries before evidence loading or provider egress.
fn validate_prompt(input: &Input, proposal: bool) -> Result<()> {
    if input.question.len() > 4096 || input.instruction.len() > 4096 {
        return Err(failure(
            "ai.prompt_size",
            "Assistance prompt exceeds 4096 bytes",
        ));
    }
    if proposal
        && (input.redact_values
            || input.instruction.trim().is_empty()
            || !input.question.is_empty())
    {
        return Err(failure(
            "ai.proposal_consent",
            "SQL proposals require an instruction and explicit unredacted context",
        ));
    }
    if !proposal && !input.instruction.is_empty() {
        return Err(failure(
            "ai.request_kind",
            "Explanation requests cannot carry proposal instructions",
        ));
    }
    Ok(())
}

/// Read a bounded bearer credential without logging, returning, persisting or deriving evidence from it.
fn credential(headers: &HeaderMap) -> Result<&str> {
    let key = headers
        .get("authorization")
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.strip_prefix("Bearer "))
        .ok_or_else(|| {
            failure(
                "ai.authorization",
                "Supply an OpenAI API key for this request",
            )
        })?;
    if key.is_empty() || key.len() > 8192 || !key.bytes().all(|byte| (33..=126).contains(&byte)) {
        return Err(failure(
            "ai.authorization",
            "Invalid bearer credential format",
        ));
    }
    Ok(key)
}

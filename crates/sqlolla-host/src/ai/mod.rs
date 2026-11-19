//! Optional untrusted assistance over reloaded canonical evidence; no application database is contacted.
mod catalog;
mod context;
mod draft;
mod model_draft;
mod output;
mod provider;
mod refinement;
mod rules;
mod schema;
#[cfg(test)]
mod tests;
use crate::state::HostState;
use axum::{
    Json, Router,
    extract::{Request, State},
    http::{HeaderMap, StatusCode},
    middleware::{self, Next},
    response::{IntoResponse, Response},
    routing::post,
};
use provider::{Byot, Provider};
use serde::Deserialize;
use serde_json::{Value, json};

pub use provider::MODEL;
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
        .nest(
            "/api/ai",
            tasks().route("/propose", post(proposal_requires_feedback)),
        )
        .nest(
            "/api/ai/byot",
            tasks()
                .route("/models", post(catalog::list))
                .route_layer(middleware::from_fn(require_selection)),
        )
}

/// Separate explicit BYOT routes from legacy endpoints that know only the OpenAI key.
fn tasks() -> Router<HostState> {
    Router::new()
        .route("/draft", post(draft::generate))
        .route("/model-draft", post(model_draft::generate))
        .route("/explain", post(explain))
        .route("/refine", post(refinement::refine))
}

/// Refuse a missing provider header on the BYOT routes before any credential can be forwarded.
async fn require_selection(request: Request, next: Next) -> Result<Response> {
    if !request.headers().contains_key("x-sqlolla-provider") {
        return Err(failure("ai.provider", "Choose a supported AI provider"));
    }
    Ok(next.run(request).await)
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
    let byot = Byot::from_headers(&headers)?;
    let evidence = context::load(&state, &input).await?;
    let prompt = context::build(&evidence, input.redact_values)?;
    let document = provider::request(byot, &input, &prompt, false).await?;
    let draft = output::validate(document, &input, &prompt, false)?;
    Ok(output::envelope(byot, &input, &prompt, &draft, false))
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
    let mut values = headers.get_all("authorization").iter();
    let key = values
        .next()
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.strip_prefix("Bearer "))
        .ok_or_else(|| {
            failure(
                "ai.authorization",
                "Supply an API key for the selected provider",
            )
        })?;
    if values.next().is_some()
        || key.is_empty()
        || key.len() > 8192
        || !key.bytes().all(|byte| (33..=126).contains(&byte))
    {
        return Err(failure(
            "ai.authorization",
            "Invalid bearer credential format",
        ));
    }
    Ok(key)
}

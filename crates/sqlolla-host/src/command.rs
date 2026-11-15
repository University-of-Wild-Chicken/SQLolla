//! Thin byte-preserving command forwarding; this layer contains no SQL parser or compiler.
use crate::{MODEL, error::HostError, state::HostState};
use axum::{Json, body::Bytes, extract::State};
use serde_json::{Value, json};

/// Expose only public readiness/model identity and the same-origin memory capability.
pub async fn status(State(state): State<HostState>) -> Json<Value> {
    Json(json!({"schema":"sqlolla.host.v1", "model":MODEL, "csrf_token":state.csrf_token()}))
}

/// Pass the complete original request bytes to the sole serialized canonical pipeline.
pub async fn command(
    State(state): State<HostState>,
    bytes: Bytes,
) -> Result<Json<Value>, HostError> {
    state.dispatch(bytes.to_vec()).await.map(Json)
}

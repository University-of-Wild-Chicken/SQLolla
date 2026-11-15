//! Host transport failures stay separate from SQL, solver and enumeration results.
use axum::{
    Json,
    http::StatusCode,
    response::{IntoResponse, Response},
};
use serde_json::json;

/// Stable host-only diagnostic; messages must never contain credentials or request bodies.
#[derive(Debug)]
pub struct HostError {
    /// Machine-readable transport diagnostic.
    pub code: &'static str,
    /// Sanitized explanation, separate from canonical pipeline output.
    pub message: String,
    status: StatusCode,
}

impl HostError {
    /// Construct a transport error without converting it into a solver verdict.
    pub fn new(status: StatusCode, code: &'static str, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
            status,
        }
    }
}

impl IntoResponse for HostError {
    /// Errors never acquire an enumeration-exhausted or semantic-success label.
    fn into_response(self) -> Response {
        (
            self.status,
            Json(json!({"schema":"sqlolla.host_error.v1", "status":"error",
            "code":self.code,"message":self.message})),
        )
            .into_response()
    }
}

impl std::fmt::Display for HostError {
    /// Display only the already-sanitized host diagnostic.
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "{}: {}", self.code, self.message)
    }
}

impl std::error::Error for HostError {}

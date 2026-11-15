//! Durable typed examples/counterexamples with on-demand Alloy enumeration and independent replay.
mod artifact;
mod prepared;
mod session;
mod store;
use crate::pipeline::{Failure, Outcome, Pipeline, Request, engine, fingerprint};
use artifact::{Bundle, Cursor, Record, StepStatus};
use prepared::Prepared;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use store::Store;

/// Bounded journal size is an explicit resource limit, never an exhaustion claim.
const MAX_INSTANCES: usize = 256;

/// Instance command identity is distinct from the interpretation of its solver result.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum Kind {
    Example,
    Counterexample,
}

/// Strict command dispatch never accepts a cursor as a filesystem path or an executable model.
#[derive(Deserialize)]
#[serde(tag = "action", deny_unknown_fields)]
enum Action {
    #[serde(rename = "instances.start")]
    Start {
        #[serde(default)]
        id: Value,
        #[serde(deserialize_with = "request_from_json")]
        request: Box<Request>,
        kind: Kind,
        #[serde(default = "default_limit")]
        max_instances: usize,
    },
    #[serde(rename = "instances.next")]
    Next {
        #[serde(default)]
        id: Value,
        cursor: String,
    },
    #[serde(rename = "instances.open")]
    Open {
        #[serde(default)]
        id: Value,
        bundle_id: String,
    },
}

/// Handle the additive offline evidence protocol without changing existing analysis requests.
pub(crate) fn handle(pipeline: &mut Pipeline, bytes: &[u8]) -> Value {
    let action = match serde_json::from_slice::<Action>(bytes) {
        Ok(value) => value,
        Err(error) => return error_response(Value::Null, json_error(error)),
    };
    let id = match &action {
        Action::Start { id, .. } | Action::Next { id, .. } | Action::Open { id, .. } => id.clone(),
    };
    let result = Store::open().and_then(|store| match action {
        Action::Start {
            request,
            kind,
            max_instances,
            ..
        } => session::start(pipeline, &store, *request, kind, max_instances),
        Action::Next { cursor, .. } => session::next(pipeline, &store, &cursor),
        Action::Open { bundle_id, .. } => session::open(pipeline, &store, &bundle_id),
    });
    match result {
        Ok(mut value) => {
            value["id"] = id;
            value
        }
        Err(error) => error_response(id, error),
    }
}

/// Evidence failures preserve error/resource classifications and never assert exhaustion.
fn error_response(id: Value, error: Failure) -> Value {
    json!({"id":id,"schema":"sqlolla.enumeration_response.v1","status":"error","code":error.0,
        "message":error.1,"enumeration_exhausted":false})
}

/// Default page budget permits several examples without claiming complete enumeration.
fn default_limit() -> usize {
    64
}

/// Stable diagnostics distinguish protocol, archive, replay and solver failures.
fn failure(code: &str, message: impl Into<String>) -> Failure {
    (code.into(), message.into())
}

/// Deserialization failures never silently drop fields from a versioned archive or request.
fn json_error(error: serde_json::Error) -> Failure {
    failure("evidence.json", error.to_string())
}

/// Restore JSON's numeric object-key decoder after serde's tagged-enum buffering.
fn request_from_json<'de, D>(deserializer: D) -> Result<Box<Request>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let value = Value::deserialize(deserializer)?;
    serde_json::from_value(value)
        .map(Box::new)
        .map_err(serde::de::Error::custom)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Numbered SQL parameters survive the additive internally tagged action envelope.
    #[test]
    fn numbered_parameter_transport() {
        let wire = json!({"action":"instances.start","kind":"example","request":{
            "sql":"SELECT $1","catalog":{"tables":{},"search_path":["public"]},
            "parameters":{"1":{"type":"int32","value":7}}}});
        let action: Action = serde_json::from_slice(&serde_json::to_vec(&wire).unwrap()).unwrap();
        let Action::Start { request, .. } = action else {
            panic!("Expected start action");
        };
        assert_eq!(request.parameters[&1], sqlolla_core::Cell::Int32(7));
    }
}

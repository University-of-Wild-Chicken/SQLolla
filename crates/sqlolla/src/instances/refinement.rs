//! Validate a canonical review snapshot and check SQL under its unchanged analysis basis.
mod observation;
mod replay;
mod snapshot;
#[cfg(test)]
mod tests;

use super::*;
use snapshot::Snapshot;

const SCHEMA: &str = "sqlolla.refinement_response.v1";

/// An active label either constrains candidate behavior or supplies advisory context.
#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
enum Disposition {
    AcceptedUseCase,
    UnintendedBehavior,
    Unsure,
}

/// Browser input supplies decisions and identities; typed observations come from replayed archives.
#[derive(Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Label {
    bundle_id: String,
    instance_id: String,
    model_revision_sha256: String,
    semantic_sha256: String,
    event_id: String,
    disposition: Disposition,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    rationale: Option<String>,
}

/// Context generation and checking share the same snapshot admission.
#[derive(Clone, Copy, Deserialize)]
enum Command {
    #[serde(rename = "refinement.context")]
    Context,
    #[serde(rename = "refinement.check")]
    Check,
}

/// Candidate fields are valid only for a check against a specific canonical snapshot.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Input {
    action: Command,
    #[serde(default, rename = "id")]
    _id: Value,
    bundle_id: String,
    model_revision_sha256: String,
    feedback: Vec<Label>,
    sql: Option<String>,
    feedback_sha256: Option<String>,
}

/// Dispatch refinement without mutating review history, source bundles or candidate SQL.
pub(crate) fn handle(pipeline: &mut Pipeline, raw: &Value, bytes: &[u8]) -> Value {
    let id = raw["id"].clone();
    let result = serde_json::from_slice::<Input>(bytes)
        .map_err(json_error)
        .and_then(|input| run(pipeline, input));
    match result {
        Ok(mut value) => {
            value["id"] = id;
            value
        }
        Err((code, message)) => {
            json!({"schema":SCHEMA,"id":id,"status":"error","code":code,"message":message,"ready":false})
        }
    }
}

/// Require the canonical digest again before binding a potentially stale candidate.
fn run(pipeline: &mut Pipeline, mut input: Input) -> Outcome<Value> {
    let check = match (input.action, input.sql.take(), input.feedback_sha256.take()) {
        (Command::Context, None, None) => None,
        (Command::Check, Some(sql), Some(hash)) if identifier(&hash) => Some((sql, hash)),
        _ => {
            return Err(failure(
                "refinement.protocol",
                "Expected candidate SQL and feedback digest only for a check",
            ));
        }
    };
    snapshot::validate(&mut input)?;
    let store = Store::open()?;
    let snap = Snapshot::load(pipeline, &store, input)?;
    match check {
        None => snap.context(),
        Some((sql, digest)) => {
            if snap.feedback_sha256 != digest {
                return Err(failure(
                    "refinement.snapshot_changed",
                    "Feedback snapshot changed",
                ));
            }
            replay::check(pipeline, &snap, sql)
        }
    }
}

/// Validate externally supplied archive and snapshot hashes before lookup.
fn identifier(hash: &str) -> bool {
    hash.len() == 64
        && hash
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

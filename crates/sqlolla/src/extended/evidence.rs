//! Evidence distinguishes independently encoded RA from enumerated interpreter transitions.
use crate::alloy::{self, Property};
use crate::finite::{FiniteModel, Transition};
use crate::pipeline::{Request, fingerprint, hash};
use serde_json::{Value, json};
use sqlolla_core::{ExecutionContext, StatementOutcome, StatementPlan};

/// Record the actual assertion command separately from satisfiable model assumptions.
pub(super) fn analysis_fields(result: &mut Value, request: &Request) {
    result["assumptions"] = json!("SATISFIABLE");
    result["property_check"] =
        json!({"command":"check", "assertion":"QueryProperty", "command_index":1});
    result["bounds"] = json!(request.bounds);
    result["domain"] = json!(request.domain);
    result["property"] = json!(request.property);
}

/// Bind all semantic context before attaching generated results or mutable solver instances.
pub(super) fn fields(
    request: &Request,
    plan: &StatementPlan,
    context: &ExecutionContext,
    encoded: &FiniteModel,
) -> Value {
    let mut result = json!({"profile":"sqlolla.explicit_outcomes.v2", "mode":request.mode,
        "sql_sha256":hash(request.sql.as_bytes()), "catalog_sha256":fingerprint(&request.catalog),
        "parameters_sha256":fingerprint(&request.parameters), "execution_sha256":fingerprint(context),
        "runtime_step_limit":sqlolla_core::MAX_EXECUTION_STEPS,
        "input_sha256":fingerprint(&json!({"database":request.database,"bounds":request.bounds,
            "domain":request.domain,"property":request.property}))});
    result["context_sha256"] = json!(fingerprint(&result));
    result["status"] = json!("ok");
    result["encoding"] = json!("RUST_EVALUATOR_BACKED_FINITE_TABLE");
    result["claim"] = json!(
        "complete evaluator outcomes within supplied finite context; not independent SQL semantic verification"
    );
    result["schema"] = json!(super::schema(plan));
    result["ir_sha256"] = json!(fingerprint(plan));
    result["model_sha256"] = json!(hash(encoded.model.as_bytes()));
    result["transition_payload_sha256"] = json!(fingerprint(&encoded.transitions));
    result["transition_count"] = json!(encoded.transitions.len());
    result["complete"] = json!(true);
    if request.include_model {
        result["model"] = json!(encoded.model);
        result["ir"] = json!(plan);
    }
    result
}

/// Error outcomes remain visible to the no-error policy and to retained-output policies.
pub(crate) fn violated(property: &Property, outcome: &StatementOutcome) -> bool {
    match outcome {
        StatementOutcome::Succeeded(value) => alloy::violated(property, &value.rows),
        StatementOutcome::SqlError(value) => {
            matches!(property, Property::NoSqlError)
                || alloy::violated(property, &value.exposed_rows)
        }
    }
}

/// Attach one replayed complete transition to its solver-selected instance identity.
pub(super) fn witness(result: &mut Value, transition: &Transition, response: &Value) {
    let instance = json!({"kind":"counterexample", "database":transition.input, "catalog":transition.input_catalog,
        "outcome":transition.outcome,"validation":"RUST_REPLAY_CONFIRMED",
        "relations":response["relations"],"complete":true,
        "completeness_scope":"all enumerated evaluator outcomes under the exact supplied finite context"});
    result["analysis"] = json!("COUNTEREXAMPLE");
    result["instance_sha256"] = json!(fingerprint(&instance));
    result["instance"] = instance;
}

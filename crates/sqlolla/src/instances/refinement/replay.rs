//! Candidate acceptance combines a real bounded Alloy check and complete reviewed-input replay.
use super::observation::{Observation, canonical};
use super::snapshot::{Review, Snapshot};
use super::*;
use crate::pipeline::hash;
use prepared::Model;
use sqlolla_core::{Database, StatementOutcome, admissible_initial_state, evaluate_with_catalog};
use std::collections::BTreeMap;

/// Bind only the candidate SQL and issue a new-analysis request only for a ready candidate.
pub(super) fn check(pipeline: &mut Pipeline, snap: &Snapshot, sql: String) -> Outcome<Value> {
    let candidate_sql_sha256 = hash(sql.as_bytes());
    let prepared = Prepared::candidate(pipeline, &snap.base, sql)?;
    let assumptions = session::assumptions(pipeline, &prepared)?;
    let bundle = Bundle::new(
        &prepared,
        Kind::Counterexample,
        snap.base.max_instances,
        &assumptions,
    )?;
    let property = property_check(pipeline, &prepared, &bundle)?;
    let cases = check_cases(&prepared, &snap.cases);
    let ready = property["status"] == "NO_COUNTEREXAMPLE_WITHIN_BOUNDS"
        && cases
            .iter()
            .all(|case| matches!(case["status"].as_str(), Some("satisfied" | "advisory")));
    let mut response = snap.response();
    response["candidate_sql_sha256"] = json!(candidate_sql_sha256);
    response["candidate_model_revision_sha256"] = json!(bundle.model_revision_sha256);
    response["property_check"] = property;
    response["cases"] = json!(cases);
    response["ready"] = json!(ready);
    if ready {
        response["candidate_request"] = json!(prepared.request);
    }
    Ok(response)
}

/// Require an actual QueryProperty check and independently replay any selected counterexample.
fn property_check(pipeline: &mut Pipeline, prepared: &Prepared, bundle: &Bundle) -> Outcome<Value> {
    let solution = pipeline
        .bridge
        .check(prepared.source(), 1)
        .map_err(engine)?;
    let mut result = json!({"status":"NO_COUNTEREXAMPLE_WITHIN_BOUNDS", "assumptions":"SATISFIABLE",
        "property":prepared.request.property,"assurance_kind":bundle.assurance_kind,
        "model_sha256":hash(prepared.source().as_bytes()),"solver":artifact::solver_metadata(&solution)});
    if solution["status"] == "SAT" {
        let semantic = prepared.replay(&solution, Kind::Counterexample)?;
        result["status"] = json!("COUNTEREXAMPLE");
        result["witness"] = json!(Observation::decode(bundle, &semantic)?);
    }
    Ok(result)
}

/// Check every active constraint; several labels may share one exact concrete input.
fn check_cases(prepared: &Prepared, reviews: &[Review]) -> Vec<Value> {
    let mut cache = BTreeMap::new();
    reviews
        .iter()
        .map(|review| {
            let mut result = json!(review.label);
            result.as_object_mut().unwrap().remove("rationale");
            if review.label.disposition == Disposition::Unsure {
                result["status"] = json!("advisory");
                return result;
            }
            let outcomes = cache
                .entry(&review.observation.database)
                .or_insert_with(|| evaluate(prepared, &review.observation.database));
            let assessment = assessments(review, outcomes);
            result
                .as_object_mut()
                .unwrap()
                .extend(assessment.as_object().unwrap().clone());
            result
        })
        .collect()
}

/// Retain complete outcome sets and ensure finite-table candidates cover the original input.
fn evaluate(prepared: &Prepared, database: &Database) -> Outcome<Vec<StatementOutcome>> {
    let catalog = &prepared.request.catalog;
    let context = &prepared.context;
    let valid =
        admissible_initial_state(catalog, database, context).map_err(|e| (e.code, e.message))?;
    if !valid {
        return Err(failure(
            "refinement.input_inadmissible",
            "Reviewed input is inadmissible under the candidate",
        ));
    }
    let values = evaluate_with_catalog(&prepared.statement, catalog, database, context)
        .map_err(|e| (e.code, e.message))?
        .outcomes;
    let outcomes: Vec<_> = values.into_iter().map(canonical).collect();
    if outcomes.is_empty() {
        return Err(failure(
            "refinement.empty_replay",
            "Reviewed input has no complete candidate outcome",
        ));
    }
    verify_inventory(&prepared.model, database, &outcomes)?;
    Ok(outcomes)
}

/// Re-evaluation and the complete finite encoding must agree for the reviewed input.
fn verify_inventory(
    model: &Model,
    database: &Database,
    outcomes: &[StatementOutcome],
) -> Outcome<()> {
    let Model::Finite(model) = model else {
        return Ok(());
    };
    let encoded = model
        .transitions
        .iter()
        .filter(|transition| &transition.input == database)
        .map(|transition| canonical(transition.outcome.clone()));
    if !outcomes.iter().cloned().eq(encoded) {
        return Err(failure(
            "refinement.incomplete_replay",
            "Reviewed input differs from the complete bounded candidate inventory",
        ));
    }
    Ok(())
}

/// Acceptance requires one exact behavior; rejection requires zero across the entire set.
pub(super) fn assessments(review: &Review, replay: &Outcome<Vec<StatementOutcome>>) -> Value {
    let outcomes = match replay {
        Ok(outcomes) if !outcomes.is_empty() => outcomes,
        Ok(_) => return json!({"status":"refused","code":"refinement.empty_replay"}),
        Err((code, message)) => return json!({"status":"refused","code":code,"message":message}),
    };
    let matching = outcomes
        .iter()
        .filter(|outcome| **outcome == review.observation.outcome)
        .count();
    let reason = match (review.label.disposition, matching) {
        (Disposition::AcceptedUseCase, 0) => Some("accepted_observation_missing"),
        (Disposition::UnintendedBehavior, 1..) => Some("rejected_observation_still_possible"),
        _ => None,
    };
    json!({"status":if reason.is_some() {"unsatisfied"} else {"satisfied"},"reason":reason,
        "candidate_outcome_count":outcomes.len(),"matching_outcome_count":matching})
}

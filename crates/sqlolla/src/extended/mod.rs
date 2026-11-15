//! Complete finite outcome exploration with an explicitly evaluator-backed SAT encoding.
pub(crate) mod evidence;
mod worlds;
use crate::bridge::Bridge;
use crate::finite::{self, FiniteModel, Transition};
use crate::pipeline::{Outcome, Request, encoding, engine};
use serde_json::{Value, json};
use sqlolla_core::{Column, ExecutionContext, StatementPlan};

/// Dispatch the extended profile without relabeling a table roundtrip as independent execution.
pub(crate) fn run(bridge: &mut Bridge, request: &Request, plan: &StatementPlan) -> Outcome<Value> {
    let context = catalog_context(request)?;
    let transitions = transitions(request, plan, &context)?;
    let encoded =
        finite::encode(transitions, schema(plan), request.property.as_ref()).map_err(encoding)?;
    let mut result = evidence::fields(request, plan, &context, &encoded);
    if request.mode == "compile" {
        result["model"] = json!(encoded.model);
        result["ir"] = json!(plan);
        return Ok(result);
    }
    let selected = solve(bridge, &encoded, 0)?
        .ok_or_else(|| engine("complete finite outcome table unexpectedly UNSAT".into()))?;
    result["alloy_status"] = json!("SAT");
    result["selected_transition"] = json!(selected.0);
    if request.mode == "analyze" {
        analyze(bridge, request, plan, &context, &encoded, &mut result)?;
    } else {
        result["outcomes"] = json!(
            encoded
                .transitions
                .iter()
                .map(|t| &t.outcome)
                .collect::<Vec<_>>()
        );
        result["validation"] = json!("ALLOY_TABLE_ROUNDTRIP");
    }
    Ok(result)
}

/// Merge persistent catalog constraints with explicitly supplied execution metadata.
pub(crate) fn catalog_context(request: &Request) -> Outcome<ExecutionContext> {
    sqlolla_core::merged_catalog_context(
        &request.catalog,
        &request.execution.clone().unwrap_or_default(),
    )
    .map_err(|error| (error.code, error.message))
}

/// Evaluate every admissible input; a capacity refusal discards the whole partial enumeration.
pub(crate) fn transitions(
    request: &Request,
    plan: &StatementPlan,
    context: &ExecutionContext,
) -> Outcome<Vec<Transition>> {
    let inputs = admissible_inputs(request, context)?;
    let mut transitions = Vec::new();
    for input in inputs {
        let result = sqlolla_core::evaluate_with_catalog(plan, &request.catalog, &input, context)
            .map_err(|e| (e.code, e.message))?;
        if result.outcomes.is_empty()
            || transitions.len().saturating_add(result.outcomes.len()) > 256
        {
            return Err((
                "resource.transitions".into(),
                "complete execution table exceeds 256 transitions or contains no outcome".into(),
            ));
        }
        transitions.extend(result.outcomes.into_iter().map(|outcome| Transition {
            input_catalog: Some(request.catalog.clone()),
            input: input.clone(),
            outcome,
        }));
    }
    Ok(transitions)
}

/// Filter only declared prestate constraints; failing statements remain execution outcomes.
fn admissible_inputs(
    request: &Request,
    context: &ExecutionContext,
) -> Outcome<Vec<sqlolla_core::Database>> {
    let inputs = if request.mode == "analyze" {
        worlds::enumerate(request)?
    } else {
        vec![request.database.clone()]
    };
    let mut admitted = Vec::new();
    for input in inputs {
        let valid = sqlolla_core::admissible_initial_state(&request.catalog, &input, context)
            .map_err(|e| (e.code, e.message))?;
        if valid {
            admitted.push(input);
        } else if request.mode != "analyze" {
            return Err((
                "invalid.initial_state".into(),
                "Concrete prestate violates a declared constraint".into(),
            ));
        }
    }
    if admitted.is_empty() {
        return Err(encoding(
            "No admissible initial state within the exact declared domain".into(),
        ));
    }
    Ok(admitted)
}

/// Check assumptions separately from the violation search, including possible SQL failures.
fn analyze(
    bridge: &mut Bridge,
    request: &Request,
    plan: &StatementPlan,
    context: &ExecutionContext,
    encoded: &FiniteModel,
    result: &mut Value,
) -> Outcome<()> {
    evidence::analysis_fields(result, request);
    let Some((index, response)) = solve(bridge, encoded, 1)? else {
        result["analysis"] = json!("NO_COUNTEREXAMPLE_WITHIN_BOUNDS");
        return Ok(());
    };
    let transition = &encoded.transitions[index];
    replay(plan, context, transition)?;
    let property = request
        .property
        .as_ref()
        .ok_or_else(|| encoding("missing analysis property".into()))?;
    if !evidence::violated(property, &transition.outcome) {
        return Err(engine(
            "selected finite transition does not violate property".into(),
        ));
    }
    evidence::witness(result, transition, &response);
    Ok(())
}

/// Unknown solver statuses are engine failures and never bounded safety claims.
fn solve(
    bridge: &mut Bridge,
    encoded: &FiniteModel,
    command: usize,
) -> Outcome<Option<(usize, Value)>> {
    let response = if command == 1 {
        bridge.check(&encoded.model, command)
    } else {
        bridge.solve(&encoded.model, command)
    }
    .map_err(engine)?;
    match response["status"].as_str() {
        Some("SAT") => Ok(Some((encoded.decode(&response).map_err(engine)?, response))),
        Some("UNSAT") => Ok(None),
        _ => Err(engine("unknown finite solver result".into())),
    }
}

/// Replay the complete observation, including state, ordering, SQL errors and effects.
pub(crate) fn replay(
    plan: &StatementPlan,
    context: &ExecutionContext,
    transition: &Transition,
) -> Outcome<()> {
    let catalog = transition
        .input_catalog
        .as_ref()
        .ok_or_else(|| engine("Missing replay catalog".into()))?;
    let replay = sqlolla_core::evaluate_with_catalog(plan, catalog, &transition.input, context)
        .map_err(|e| (e.code, e.message))?;
    if !replay.outcomes.contains(&transition.outcome) {
        return Err(engine("finite transition replay mismatch".into()));
    }
    Ok(())
}

/// Bound statement output types cover SELECT and every mutation's RETURNING projection.
pub(crate) fn schema(plan: &StatementPlan) -> &[Column] {
    match plan {
        StatementPlan::Initialized { body, .. } => schema(body),
        StatementPlan::With(plan) => schema(&plan.body),
        StatementPlan::Query(plan) => &plan.schema,
        StatementPlan::LockedQuery(plan) => &plan.schema,
        StatementPlan::Insert(plan) => &plan.returning.schema,
        StatementPlan::Update(plan) => &plan.returning.schema,
        StatementPlan::Delete(plan) => &plan.returning.schema,
        StatementPlan::Rejected { .. } => &[],
        StatementPlan::CreateTable(_)
        | StatementPlan::CreateView(_)
        | StatementPlan::DropView(_) => &[],
    }
}

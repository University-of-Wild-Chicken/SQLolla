//! Reopening replays the exact retained prefix and refuses changed solver enumeration order.
use super::*;
use std::time::{Duration, Instant};

/// Persist one immutable revision and yield its first model on demand.
pub(super) fn start(
    pipeline: &mut Pipeline,
    store: &Store,
    request: Request,
    kind: Kind,
    limit: usize,
) -> Outcome<Value> {
    validate_start(&request, kind, limit)?;
    let prepared = Prepared::new(pipeline, request)?;
    let assumptions = assumptions(pipeline, &prepared)?;
    let bundle = Bundle::new(&prepared, kind, limit, &assumptions)?;
    let id = store.save_bundle(&bundle)?;
    let record = advance(pipeline, store, &bundle, &id, &prepared, 0)?;
    response(store, &id, &bundle, record)
}

/// A cursor is accepted only after its entire prior prefix has been regenerated and replayed.
pub(super) fn next(pipeline: &mut Pipeline, store: &Store, opaque: &str) -> Outcome<Value> {
    let cursor = store.cursor(opaque)?;
    if cursor.schema != "sqlolla.instance_cursor.v1" || cursor.next_ordinal == 0 {
        return Err(failure("cursor.invalid", "Cursor schema/ordinal mismatch"));
    }
    let bundle = store.bundle(&cursor.bundle_id)?;
    let previous = store
        .record(&cursor.bundle_id, cursor.next_ordinal - 1)?
        .ok_or_else(|| failure("cursor.invalid", "Cursor prefix missing"))?;
    if previous.status != StepStatus::Instance
        || fingerprint(&previous) != cursor.previous_record_sha256
    {
        return Err(failure("cursor.invalid", "Cursor prefix identity mismatch"));
    }
    let prepared = restore(pipeline, &bundle)?;
    let record = advance(
        pipeline,
        store,
        &bundle,
        &cursor.bundle_id,
        &prepared,
        cursor.next_ordinal,
    )?;
    response(store, &cursor.bundle_id, &bundle, record)
}

/// Load immutable context and already committed evidence without generating an unrequested next model.
pub(super) fn open(pipeline: &mut Pipeline, store: &Store, id: &str) -> Outcome<Value> {
    let bundle = store.bundle(id)?;
    let records = store.records(id, bundle.max_instances)?;
    let prepared = restore(pipeline, &bundle)?;
    let last = records.len() - 1;
    let record = advance(pipeline, store, &bundle, id, &prepared, last)?;
    let mut result = response(store, id, &bundle, record)?;
    result["records"] = json!(records);
    result["bundle"] = json!(bundle);
    Ok(result)
}

/// The command mode is fixed at creation; examples never acquire counterexample labels by inference.
fn validate_start(request: &Request, kind: Kind, limit: usize) -> Outcome<()> {
    if !(1..=MAX_INSTANCES).contains(&limit) {
        return Err(failure(
            "resource.instances",
            "Instance limit must be1..256",
        ));
    }
    if kind == Kind::Counterexample && (request.mode != "analyze" || request.property.is_none()) {
        return Err(failure(
            "evidence.property",
            "Counterexamples require analyze mode and an explicit property",
        ));
    }
    Ok(())
}

/// Rebinding and tool-identity validation precede every resumed solver step.
fn restore(pipeline: &mut Pipeline, bundle: &Bundle) -> Outcome<Prepared> {
    if bundle.schema != "sqlolla.evidence_bundle.v1" {
        return Err(failure("store.version", "Unknown bundle schema"));
    }
    validate_start(&bundle.request, bundle.kind, bundle.max_instances)?;
    let prepared = Prepared::new(pipeline, bundle.request.clone())?;
    let assumptions = assumptions(pipeline, &prepared)?;
    bundle.verify(&prepared, &assumptions)?;
    Ok(prepared)
}

/// Unsatisfiable assumptions are reported separately from an exhausted property check.
fn assumptions(pipeline: &mut Pipeline, prepared: &Prepared) -> Outcome<Value> {
    let result = pipeline
        .bridge
        .solve(prepared.source(), 0)
        .map_err(engine)?;
    if result["status"] != "SAT" || result["command_kind"] != "run" {
        return Err(failure(
            "evidence.assumptions",
            "Query assumptions have no model within declared bounds",
        ));
    }
    Ok(result)
}

/// Regenerate all earlier solver choices; nondeterministic prefix changes fail closed.
fn advance(
    pipeline: &mut Pipeline,
    store: &Store,
    bundle: &Bundle,
    id: &str,
    prepared: &Prepared,
    target: usize,
) -> Outcome<Record> {
    if target > bundle.max_instances {
        return Err(failure("cursor.invalid", "Cursor exceeds configured limit"));
    }
    let mut solution = begin(pipeline, bundle, prepared)?;
    let mut previous = id.to_owned();
    let began = Instant::now();
    for ordinal in 0..=target {
        replay_time(began)?;
        let record = step(bundle, prepared, &solution, ordinal, previous)?;
        committed(store, id, &record, ordinal < target)?;
        if ordinal == target {
            return Ok(record);
        }
        continuing(&record)?;
        previous = fingerprint(&record);
        if ordinal + 1 < bundle.max_instances {
            solution = pipeline.bridge.next_solution().map_err(engine)?;
        }
    }
    Err(engine("Enumeration loop did not produce a record".into()))
}

/// Only a genuine UNSAT next step is exhausted; capacity remains a separate terminal result.
fn step(
    bundle: &Bundle,
    prepared: &Prepared,
    solution: &Value,
    ordinal: usize,
    previous: String,
) -> Outcome<Record> {
    if ordinal == bundle.max_instances {
        return Ok(limit_record(ordinal, previous));
    }
    validate_command(solution, bundle.kind, ordinal)?;
    let (status, instance) = match solution["status"].as_str() {
        Some("SAT") => (
            StepStatus::Instance,
            Some(artifact::instance(
                bundle,
                prepared.replay(solution, bundle.kind)?,
                solution,
            )?),
        ),
        Some("UNSAT") => (StepStatus::Exhausted, None),
        _ => return Err(engine("Unexpected enumeration solver status".into())),
    };
    Ok(make_record(ordinal, previous, status, instance, solution))
}

/// Start a private API enumeration with its fixed archived command kind.
fn begin(pipeline: &mut Pipeline, bundle: &Bundle, prepared: &Prepared) -> Outcome<Value> {
    pipeline
        .bridge
        .begin(
            prepared.source(),
            usize::from(bundle.kind == Kind::Counterexample),
        )
        .map_err(engine)
}

/// Record complete solver metadata separately from the typed instance payload.
fn make_record(
    ordinal: usize,
    previous: String,
    status: StepStatus,
    instance: Option<Value>,
    solution: &Value,
) -> Record {
    Record {
        schema: "sqlolla.instance_record.v1".into(),
        ordinal,
        previous_record_sha256: previous,
        status,
        instance,
        solver: artifact::solver_metadata(solution),
    }
}

/// A limit record explicitly states that another solver step was not requested.
fn limit_record(ordinal: usize, previous: String) -> Record {
    Record {
        schema: "sqlolla.instance_record.v1".into(),
        ordinal,
        previous_record_sha256: previous,
        status: StepStatus::Limit,
        instance: None,
        solver: json!({"status":"NOT_REQUESTED","reason":"configured_instance_limit"}),
    }
}

/// Bounded resume work cannot be reinterpreted as a complete enumeration.
fn replay_time(began: Instant) -> Outcome<()> {
    if began.elapsed() > Duration::from_secs(45) {
        return Err(failure(
            "resource.resume_time",
            "Complete prefix replay exceeded time capacity",
        ));
    }
    Ok(())
}

/// A valid cursor cannot advance beyond an already exhausted or limited prefix.
fn continuing(record: &Record) -> Outcome<()> {
    if record.status != StepStatus::Instance {
        return Err(failure(
            "cursor.invalid",
            "Cursor passed a terminal journal record",
        ));
    }
    Ok(())
}

/// A property check cannot be replaced by a satisfiability run with a convenient label.
fn validate_command(solution: &Value, kind: Kind, ordinal: usize) -> Outcome<()> {
    let command = if kind == Kind::Counterexample {
        "check"
    } else {
        "run"
    };
    if solution["command_kind"] != command
        || solution["ordinal"] != ordinal
        || (kind == Kind::Counterexample && solution["command_label"] != "QueryProperty")
    {
        return Err(failure(
            "evidence.command",
            "Solver command or ordinal identity mismatch",
        ));
    }
    Ok(())
}

/// Prefix equality covers full typed observations and atom tuples, not only output row counts.
fn committed(store: &Store, id: &str, record: &Record, must_exist: bool) -> Outcome<()> {
    if let Some(old) = store.record(id, record.ordinal)? {
        if fingerprint(&old) != fingerprint(record) {
            return Err(failure(
                "resume.prefix_changed",
                "Regenerated solver prefix differs from committed evidence",
            ));
        }
        return Ok(());
    }
    if must_exist {
        return Err(failure(
            "cursor.invalid",
            "Requested prefix was never committed",
        ));
    }
    store.save_record(id, record)
}

/// Advancing cursors are created only for yielded instances, never errors, limits or exhaustion.
fn response(store: &Store, id: &str, bundle: &Bundle, record: Record) -> Outcome<Value> {
    let cursor = if record.status == StepStatus::Instance {
        Some(store.save_cursor(&Cursor {
            schema: "sqlolla.instance_cursor.v1".into(),
            bundle_id: id.into(),
            next_ordinal: record.ordinal + 1,
            previous_record_sha256: fingerprint(&record),
        })?)
    } else {
        None
    };
    Ok(
        json!({"schema":"sqlolla.enumeration_response.v1","status":"ok","bundle_id":id,
        "model_revision_sha256":bundle.model_revision_sha256,"assurance_kind":bundle.assurance_kind,
        "enumeration_status":record.status,"enumeration_exhausted":record.status==StepStatus::Exhausted,
        "cursor":cursor,"record":record,"max_instances":bundle.max_instances,
        "claim":"solver solutions within this exact immutable finite model; examples do not prove property safety"}),
    )
}

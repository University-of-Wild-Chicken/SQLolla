//! Review comparisons retain complete outcome identity and refuse unsafe input shortcuts.
use super::observation::{Observation, canonical};
use super::replay::assessments;
use super::snapshot::{Review, validate};
use super::*;
use sqlolla_core::{
    Catalog, Cell, EffectEvent, FailurePhase, ResultOrdering, SqlFailure, SqlType,
    StatementOutcome, StatementSuccess,
};

/// Construct an otherwise well-formed label with no browser-supplied behavior.
fn label(disposition: Disposition) -> Label {
    Label {
        bundle_id: "1".repeat(64),
        instance_id: "2".repeat(64),
        model_revision_sha256: "3".repeat(64),
        semantic_sha256: "4".repeat(64),
        event_id: "selected".into(),
        disposition,
        rationale: None,
    }
}

/// Build the common envelope used by pure command admission tests.
fn input(feedback: Vec<Label>) -> Input {
    Input {
        action: Command::Context,
        _id: Value::Null,
        bundle_id: "1".repeat(64),
        model_revision_sha256: "3".repeat(64),
        feedback,
        sql: None,
        feedback_sha256: None,
    }
}

/// Construct a typed SELECT observation with the caller's ordering semantics.
fn select(values: &[Cell], ordering: ResultOrdering) -> StatementOutcome {
    StatementOutcome::Succeeded(StatementSuccess {
        catalog: None,
        database: Default::default(),
        rows: values.iter().map(|value| vec![value.clone()]).collect(),
        ordering,
        affected: values.len() as u64,
        command_tag: format!("SELECT {}", values.len()),
        effects: vec![],
    })
}

/// Build a concrete review using an independently supplied typed outcome.
fn review(disposition: Disposition, outcome: StatementOutcome) -> Review {
    let catalog: Catalog =
        serde_json::from_value(json!({"tables":{},"search_path":["public"]})).unwrap();
    Review {
        label: label(disposition),
        observation: Observation {
            database: Default::default(),
            catalog,
            outcome,
        },
    }
}

/// Replay predicates distinguish admissible branches, even when another branch already matches.
#[test]
fn nondeterministic_outcomes() {
    let keep = select(&[Cell::Int32(0)], ResultOrdering::Bag);
    let lose = select(&[Cell::Int32(1)], ResultOrdering::Bag);
    let yes = review(Disposition::AcceptedUseCase, keep.clone());
    let no = review(Disposition::UnintendedBehavior, lose.clone());
    let both = Ok(vec![keep.clone(), lose]);
    assert_eq!(assessments(&yes, &both)["status"], "satisfied");
    assert_eq!(assessments(&no, &both)["status"], "unsatisfied");
    assert_eq!(assessments(&no, &Ok(vec![keep]))["status"], "satisfied");
    assert_eq!(assessments(&no, &Ok(vec![]))["status"], "refused");
}

/// Preserve typed NULL and multiplicity while treating only unordered displays as bags.
#[test]
fn bag_and_ordering() {
    let null = Cell::Null(SqlType::Int32);
    let number = Cell::Int32(1);
    let rows = [null.clone(), number.clone(), null.clone()];
    let reversed = [number.clone(), null.clone(), null.clone()];
    assert_eq!(
        canonical(select(&rows, ResultOrdering::Bag)),
        canonical(select(&reversed, ResultOrdering::Bag))
    );
    assert_ne!(
        canonical(select(&rows, ResultOrdering::Ordered)),
        canonical(select(&reversed, ResultOrdering::Ordered))
    );
    assert_ne!(
        canonical(select(&rows, ResultOrdering::Bag)),
        canonical(select(&[null, number], ResultOrdering::Bag))
    );
    assert_ne!(
        select(&[Cell::Null(SqlType::Int32)], ResultOrdering::Bag),
        select(&[Cell::Null(SqlType::Bool)], ResultOrdering::Bag)
    );
}

/// SQL diagnostic phase and retained effects are part of the exact rejected observation.
#[test]
fn errors_and_retained_effects() {
    let failure = SqlFailure {
        sqlstate: "22012".into(),
        phase: FailurePhase::Statement,
        constraint_name: None,
        column_name: None,
        exposed_rows: vec![],
        effects: vec![EffectEvent::SequenceValue {
            sequence: "public.s".into(),
            value: 1,
        }],
    };
    let rejected = review(
        Disposition::UnintendedBehavior,
        StatementOutcome::SqlError(failure.clone()),
    );
    let no_effects = StatementOutcome::SqlError(SqlFailure {
        effects: vec![],
        ..failure.clone()
    });
    let original = StatementOutcome::SqlError(failure);
    assert_eq!(
        assessments(&rejected, &Ok(vec![no_effects.clone()]))["status"],
        "satisfied"
    );
    assert_eq!(
        assessments(&rejected, &Ok(vec![no_effects, original]))["status"],
        "unsatisfied"
    );
}

/// Unsure is advisory, and duplicate effective evidence cannot count twice.
#[test]
fn active_dispositions() {
    let unsure = label(Disposition::Unsure);
    let yes = label(Disposition::AcceptedUseCase);
    assert_eq!(
        validate(&mut input(vec![])).unwrap_err().0,
        "refinement.feedback_required"
    );
    assert_eq!(
        validate(&mut input(vec![unsure.clone()])).unwrap_err().0,
        "refinement.feedback_required"
    );
    assert_eq!(
        validate(&mut input(vec![yes.clone(), yes.clone()]))
            .unwrap_err()
            .0,
        "refinement.duplicate_feedback"
    );
    let other = Label {
        instance_id: "5".repeat(64),
        event_id: "other".into(),
        ..unsure
    };
    assert!(validate(&mut input(vec![yes, other])).is_ok());
}

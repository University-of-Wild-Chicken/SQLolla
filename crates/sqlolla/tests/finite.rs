//! Actual Alloy API checks for complete explicit transition tables and hostile tuple edits.
use serde_json::json;
use sqlolla::{alloy::Property, bridge::Bridge, finite};
use sqlolla_core::*;

/// Build a successful observation with duplicate and nullable occurrences retained.
fn success(rows: Bag) -> StatementOutcome {
    StatementOutcome::Succeeded(StatementSuccess {
        catalog: None,
        database: Database::new(),
        affected: rows.len() as u64,
        command_tag: format!("SELECT {}", rows.len()),
        rows,
        ordering: ResultOrdering::Ordered,
        effects: Vec::new(),
    })
}

/// A SQL failure retains its own diagnostic and any exposed output.
fn failure() -> StatementOutcome {
    StatementOutcome::SqlError(SqlFailure {
        sqlstate: "22012".into(),
        phase: FailurePhase::Statement,
        constraint_name: None,
        column_name: None,
        exposed_rows: vec![vec![Cell::Int32(8)]],
        effects: Vec::new(),
    })
}

/// Two transitions deliberately differ in status and output multiplicity.
fn model(property: Property) -> finite::FiniteModel {
    let column: Column =
        serde_json::from_value(json!({"name":"n","ty":"int32","nullable":true})).unwrap();
    let rows = vec![
        vec![Cell::Int32(8)],
        vec![Cell::Int32(8)],
        vec![Cell::Null(SqlType::Int32)],
    ];
    let transitions = vec![success(rows), failure()]
        .into_iter()
        .map(|outcome| finite::Transition {
            input_catalog: None,
            input: Database::new(),
            outcome,
        })
        .collect();
    finite::encode(transitions, &[column], Some(&property)).unwrap()
}

/// SAT can select every enumerated transition, including the explicit SQL failure.
#[test]
fn all_transitions_are_selectable() {
    let model = model(Property::NoSqlError);
    let mut bridge = Bridge::new().unwrap();
    for index in 0..2 {
        let response = bridge.solve(&model.model, index + 2).unwrap();
        assert_eq!(response["status"], "SAT");
        assert_eq!(model.decode(&response).unwrap(), index);
    }
    let response = bridge.solve(&model.model, 1).unwrap();
    assert_eq!(model.decode(&response).unwrap(), 1);
}

/// Duplicate row occurrences count separately in Alloy's cardinality policy.
#[test]
fn cardinality_and_payload_corruption() {
    let model = model(Property::MaxRows { max: 2 });
    let mut bridge = Bridge::new().unwrap();
    let mut response = bridge.solve(&model.model, 1).unwrap();
    assert_eq!(model.decode(&response).unwrap(), 0);
    response["relations"]["this/Row.cells"][0][2] = json!("forged_atom");
    assert!(
        model
            .decode(&response)
            .unwrap_err()
            .contains("cells differ")
    );
}

/// Empty RETURNING schemas preserve zero-column occurrences without invented values.
#[test]
fn zero_column_rows_are_not_dropped() {
    let model = finite::encode(
        vec![finite::Transition {
            input_catalog: None,
            input: Database::new(),
            outcome: success(vec![vec![], vec![]]),
        }],
        &[],
        Some(&Property::MaxRows { max: 1 }),
    )
    .unwrap();
    let mut bridge = Bridge::new().unwrap();
    let response = bridge.solve(&model.model, 1).unwrap();
    assert_eq!(response["status"], "SAT");
    assert_eq!(model.decode(&response).unwrap(), 0);
}

/// Corrupting state or retained effects cannot pass a row-only replay check.
#[test]
fn complete_observation_payload_is_checked() {
    let model = model(Property::NoSqlError);
    let mut bridge = Bridge::new().unwrap();
    let response = bridge.solve(&model.model, 0).unwrap();
    for field in ["prePayload", "postPayload", "eventPayload", "detailPayload"] {
        let mut corrupted = response.clone();
        corrupted["relations"][format!("this/Transition.{field}")][0][1] = json!("forged_payload");
        assert!(
            model
                .decode(&corrupted)
                .unwrap_err()
                .contains("observation")
        );
    }
}

//! Execute actual Alloy assertion checks and reject run commands mislabeled as checks.
use serde_json::{Value, json};
use sqlolla::{bridge::Bridge, pipeline::Pipeline};

/// Bound the same nullable input domain for native and evaluator-backed assertion checks.
fn request(sql: &str, property: Value) -> Value {
    json!({"id":"assertion", "mode":"analyze", "include_model":true, "sql":sql,
        "catalog":{"search_path":["public"],"tables":{"r":{"schema":"public","name":"r",
            "columns":[{"name":"x","ty":"int32","nullable":true}]}}},
        "bounds":{"r":2}, "domain":[{"type":"int32","value":0},{"type":"int32","value":1},
            {"type":"null","value":"int32"}], "property":property})
}

/// The same policy passes a filtered query and finds a replayed missing-filter counterexample.
#[test]
fn native_assertion_pass_and_counterexample() {
    let mut pipeline = Pipeline::new().unwrap();
    let property = json!({"kind":"only_value","column":0,"value":{"type":"int32","value":1}});
    for (sql, expected) in [
        (
            "SELECT x FROM r WHERE x=1",
            "NO_COUNTEREXAMPLE_WITHIN_BOUNDS",
        ),
        ("SELECT x FROM r", "COUNTEREXAMPLE"),
    ] {
        let response =
            pipeline.handle(&serde_json::to_vec(&request(sql, property.clone())).unwrap());
        assert_eq!(response["analysis"], expected, "{response}");
        assert_eq!(response["assumptions"], "SATISFIABLE");
        assert_eq!(response["property_check"]["command"], "check");
        assert!(
            response["model"]
                .as_str()
                .unwrap()
                .contains("check QueryProperty")
        );
    }
}

/// SQL error outcomes remain counterexamples to no-error assertions in the finite table.
#[test]
fn finite_assertion_preserves_error_counterexamples() {
    let mut pipeline = Pipeline::new().unwrap();
    for (sql, expected) in [
        ("SELECT count(*) FROM r", "NO_COUNTEREXAMPLE_WITHIN_BOUNDS"),
        ("SELECT 1/x FROM r", "COUNTEREXAMPLE"),
    ] {
        let response = pipeline
            .handle(&serde_json::to_vec(&request(sql, json!({"kind":"no_sql_error"}))).unwrap());
        assert_eq!(response["analysis"], expected, "{response}");
        assert_eq!(response["encoding"], "RUST_EVALUATOR_BACKED_FINITE_TABLE");
        assert!(
            response["model"]
                .as_str()
                .unwrap()
                .contains("ExecutionTable implies ResultProperty")
        );
    }
}

/// Bridge command metadata prevents a satisfiability run from masquerading as a safety check.
#[test]
fn run_is_not_an_assertion_check() {
    let mut bridge = Bridge::new().unwrap();
    let model =
        "sig A {}\nrun {} for 1\nassert QueryProperty { no A }\ncheck QueryProperty for 1\n";
    assert!(bridge.check(model, 0).is_err());
    let response = bridge.check(model, 1).unwrap();
    assert_eq!(response["command_kind"], "check");
    assert_eq!(response["status"], "SAT");
}

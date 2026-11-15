use super::*;

/// Construct explicit local content whose values must stay out of the provider packet.
fn input() -> DraftInput {
    serde_json::from_value(json!({"instruction":"Find delayed shipments", "share_schema":true,
        "request":{"mode":"analyze","sql":"SELECT 'private query'","catalog":{"search_path":["public"],
        "tables":{"shipments":{"schema":"public","name":"shipments","columns":[{"name":"delay","ty":"int64","nullable":false}]}}},
        "parameters":{"1":{"type":"int64","value":9223372036854775807_i64}},
        "property":{"kind":"max_rows","max":2},"bounds":{"shipments":1},"domain":[{"type":"text","value":"private value"}]}})).unwrap()
}

/// Existing SQL, parameters, data and property are never in the first-draft provider packet.
#[test]
fn shares_schema_and_type_only() {
    let input = input();
    let (packet, digest) = packet(&input).unwrap();
    assert!(!packet.contains("private"));
    assert!(!packet.contains("9223372036854775807"));
    let projected: Value = serde_json::from_str(&packet).unwrap();
    assert_eq!(projected["parameter_types"], json!({"1":"int64"}));
    assert_eq!(
        projected["schema"]["tables"]["shipments"]["columns"][0]["name"],
        "delay"
    );
    assert_eq!(
        body(packet, &digest).encode(Provider::OpenAi)["model"],
        MODEL
    );
}

/// Neither a forged compiler verdict nor a stale digest becomes a valid draft.
#[test]
fn rejects_excess_authority() {
    assert!(
        checked_output(
            json!({"context_sha256":"other","sql":"SELECT 1","assumptions":[]}),
            "current"
        )
        .is_err()
    );
    assert!(
        checked_output(
            json!({"context_sha256":"current","sql":"SELECT 1","assumptions":[],"compiled":true}),
            "current"
        )
        .is_err()
    );
}

/// Compile uses the exact typed parameters and catalog, with a fresh empty bag where no data is declared.
#[test]
fn compile_preserves_local_inputs() {
    let input = input();
    let compiled = compile_request(input.request.clone(), "SELECT delay FROM shipments");
    assert_eq!(compiled.catalog, input.request.catalog);
    assert_eq!(compiled.parameters, input.request.parameters);
    assert!(compiled.database["shipments"].is_empty());
    assert!(
        compiled.property.is_none() && compiled.bounds.is_empty() && compiled.domain.is_empty()
    );
}

//! Citation schema bounds never change the required local reference membership check.
use super::*;

/// Form a synthetic request without real provider data or credentials.
fn input() -> Input {
    Input {
        bundle_id: "a".repeat(64),
        instance_id: "b".repeat(64),
        model_revision_sha256: "c".repeat(64),
        redact_values: false,
        question: String::new(),
        instruction: "Preserve exact use cases".into(),
    }
}

/// Keep the complete pointer inventory even when provider schema constraints must be looser.
fn prompt(count: usize, prefix: &str) -> context::Prompt {
    context::Prompt {
        value: Value::Null,
        hash: "d".repeat(64),
        references: (0..count)
            .map(|index| format!("/{prefix}/{index}"))
            .collect(),
    }
}

/// Inspect only the provider schema type used for a cited reference.
fn citation(input: &Input, prompt: &context::Prompt) -> Value {
    response(input, prompt, true)
        .pointer("/properties/claims/items/properties/fact_references/items")
        .unwrap()
        .clone()
}

/// Use an enum for small inventories and a string when count or length exceeds its budget.
#[test]
fn citation_enum_refuses_provider_schema_overflow() {
    let input = input();
    let small = prompt(250, "r");
    assert_eq!(
        citation(&input, &small)["enum"].as_array().unwrap().len(),
        250
    );
    for prompt in [prompt(251, "r"), prompt(250, &"r".repeat(100))] {
        assert_eq!(citation(&input, &prompt), json!({"type":"string"}));
        assert!(prompt.references.len() >= 250);
    }
}

/// An invented citation in a large packet still fails independent exact membership validation.
#[test]
fn fallback_schema_keeps_every_local_citation_obligation() {
    let input = input();
    let prompt = prompt(251, "r");
    let mut document = json!({"instance_id":input.instance_id,"model_revision_sha256":input.model_revision_sha256,
        "context_sha256":prompt.hash,"title":"Candidate","claims":[{"text":"Evidence","fact_references":["/r/251"]}],
        "assumptions":[],"sql":"SELECT 1"});
    let error = output::validate(document.clone(), &input, &prompt, true)
        .err()
        .unwrap();
    assert_eq!(error.code, "ai.output_reference");
    document["claims"][0]["fact_references"] = json!(["/r/250"]);
    assert!(output::validate(document, &input, &prompt, true).is_ok());
}

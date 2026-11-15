//! Adversarial assistance tests use synthetic secrets and never contact an external provider.
use super::*;

/// Construct a canonical in-memory fixture with values in every redaction-sensitive location.
fn evidence() -> context::Evidence {
    let secret = "test-secret-payload";
    let semantic = json!({"output":[[{"type":"text","value":secret}]]});
    context::Evidence {
        bundle: json!({"ir":{"literal":secret},"model":format!("// {secret}"),"request":{"sql":format!("SELECT '{secret}'"),"mode":"analyze",
        "property":{"kind":"no_value","value":secret},"parameters":{"1":secret},"domain":[secret],
        "bounds":{"t":2},"catalog":{"tables":{"t":{"schema":"public","name":"t","columns":[{"name":"v","ty":"text","nullable":true}],"defaults":secret}}}}}),
        instance: json!({"instance_id":"a".repeat(64),"model_revision_sha256":"b".repeat(64),"semantic":semantic,
            "facts":[{"reference":"/semantic/output/0","kind":"output_row","occurrence":0,"value":semantic["output"][0]}]}),
    }
}

/// Provide exact IDs without any credentials in the serializable AI request body.
fn input() -> Input {
    Input {
        bundle_id: "c".repeat(64),
        instance_id: "a".repeat(64),
        model_revision_sha256: "b".repeat(64),
        redact_values: false,
        question: String::new(),
        instruction: "Filter rows".into(),
    }
}

/// A valid assistant document carries no authority-bearing context or solver fields.
fn document(prompt: &context::Prompt) -> Value {
    json!({"instance_id":"a".repeat(64),"model_revision_sha256":"b".repeat(64),"context_sha256":prompt.hash,
        "title":"Observed row","claims":[{"text":"A supplied output row exists.","fact_references":["/semantic/output/0"]}],
        "assumptions":["Only the declared finite scope is considered."],"sql":null})
}

/// Redaction excludes source and scalar payloads rather than merely replacing selected displayed cells.
#[test]
fn redaction_removes_all_payload_locations() {
    let evidence = evidence();
    let hidden = context::build(&evidence, true).unwrap();
    assert!(!hidden.value.to_string().contains("test-secret-payload"));
    assert!(hidden.value.get("request").is_none());
    assert!(hidden.value.get("source_triple").is_none());
    assert_eq!(hidden.value["source_triple_included"], false);
    assert_eq!(hidden.value["prompt_view_complete"], false);
    assert!(hidden.value["facts"][0].get("value").is_none());
    assert!(
        context::build(&evidence, false)
            .unwrap()
            .value
            .to_string()
            .contains("test-secret-payload")
    );
}

/// All three exact persisted representations are sent together only with explicit disclosure.
#[test]
fn source_triple_and_view_completeness_are_explicit() {
    let mut evidence = evidence();
    let complete = context::build(&evidence, false).unwrap();
    assert_eq!(complete.value["source_triple"]["ir"], evidence.bundle["ir"]);
    assert_eq!(
        complete.value["source_triple"]["alloy"],
        evidence.bundle["model"]
    );
    assert_eq!(
        complete.value["source_triple"]["sql"],
        evidence.bundle["request"]["sql"]
    );
    assert_eq!(complete.value["prompt_view_complete"], true);
    let rows: Vec<_> = (0..65).map(|index| json!(index)).collect();
    evidence.instance["semantic"]["output"] = json!(rows);
    evidence.instance["facts"] = json!(
        (0..65)
            .map(|index| json!({
        "reference":format!("/semantic/output/{index}"),"value":index,"kind":"row"}))
            .collect::<Vec<_>>()
    );
    let truncated = context::build(&evidence, false).unwrap();
    assert_eq!(truncated.value["prompt_view_complete"], false);
    assert_eq!(truncated.value["omitted_fact_count"], 1);
    evidence.bundle["model"] = Value::Null;
    assert!(context::build(&evidence, false).is_err());
}

/// A syntactically valid structured response cannot invent references, replace bounds or drift identity.
#[test]
fn output_rejects_untrusted_authority_and_references() {
    let prompt = context::build(&evidence(), false).unwrap();
    let mut request = input();
    request.instruction.clear();
    assert!(output::validate(document(&prompt), &request, &prompt, false).is_ok());
    for field in ["instance_id", "model_revision_sha256", "context_sha256"] {
        let mut value = document(&prompt);
        value[field] = json!("d".repeat(64));
        assert!(output::validate(value, &request, &prompt, false).is_err());
    }
    let mut value = document(&prompt);
    value["bounds"] = json!({"t":999});
    assert!(output::validate(value, &request, &prompt, false).is_err());
    let mut value = document(&prompt);
    value["claims"][0]["fact_references"] = json!(["/invented"]);
    assert!(output::validate(value, &request, &prompt, false).is_err());
}

/// Refusals, incomplete responses, tool calls and model aliases never fall back to usable prose.
#[test]
fn provider_identity_and_completion_are_required() {
    let base = json!({"model":MODEL,"status":"completed","error":null,"output":[{"type":"message","role":"assistant",
        "status":"completed","content":[{"type":"output_text","text":"{}"}]}]});
    assert_eq!(provider::extract(&base).unwrap(), json!({}));
    for (field, value) in [
        ("model", json!("other-model")),
        ("status", json!("incomplete")),
        ("output", json!([{"type":"function_call"}])),
    ] {
        let mut response = base.clone();
        response[field] = value;
        assert!(provider::extract(&response).is_err());
    }
    let mut refusal = base;
    refusal["output"][0]["content"] = json!([{"type":"refusal","refusal":"secret-like body"}]);
    assert_eq!(
        provider::extract(&refusal).err().unwrap().code,
        "ai.refused"
    );
}

/// Redacted proposal requests and tampered canonical facts fail before provider egress.
#[test]
fn admission_precedes_egress() {
    let mut request = input();
    request.redact_values = true;
    assert!(validate(&request, true).is_err());
    let mut evidence = evidence();
    evidence.instance["facts"][0]["value"] = json!("tampered");
    assert!(context::build(&evidence, false).is_err());
}

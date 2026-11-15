//! No-network adversarial checks for the review-driven provider boundary.
use super::*;

/// Supply only synthetic canonical IDs and a human prompt, never provider credentials.
fn input(disposition: &str) -> RefinementInput {
    RefinementInput {
        bundle_id: "a".repeat(64),
        model_revision_sha256: "b".repeat(64),
        feedback: vec![json!({"instance_id":"c".repeat(64),"disposition":disposition})],
        instruction: "Only show tenant 1".into(),
        redact_values: false,
        feedback_sha256: None,
    }
}

/// Provide an internally coherent packet without relying on untrusted browser facts.
fn prepared(input: &RefinementInput) -> Value {
    json!({"feedback_sha256":"d".repeat(64),"packet":{"feedback":input.feedback},
        "references":["/feedback/0"]})
}

/// Empty and unsure-only bins cannot issue a generation; either decisive disposition can.
#[test]
fn decisive_feedback_is_required() {
    assert!(admission(&input("accepted_use_case")).is_ok());
    assert!(admission(&input("unintended_behavior")).is_ok());
    assert_eq!(
        admission(&input("unsure")).unwrap_err().code,
        "ai.feedback_required"
    );
    let mut empty = input("accepted_use_case");
    empty.feedback.clear();
    assert!(admission(&empty).is_err());
}

/// Stale previews, redaction and missing intent fail before network egress.
#[test]
fn stale_or_undisclosed_context_is_refused() {
    let mut request = input("accepted_use_case");
    let canonical = prepared(&request);
    request.feedback_sha256 = Some("e".repeat(64));
    assert!(expected_digest(&request, &canonical).is_err());
    request.redact_values = true;
    assert!(admission(&request).is_err());
    request.redact_values = false;
    request.instruction.clear();
    assert!(admission(&request).is_err());
}

/// Every bin and the original instruction are committed by the exact transmitted context digest.
#[test]
fn packet_digest_binds_prompt_and_all_bins() {
    let request = input("accepted_use_case");
    let canonical = prepared(&request);
    let first = packet(&request, &canonical).unwrap();
    let mut different = input("accepted_use_case");
    different.instruction.push_str("; extra intent");
    assert_ne!(first.hash, packet(&different, &canonical).unwrap().hash);
    let mut changed = canonical.clone();
    changed["packet"]["feedback"][0]["disposition"] = json!("unsure");
    assert_ne!(first.hash, packet(&request, &changed).unwrap().hash);
    assert_eq!(first.value["original_instruction"], request.instruction);
}

/// References into nonexistent or truncated data cannot become model citation choices.
#[test]
fn no_invented_references_or_silent_context_truncation() {
    let request = input("accepted_use_case");
    let mut canonical = prepared(&request);
    canonical["references"] = json!(["/feedback/99"]);
    assert!(packet(&request, &canonical).is_err());
    canonical["references"] = json!(["/feedback/0"]);
    canonical["packet"]["huge"] = json!("x".repeat(131_072));
    assert_eq!(
        packet(&request, &canonical).err().unwrap().code,
        "ai.context_size"
    );
}

/// Candidate validation receives the exact same review snapshot and the worker's committed digest.
#[test]
fn model_cannot_replace_review_requirements() {
    let request = input("unintended_behavior");
    let canonical = prepared(&request);
    let action = command(&request, Some(("SELECT 1", &canonical)));
    assert_eq!(action["feedback"], json!(request.feedback));
    assert_eq!(action["feedback_sha256"], canonical["feedback_sha256"]);
    assert_eq!(action["bundle_id"], request.bundle_id);
    assert_eq!(
        action["model_revision_sha256"],
        request.model_revision_sha256
    );
    assert_eq!(action["sql"], "SELECT 1");
}

/// The previous unlabelled endpoint no longer offers a bypass around the decisive-case gate.
#[tokio::test]
async fn legacy_proposal_route_refuses_regeneration() {
    assert_eq!(
        proposal_requires_feedback().await.code,
        "ai.feedback_required"
    );
}

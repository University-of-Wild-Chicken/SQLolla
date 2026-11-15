//! Verify routing and response admission without contacting any provider.
use super::*;

/// A selected provider fixes the TLS endpoint and credential header.
#[test]
fn routes_keys_only_to_selected_provider() {
    let _ = rustls::crypto::ring::default_provider().install_default();
    let client = reqwest::Client::new();
    for (id, host, header) in [
        ("openai", "api.openai.com", "authorization"),
        ("anthropic", "api.anthropic.com", "authorization"),
        (
            "gemini",
            "generativelanguage.googleapis.com",
            "x-goog-api-key",
        ),
    ] {
        let mut headers = HeaderMap::new();
        headers.insert(
            "authorization",
            "Bearer synthetic-test-key".parse().unwrap(),
        );
        headers.insert("x-sqlolla-provider", id.parse().unwrap());
        let selected = Byot::from_headers(&headers).unwrap();
        let request = selected.request(&client).build().unwrap();
        check_credential(&request, host, header);
        headers.append("x-sqlolla-provider", "openai".parse().unwrap());
        assert!(Byot::from_headers(&headers).is_err());
    }
}

/// Check the chosen endpoint and prohibit a second credential carrier.
fn check_credential(request: &reqwest::Request, host: &str, header: &str) {
    assert_eq!(request.url().scheme(), "https");
    assert_eq!(request.url().host_str(), Some(host));
    assert!(request.url().query().is_none());
    assert!(request.headers()[header].is_sensitive());
    let sent: Vec<_> = ["authorization", "x-goog-api-key"]
        .into_iter()
        .filter(|name| request.headers().contains_key(*name))
        .collect();
    assert_eq!(sent, [header]);
}

/// Gemini must return one complete message from the exact selected model.
#[test]
fn gemini_requires_exact_model_and_completed_text() {
    let valid = json!({"modelVersion":"gemini-4-hydrogen","candidates":[{"finishReason":"STOP",
        "content":{"role":"model","parts":[{"text":"{}", "thoughtSignature":"metadata"}]}}]});
    assert_eq!(gemini_text(&valid).unwrap(), "{}");
    for (pointer, value) in [
        ("/modelVersion", json!("other-model")),
        ("/candidates/0/finishReason", json!("MAX_TOKENS")),
        (
            "/candidates/0/content/parts",
            json!([{"text":"{}", "functionCall":{}}]),
        ),
    ] {
        let mut invalid = valid.clone();
        *invalid.pointer_mut(pointer).unwrap() = value;
        assert!(gemini_text(&invalid).is_err());
    }
}

/// Claude can include a reasoning block before a single final JSON message.
#[test]
fn anthropic_requires_exact_model_and_end_turn() {
    let mut reply = json!({"type":"message","model":"claude-sonnet-5-5","role":"assistant",
        "stop_reason":"end_turn","content":[{"type":"thinking"},{"type":"text","text":"{}"}]});
    assert_eq!(anthropic_text(&reply).unwrap(), "{}");
    reply["stop_reason"] = json!("max_tokens");
    assert!(anthropic_text(&reply).is_err());
    reply["stop_reason"] = json!("end_turn");
    reply["content"][0]["type"] = json!("tool_use");
    assert!(anthropic_text(&reply).is_err());
}

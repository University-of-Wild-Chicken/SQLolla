//! Verify routing and response admission without contacting any provider.
use super::*;

/// A selected provider fixes the TLS endpoint and credential header.
#[test]
fn routes_keys_only_to_selected_provider() {
    let _ = rustls::crypto::ring::default_provider().install_default();
    let client = reqwest::Client::new();
    for (id, host, header) in [
        ("openai", "api.openai.com", "authorization"),
        ("anthropic", "api.anthropic.com", "x-api-key"),
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
    let sent: Vec<_> = ["authorization", "x-api-key", "x-goog-api-key"]
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
    assert_eq!(gemini_text(&valid, "gemini-4-hydrogen").unwrap(), "{}");
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
        assert!(gemini_text(&invalid, "gemini-4-hydrogen").is_err());
    }
}

/// Claude can include a reasoning block before a single final JSON message.
#[test]
fn anthropic_requires_exact_model_and_end_turn() {
    let mut reply = json!({"type":"message","model":"claude-sonnet-5-5","role":"assistant",
        "stop_reason":"end_turn","content":[{"type":"thinking"},{"type":"text","text":"{}"}]});
    assert_eq!(anthropic_text(&reply, "claude-sonnet-5-5").unwrap(), "{}");
    reply["stop_reason"] = json!("max_tokens");
    assert!(anthropic_text(&reply, "claude-sonnet-5-5").is_err());
    reply["stop_reason"] = json!("end_turn");
    reply["content"][0]["type"] = json!("tool_use");
    assert!(anthropic_text(&reply, "claude-sonnet-5-5").is_err());
}

/// Build provider and model selection headers around a synthetic key.
fn selection(provider: &str, model: Option<&str>) -> HeaderMap {
    let mut headers = HeaderMap::new();
    headers.insert(
        "authorization",
        "Bearer synthetic-test-key".parse().unwrap(),
    );
    headers.insert("x-sqlolla-provider", provider.parse().unwrap());
    if let Some(model) = model {
        headers.insert("x-sqlolla-model", model.parse().unwrap());
    }
    headers
}

/// An absent choice keeps each tested default; an explicit choice must stay in its provider family.
#[test]
fn model_choice_is_explicit_and_family_bound() {
    for (provider, default) in [
        ("openai", MODEL),
        ("anthropic", "claude-sonnet-5-5"),
        ("gemini", "gemini-4-hydrogen"),
    ] {
        let headers = selection(provider, None);
        assert_eq!(
            Byot::from_headers(&headers).unwrap().identity(),
            (provider, default)
        );
    }
    for (provider, model) in [
        ("anthropic", "claude-haiku-5-5"),
        ("openai", "o4-mini"),
        ("gemini", "gemini-4.1-pro"),
    ] {
        let headers = selection(provider, Some(model));
        assert_eq!(Byot::from_headers(&headers).unwrap().model, model);
    }
}

/// Foreign families, unsafe path text, uppercase aliases and duplicate choices are refused.
#[test]
fn model_choice_rejects_unadmitted_names() {
    for (provider, model) in [
        ("anthropic", "gpt-6.1-sol"),
        ("openai", "claude-opus-5-5"),
        ("gemini", "../models/x"),
        ("gemini", "Gemini-4"),
        ("openai", "omni"),
    ] {
        assert!(Byot::from_headers(&selection(provider, Some(model))).is_err());
    }
    let mut twice = selection("anthropic", Some("claude-opus-5-5"));
    twice.append("x-sqlolla-model", "claude-haiku-5-5".parse().unwrap());
    assert_eq!(Byot::from_headers(&twice).err().unwrap().code, "ai.model");
}

/// The selected model reaches the wire body and Gemini's fixed path template.
#[test]
fn selected_model_reaches_the_request() {
    let _ = rustls::crypto::ring::default_provider().install_default();
    let headers = selection("gemini", Some("gemini-4.1-pro"));
    let byot = Byot::from_headers(&headers).unwrap();
    let request = byot.request(&reqwest::Client::new()).build().unwrap();
    assert_eq!(
        request.url().path(),
        "/v1beta/models/gemini-4.1-pro:generateContent"
    );
    let task = Completion {
        instructions: "rules".into(),
        user: json!({}),
        schema_name: "schema",
        schema: json!({"type":"object"}),
    };
    assert_eq!(
        task.encode(Provider::Anthropic, "claude-opus-5-5")["model"],
        "claude-opus-5-5"
    );
}

/// Dated snapshots of the selected model are accepted; sibling models and aliases are not.
#[test]
fn served_model_must_match_the_selection() {
    let reply = |model: &str| {
        json!({"type":"message","model":model,"role":"assistant",
        "stop_reason":"end_turn","content":[{"type":"text","text":"{}"}]})
    };
    assert!(anthropic_text(&reply("claude-opus-5-5"), "claude-opus-5-5").is_ok());
    assert!(anthropic_text(&reply("claude-sonnet-5-5"), "claude-sonnet-5").is_err());
    assert!(anthropic_text(&reply("claude-opus-5-5"), "claude-sonnet-5-5").is_err());
    assert!(catalog::serves(
        &json!("gpt-6.1-sol-2026-10-01"),
        "gpt-6.1-sol"
    ));
    assert!(catalog::serves(
        &json!("gpt-6.1-sol-20261001"),
        "gpt-6.1-sol"
    ));
    assert!(!catalog::serves(&json!("gpt-6.1-sol-mini"), "gpt-6.1-sol"));
    assert!(!catalog::serves(&json!(null), "gpt-6.1-sol"));
}

/// Catalog listings keep admissible text models and drop other families, media models and junk.
#[test]
fn catalog_lists_only_admissible_text_models() {
    let openai = json!({"data":[{"id":"gpt-6.1-sol"},{"id":"gpt-image-2"},{"id":"o4-mini"},
        {"id":"text-embedding-3-large"},{"id":"gpt-6.1-sol"},{"id":"ft:gpt-6.1:org"}]});
    assert_eq!(
        catalog_ids(Provider::OpenAi, &openai).unwrap(),
        ["gpt-6.1-sol", "o4-mini"]
    );
    let anthropic = json!({"data":[{"id":"claude-sonnet-5-5"},{"id":"claude-opus-5-5"}]});
    assert_eq!(
        catalog_ids(Provider::Anthropic, &anthropic).unwrap(),
        ["claude-opus-5-5", "claude-sonnet-5-5"]
    );
    let gemini = json!({"models":[
        {"name":"models/gemini-4-hydrogen","supportedGenerationMethods":["generateContent"]},
        {"name":"models/gemini-embedding-2","supportedGenerationMethods":["embedContent"]},
        {"name":"gemini-unprefixed","supportedGenerationMethods":["generateContent"]}]});
    assert_eq!(
        catalog_ids(Provider::Gemini, &gemini).unwrap(),
        ["gemini-4-hydrogen"]
    );
    assert!(catalog_ids(Provider::Gemini, &json!({"data":[]})).is_err());
}

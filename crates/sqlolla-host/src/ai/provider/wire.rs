//! Fixed per-provider destinations, request formats and response identity checks.
use super::*;

/// Preserve the original OpenAI model and legacy status field.
pub const MODEL: &str = "gpt-6.1-sol";

/// The entire provider allowlist. Callers never supply URLs or model identifiers.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(in crate::ai) enum Provider {
    OpenAi,
    Anthropic,
    Gemini,
}

impl Provider {
    /// Stable wire identity and the model allowed at this provider's endpoint.
    pub(in crate::ai) fn identity(self) -> (&'static str, &'static str) {
        match self {
            Self::OpenAi => ("openai", MODEL),
            Self::Anthropic => ("anthropic", "claude-sonnet-5-5"),
            Self::Gemini => ("gemini", "gemini-4-hydrogen"),
        }
    }

    /// Admit a single explicit choice, including the original OpenAI-only client's header omission.
    fn from_headers(headers: &HeaderMap) -> Result<Self> {
        let mut values = headers.get_all("x-sqlolla-provider").iter();
        match (values.next(), values.next()) {
            (None, None) => Ok(Self::OpenAi),
            (Some(value), None) if value == "openai" => Ok(Self::OpenAi),
            (Some(value), None) if value == "anthropic" => Ok(Self::Anthropic),
            (Some(value), None) if value == "gemini" => Ok(Self::Gemini),
            _ => Err(failure("ai.provider", "Choose a supported AI provider")),
        }
    }
}

/// The key and destination stay paired for the lifetime of a single request.
#[derive(Clone, Copy)]
pub(in crate::ai) struct Byot<'a> {
    pub provider: Provider,
    key: &'a str,
}

impl<'a> Byot<'a> {
    /// Resolve provider routing and authentication before any upstream request.
    pub(in crate::ai) fn from_headers(headers: &'a HeaderMap) -> Result<Self> {
        Ok(Self {
            provider: Provider::from_headers(headers)?,
            key: credential(headers)?,
        })
    }

    /// Attach the key only to the selected provider's fixed HTTPS destination.
    pub(super) fn request(self, client: &reqwest::Client) -> reqwest::RequestBuilder {
        match self.provider {
            Provider::OpenAi => client
                .post("https://api.openai.com/v1/responses")
                .bearer_auth(self.key),
            Provider::Anthropic => client
                .post("https://api.anthropic.com/v1/messages")
                .bearer_auth(self.key)
                .header("anthropic-version", "2023-06-01"),
            Provider::Gemini => client
                .post("https://generativelanguage.googleapis.com/v1beta/models/gemini-4-hydrogen:generateContent")
                .header("x-goog-api-key", sensitive_key(self.key)),
        }
    }
}

/// Mark a validated API key as sensitive in the outbound request metadata.
fn sensitive_key(key: &str) -> reqwest::header::HeaderValue {
    let mut value = reqwest::header::HeaderValue::from_str(key).expect("validated visible ASCII");
    value.set_sensitive(true);
    value
}

/// A task's disclosure and structured output expectations precede wire encoding.
pub(in crate::ai) struct Completion {
    pub instructions: String,
    pub user: Value,
    pub schema_name: &'static str,
    pub schema: Value,
}

impl Completion {
    /// Encode one task with provider-native structured output and no tools.
    pub(in crate::ai) fn encode(mut self, provider: Provider) -> Value {
        let (_, model) = provider.identity();
        let text = self.user.to_string();
        if provider != Provider::OpenAi {
            portable_schema(&mut self.schema);
        }
        match provider {
            Provider::OpenAi => json!({"model":model,"store":false,"tools":[],"tool_choice":"none",
                "max_output_tokens":4096,"reasoning":{"effort":"low"},"instructions":self.instructions,
                "input":[{"role":"user","content":[{"type":"input_text","text":text}]}],
                "text":{"format":{"type":"json_schema","name":self.schema_name,"strict":true,"schema":self.schema}}}),
            Provider::Anthropic => {
                json!({"model":model,"max_tokens":4096,"system":self.instructions,
                    "messages":[{"role":"user","content":text}],
                    "output_config":{"effort":"low","format":{"type":"json_schema","schema":self.schema}}})
            }
            Provider::Gemini => json!({"systemInstruction":{"parts":[{"text":self.instructions}]},
                    "contents":[{"role":"user","parts":[{"text":text}]}],
                    "generationConfig":{"maxOutputTokens":4096,"thinkingConfig":{"thinkingLevel":"LOW"},
                        "responseMimeType":"application/json","responseJsonSchema":self.schema}}),
        }
    }
}

/// Avoid unsupported Claude bounds and oversized Gemini grammars. Local gates enforce all bounds.
fn portable_schema(schema: &mut Value) {
    let Some(object) = schema.as_object_mut() else {
        return;
    };
    for unsupported in ["minLength", "maxLength", "maxItems"] {
        object.remove(unsupported);
    }
    if let Some(Value::Array(types)) = object.get("type") {
        let branches: Vec<_> = types.iter().map(|ty| json!({"type":ty})).collect();
        object.remove("type");
        object.insert("anyOf".into(), json!(branches));
    }
    if let Some(properties) = object.get_mut("properties").and_then(Value::as_object_mut) {
        properties.values_mut().for_each(portable_schema);
    }
    if let Some(items) = object.get_mut("items") {
        portable_schema(items);
    }
    if let Some(branches) = object.get_mut("anyOf").and_then(Value::as_array_mut) {
        branches.iter_mut().for_each(portable_schema);
    }
}

/// Decode one completed Claude assistant message, ignoring only documented thinking metadata.
pub(super) fn anthropic_text(response: &Value) -> Result<&str> {
    if response["model"] != Provider::Anthropic.identity().1
        || response["type"] != "message"
        || response["role"] != "assistant"
        || response["stop_reason"] != "end_turn"
    {
        return Err(identity_error());
    }
    completed_text(&response["content"], |part| match part["type"].as_str() {
        Some("thinking" | "redacted_thinking") => Ok(None),
        Some("text") => part["text"].as_str().map(Some).ok_or_else(shape_error),
        _ => Err(shape_error()),
    })
}

/// Require one completed Gemini candidate with the exact modelVersion.
pub(super) fn gemini_text(response: &Value) -> Result<&str> {
    let candidates = response["candidates"].as_array().ok_or_else(shape_error)?;
    let [candidate] = candidates.as_slice() else {
        return Err(shape_error());
    };
    if response["modelVersion"] != Provider::Gemini.identity().1
        || candidate["finishReason"] != "STOP"
        || candidate["content"]["role"] != "model"
    {
        return Err(identity_error());
    }
    completed_text(&candidate["content"]["parts"], gemini_part)
}

/// Reject tool and non-text parts, even when a part also contains visible text.
fn gemini_part(part: &Value) -> Result<Option<&str>> {
    let object = part.as_object().ok_or_else(shape_error)?;
    if object
        .keys()
        .any(|key| !matches!(key.as_str(), "text" | "thought" | "thoughtSignature"))
    {
        return Err(shape_error());
    }
    let text = part["text"].as_str().ok_or_else(shape_error)?;
    Ok((part["thought"] != true).then_some(text))
}

/// Accept exactly one visible text part and reject other content via the provider-specific decoder.
fn completed_text(
    parts: &Value,
    classify: impl Fn(&Value) -> Result<Option<&str>>,
) -> Result<&str> {
    let mut text = None;
    for part in parts.as_array().ok_or_else(shape_error)? {
        if let Some(part) = classify(part)?
            && text.replace(part).is_some()
        {
            return Err(shape_error());
        }
    }
    text.ok_or_else(shape_error)
}

/// Provider-controlled error bodies and model identifiers are excluded from diagnostics.
fn identity_error() -> AiError {
    failure(
        "ai.provider_identity",
        "The provider response was incomplete or used a different model",
    )
}

/// Keep unsupported output shapes distinct from usable assistant text.
fn shape_error() -> AiError {
    failure(
        "ai.provider_shape",
        "Expected one structured assistant message",
    )
}

#[cfg(test)]
mod tests;

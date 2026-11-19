//! Explicit per-request model choice: name admission, served-model identity and catalog listing.
use super::*;

/// Reject a malformed, foreign-family or duplicated model choice before any provider egress.
fn invalid_model() -> AiError {
    failure(
        "ai.model",
        "Choose a model offered by the selected provider",
    )
}

/// A model identifier is a short lowercase token that is safe inside a fixed provider URL path.
pub(in crate::ai) fn valid_model_name(model: &str) -> bool {
    (1..=64).contains(&model.len())
        && model.as_bytes()[0].is_ascii_alphanumeric()
        && model
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || b"-.".contains(&byte))
}

impl Provider {
    /// Model families each provider endpoint may receive; other names are refused, not rerouted.
    pub(in crate::ai) fn admits(self, model: &str) -> bool {
        match self {
            Self::OpenAi => {
                model.starts_with("gpt-")
                    || model.starts_with("chatgpt-")
                    || model
                        .strip_prefix('o')
                        .is_some_and(|rest| rest.starts_with(|first: char| first.is_ascii_digit()))
            }
            Self::Anthropic => model.starts_with("claude-"),
            Self::Gemini => model.starts_with("gemini-"),
        }
    }
}

/// Use the explicit `X-SQLolla-Model` choice, or the provider's tested default when it is absent.
pub(in crate::ai) fn selected(headers: &HeaderMap, provider: Provider) -> Result<&str> {
    let mut values = headers.get_all("x-sqlolla-model").iter();
    let model = match (values.next(), values.next()) {
        (None, None) => return Ok(provider.identity().1),
        (Some(value), None) => value.to_str().map_err(|_| invalid_model())?,
        _ => return Err(invalid_model()),
    };
    if !valid_model_name(model) || !provider.admits(model) {
        return Err(invalid_model());
    }
    Ok(model)
}

/// Accept the selected model or its dated snapshot, never a sibling such as `-5` versus `-5-5`.
pub(in crate::ai) fn serves(reported: &Value, selected: &str) -> bool {
    let Some(reported) = reported.as_str() else {
        return false;
    };
    reported == selected
        || reported
            .strip_prefix(selected)
            .and_then(|rest| rest.strip_prefix('-'))
            .is_some_and(dated)
}

/// Snapshot suffixes are exactly `YYYYMMDD` or `YYYY-MM-DD`.
fn dated(suffix: &str) -> bool {
    let bytes = suffix.as_bytes();
    match bytes.len() {
        8 => bytes.iter().all(u8::is_ascii_digit),
        10 => bytes.iter().enumerate().all(|(index, byte)| {
            if index == 4 || index == 7 {
                *byte == b'-'
            } else {
                byte.is_ascii_digit()
            }
        }),
        _ => false,
    }
}

/// Extract text-generation model identifiers this provider would admit, sorted and bounded.
pub(in crate::ai) fn catalog_ids(provider: Provider, catalog: &Value) -> Result<Vec<String>> {
    let items = match provider {
        Provider::Gemini => &catalog["models"],
        _ => &catalog["data"],
    };
    let mut models: Vec<String> = items
        .as_array()
        .ok_or_else(|| failure("ai.provider_shape", "Missing provider model list"))?
        .iter()
        .filter_map(|item| listed_id(provider, item))
        .filter(|id| valid_model_name(id) && provider.admits(id) && textual(id))
        .map(str::to_owned)
        .collect();
    models.sort();
    models.dedup();
    models.truncate(200);
    Ok(models)
}

/// Read one catalog entry's identifier; Gemini entries must also support content generation.
fn listed_id(provider: Provider, item: &Value) -> Option<&str> {
    if provider != Provider::Gemini {
        return item["id"].as_str();
    }
    let methods = item["supportedGenerationMethods"].as_array()?;
    methods
        .iter()
        .any(|method| method == "generateContent")
        .then(|| item["name"].as_str()?.strip_prefix("models/"))
        .flatten()
}

/// Leave speech, image, embedding and realtime endpoints out of a structured-text model list.
fn textual(model: &str) -> bool {
    const NON_TEXT: [&str; 8] = [
        "audio",
        "realtime",
        "transcribe",
        "tts",
        "image",
        "embedding",
        "moderation",
        "search",
    ];
    !NON_TEXT.iter().any(|kind| model.contains(kind))
}

//! PostgreSQL qualified-name splitting under UTF-8/C identifier case folding.
use crate::json::error;
use crate::Result;

/// Preserve quoted spelling, fold unquoted ASCII and keep dots inside quoted identifiers.
pub(super) fn split(text: &str) -> Result<Option<Vec<String>>> {
    let mut rest = trim(text);
    let mut result = Vec::new();
    while !rest.is_empty() {
        if result.len() > 64 {
            return Err(error(
                "resource.catalog_name",
                "Too many identifier components",
            ));
        }
        let Some((name, suffix)) = component(rest) else {
            return Ok(None);
        };
        result.push(truncate(name));
        rest = trim(suffix);
        if rest.is_empty() {
            return Ok(Some(result));
        }
        let Some(suffix) = rest.strip_prefix('.') else {
            return Ok(None);
        };
        rest = trim(suffix);
        if rest.is_empty() {
            return Ok(None);
        }
    }
    Ok(Some(result))
}

/// An unquoted identifier extends to a separator or SQL scanner whitespace.
fn component(text: &str) -> Option<(String, &str)> {
    if text.starts_with('"') {
        return quoted(text);
    }
    let length = text
        .find(|value: char| value == '.' || value.is_ascii_whitespace())
        .unwrap_or(text.len());
    if length == 0 {
        return None;
    }
    Some((text[..length].to_ascii_lowercase(), &text[length..]))
}

/// A doubled quote is one identifier byte; an unclosed quote is a soft input failure.
fn quoted(text: &str) -> Option<(String, &str)> {
    let mut result = String::new();
    let mut chars = text.char_indices().peekable();
    chars.next();
    while let Some((index, value)) = chars.next() {
        if value != '"' {
            result.push(value);
            continue;
        }
        if chars.peek().is_some_and(|(_, value)| *value == '"') {
            chars.next();
            result.push('"');
        } else {
            return Some((result, &text[index + 1..]));
        }
    }
    None
}

/// PostgreSQL identifier truncation ends on a complete UTF-8 character before byte64.
fn truncate(mut value: String) -> String {
    let mut end = value.len().min(63);
    while !value.is_char_boundary(end) {
        end -= 1;
    }
    value.truncate(end);
    value
}

/// Scanner whitespace is ASCII and must not silently include Unicode space characters.
fn trim(value: &str) -> &str {
    value.trim_start_matches(|value: char| value.is_ascii_whitespace())
}

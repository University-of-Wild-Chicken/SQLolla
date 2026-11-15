//! Loopback same-origin admission and memory-only anti-CSRF capabilities.
use crate::{MAX_REQUEST_BYTES, config::BrowserOrigin, error::HostError, state::HostState};
use axum::{
    body::{Body, to_bytes},
    extract::{Request, State},
    http::{HeaderMap, Method, StatusCode},
    middleware::Next,
    response::{IntoResponse, Response},
};
use std::time::Duration;

/// Generate an unpredictable capability without persisting it or exposing it through logs.
pub(crate) fn nonce() -> Result<String, HostError> {
    let mut bytes = [0u8; 32];
    getrandom::fill(&mut bytes).map_err(|_| {
        HostError::new(
            StatusCode::SERVICE_UNAVAILABLE,
            "host.randomness",
            "Secure session capability could not be created",
        )
    })?;
    Ok(bytes.iter().map(|byte| format!("{byte:02x}")).collect())
}

/// Reject foreign origins and unsupported bodies before canonical or provider work begins.
pub async fn guard(State(state): State<HostState>, request: Request, next: Next) -> Response {
    let result = validate(
        request.headers(),
        request.method(),
        state.origin(),
        state.csrf_token(),
    );
    let response = match result {
        Err(error) => error.into_response(),
        Ok(()) => admitted(state, request, next).await,
    };
    protect(response)
}

/// Hold API capacity through complete handling without queuing unlimited waiting clients.
async fn admitted(state: HostState, request: Request, next: Next) -> Response {
    let permit = request
        .uri()
        .path()
        .starts_with("/api/")
        .then(|| state.admit());
    let _permit = match permit.transpose() {
        Ok(permit) => permit,
        Err(error) => return error.into_response(),
    };
    match bounded_request(request).await {
        Ok(request) => next.run(request).await,
        Err(error) => error.into_response(),
    }
}

/// Compare actual Host and Origin headers with the configured identities; ignore forwarded headers.
fn validate(
    headers: &HeaderMap,
    method: &Method,
    origin: &BrowserOrigin,
    csrf: &str,
) -> Result<(), HostError> {
    if header(headers, "host") != Some(origin.authority()) {
        return Err(forbidden(
            "host.authority",
            "Unexpected configured host authority",
        ));
    }
    if headers.contains_key("origin") && header(headers, "origin") != Some(origin.value()) {
        return Err(forbidden(
            "host.origin",
            "Cross-origin requests are not allowed",
        ));
    }
    if *method == Method::POST {
        validate_post(headers, origin.value(), csrf)?;
    } else if *method != Method::GET && *method != Method::HEAD {
        return Err(forbidden(
            "host.method",
            "Unsupported browser request method",
        ));
    }
    Ok(())
}

/// JSON POST requests require the same-origin capability and an explicit matching Origin.
fn validate_post(headers: &HeaderMap, origin: &str, csrf: &str) -> Result<(), HostError> {
    if header(headers, "origin") != Some(origin) || header(headers, "x-sqlolla-csrf") != Some(csrf)
    {
        return Err(forbidden(
            "host.csrf",
            "Same-origin session capability required",
        ));
    }
    let content = header(headers, "content-type")
        .unwrap_or("")
        .split(';')
        .next()
        .unwrap_or("");
    if !content.trim().eq_ignore_ascii_case("application/json") {
        return Err(HostError::new(
            StatusCode::UNSUPPORTED_MEDIA_TYPE,
            "host.content_type",
            "Expected application/json",
        ));
    }
    Ok(())
}

/// An incomplete or oversized body cannot occupy an unbounded input lane.
async fn bounded_request(request: Request) -> Result<Request, HostError> {
    if request.method() != Method::POST {
        return Ok(request);
    }
    let (parts, body) = request.into_parts();
    let bytes = tokio::time::timeout(Duration::from_secs(10), to_bytes(body, MAX_REQUEST_BYTES))
        .await
        .map_err(|_| {
            HostError::new(
                StatusCode::REQUEST_TIMEOUT,
                "host.body_deadline",
                "Request body deadline exceeded",
            )
        })?
        .map_err(|_| {
            HostError::new(
                StatusCode::PAYLOAD_TOO_LARGE,
                "host.request_limit",
                "Complete request exceeds two MiB or is malformed",
            )
        })?;
    Ok(Request::from_parts(parts, Body::from(bytes)))
}

/// Read one UTF-8 header without reflecting arbitrary header bytes into errors.
fn header<'a>(headers: &'a HeaderMap, name: &str) -> Option<&'a str> {
    let mut values = headers.get_all(name).iter();
    let value = values.next()?.to_str().ok()?;
    values.next().is_none().then_some(value)
}

#[cfg(test)]
mod tests;

/// Admission errors have no SQL or solver interpretation.
fn forbidden(code: &'static str, message: &'static str) -> HostError {
    HostError::new(StatusCode::FORBIDDEN, code, message)
}

/// Responses stay private and permit only the bundled worker/Graphviz assets.
fn protect(mut response: Response) -> Response {
    let headers = response.headers_mut();
    for (name, value) in [
        ("cache-control", "no-store"),
        ("x-content-type-options", "nosniff"),
        ("referrer-policy", "no-referrer"),
        ("x-frame-options", "DENY"),
        (
            "content-security-policy",
            "default-src 'self'; script-src 'self' 'wasm-unsafe-eval'; worker-src 'self' blob:; style-src 'self' 'unsafe-inline'; img-src 'self' data:; connect-src 'self'; object-src 'none'; base-uri 'none'; frame-ancestors 'none'",
        ),
    ] {
        headers.insert(name, axum::http::HeaderValue::from_static(value));
    }
    response
}

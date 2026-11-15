//! Same-origin boundary tests include malformed and duplicate browser headers.
use super::*;
use axum::http::HeaderValue;

const ORIGIN: &str = "http://127.0.0.1:8787";

/// Supply a valid same-origin POST baseline without using a real credential.
fn headers() -> HeaderMap {
    [
        ("host", "127.0.0.1:8787"),
        ("origin", ORIGIN),
        ("x-sqlolla-csrf", "test"),
        ("content-type", "application/json; charset=utf-8"),
    ]
    .into_iter()
    .map(|(name, value)| {
        (
            axum::http::HeaderName::from_static(name),
            HeaderValue::from_static(value),
        )
    })
    .collect()
}

/// Foreign origins, rebinding hosts and missing capabilities never enter a handler.
#[test]
fn origin_host_and_nonce_are_required() {
    assert!(validate(&headers(), &Method::POST, ORIGIN, "test").is_ok());
    for (name, value, code) in [
        ("host", "attacker.test:8787", "host.authority"),
        ("origin", "https://attacker.test", "host.origin"),
        ("x-sqlolla-csrf", "wrong", "host.csrf"),
        ("content-type", "text/plain", "host.content_type"),
    ] {
        let mut fields = headers();
        fields.insert(name, HeaderValue::from_static(value));
        assert_eq!(
            validate(&fields, &Method::POST, ORIGIN, "test")
                .unwrap_err()
                .code,
            code
        );
    }
    let mut fields = headers();
    fields.remove("origin");
    assert_eq!(
        validate(&fields, &Method::POST, ORIGIN, "test")
            .unwrap_err()
            .code,
        "host.csrf"
    );
    assert!(validate(&fields, &Method::GET, ORIGIN, "test").is_ok());
}

/// Duplicate capabilities or origins cannot exploit first-header interpretation differences.
#[test]
fn duplicate_security_headers_are_rejected() {
    for name in ["host", "origin", "x-sqlolla-csrf", "content-type"] {
        let mut fields = headers();
        fields.append(name, fields.get(name).unwrap().clone());
        assert!(validate(&fields, &Method::POST, ORIGIN, "test").is_err());
    }
    let mut fields = headers();
    fields.append("origin", HeaderValue::from_static(ORIGIN));
    assert!(validate(&fields, &Method::GET, ORIGIN, "test").is_err());
}

/// Unsupported methods cannot bypass the POST capability requirement.
#[test]
fn methods_and_response_policy_are_explicit() {
    assert_eq!(
        validate(&headers(), &Method::DELETE, ORIGIN, "test")
            .unwrap_err()
            .code,
        "host.method"
    );
    let response = protect(Response::new(Body::empty()));
    assert_eq!(response.headers()["cache-control"], "no-store");
    assert_eq!(response.headers()["x-frame-options"], "DENY");
    assert!(
        response.headers()["content-security-policy"]
            .to_str()
            .unwrap()
            .contains("connect-src 'self'")
    );
}

/// Body capacity is exact and an admitted buffer preserves every incoming byte.
#[tokio::test]
async fn body_limit_preserves_or_refuses_whole_payload() {
    let bytes = vec![b' '; MAX_REQUEST_BYTES];
    let request = Request::builder()
        .method("POST")
        .body(Body::from(bytes.clone()))
        .unwrap();
    let accepted = bounded_request(request).await.unwrap();
    assert_eq!(
        to_bytes(accepted.into_body(), MAX_REQUEST_BYTES)
            .await
            .unwrap()
            .as_ref(),
        &bytes
    );
    let oversized = Request::builder()
        .method("POST")
        .body(Body::from(vec![b' '; MAX_REQUEST_BYTES + 1]))
        .unwrap();
    assert_eq!(
        bounded_request(oversized).await.unwrap_err().code,
        "host.request_limit"
    );
}

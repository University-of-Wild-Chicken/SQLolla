//! Verify the exact browser identity before accepting deployment configuration.
use super::*;

/// Direct access keeps the actual loopback port except when HTTP implies it.
#[test]
fn loopback_origin_uses_browser_serialization() {
    assert_eq!(
        BrowserOrigin::loopback(8787).value(),
        "http://127.0.0.1:8787"
    );
    assert_eq!(BrowserOrigin::loopback(80).authority(), "127.0.0.1");
}

/// Preserve a canonical IIS authority, including an explicit nondefault HTTPS port.
#[test]
fn accepts_canonical_https_origins() {
    for authority in [
        "review.example.test",
        "review.example.test:8443",
        "[::1]:8443",
    ] {
        let url = format!("https://{authority}");
        let origin = BrowserOrigin::https(&url).unwrap();
        assert_eq!(origin.value(), url);
        assert_eq!(origin.authority(), authority);
    }
}

/// Reject paths, credentials, queries, fragments and browser-normalized alternative forms.
#[test]
fn rejects_noncanonical_https_origins() {
    for value in [
        "",
        "http://review.example.test",
        "https://review.example.test/",
        "https://review.example.test/api",
        "https://review.example.test?api",
        "https://review.example.test#api",
        "https://user@review.example.test",
        "https://review.example.test:443",
        "https://Review.Example.Test",
        "https://review.example.test\n",
        "https://",
    ] {
        assert!(BrowserOrigin::https(value).is_err(), "{value}");
    }
}

//! Administrator-owned paths and loopback binding are independent of browser commands.
use std::path::PathBuf;

/// Local server configuration; requests cannot supply filesystem roots or executable paths.
pub struct Config {
    /// Loopback port; zero requests a fresh ephemeral port for integration tests.
    pub port: u16,
    /// Canonical directory containing the self-hosted built frontend.
    pub frontend: PathBuf,
    /// One externally visible HTTPS origin when a reverse proxy fronts this listener.
    pub public_origin: Option<BrowserOrigin>,
}

/// The browser identities are fixed at startup, independently of proxy headers.
#[derive(Debug)]
pub struct BrowserOrigin {
    value: String,
    authority: String,
}

impl BrowserOrigin {
    /// Use the browser's canonical HTTP origin for the direct local listener.
    pub fn loopback(port: u16) -> Self {
        let authority = if port == 80 {
            "127.0.0.1".into()
        } else {
            format!("127.0.0.1:{port}")
        };
        Self {
            value: format!("http://{authority}"),
            authority,
        }
    }

    /// Accept only the exact browser serialization of a pathless HTTPS origin.
    pub fn https(value: &str) -> Result<Self, &'static str> {
        const INVALID: &str =
            "SQLolla_PUBLIC_ORIGIN must be a canonical HTTPS origin without a trailing slash";
        let authority = value.strip_prefix("https://").ok_or(INVALID)?;
        let parsed = reqwest::Url::parse(value).map_err(|_| INVALID)?;
        if parsed.origin().ascii_serialization() != value {
            return Err(INVALID);
        }
        Ok(Self {
            value: value.into(),
            authority: authority.into(),
        })
    }

    /// Return the exact allowed browser Origin header.
    pub fn value(&self) -> &str {
        &self.value
    }

    /// Return the exact HTTP authority IIS must preserve in the forwarded Host header.
    pub(crate) fn authority(&self) -> &str {
        &self.authority
    }
}

impl Config {
    /// Resolve only process-level configuration and verify the built frontend exists.
    pub fn from_environment() -> Result<Self, Box<dyn std::error::Error>> {
        let port = std::env::var("SQLolla_HOST_PORT")
            .unwrap_or_else(|_| "8787".into())
            .parse()?;
        let root = std::env::var_os("SQLolla_ROOT")
            .map(PathBuf::from)
            .unwrap_or(std::env::current_dir()?);
        let frontend = root.join("frontend/dist").canonicalize()?;
        if !frontend.join("index.html").is_file() {
            return Err("Build the frontend before starting the host".into());
        }
        let public_origin = match std::env::var("SQLolla_PUBLIC_ORIGIN") {
            Ok(value) => Some(BrowserOrigin::https(&value)?),
            Err(std::env::VarError::NotPresent) => None,
            Err(error) => return Err(error.into()),
        };
        Ok(Self {
            port,
            frontend,
            public_origin,
        })
    }
}

#[cfg(test)]
mod tests;

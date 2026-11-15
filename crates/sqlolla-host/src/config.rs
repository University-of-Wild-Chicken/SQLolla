//! Administrator-owned paths and loopback binding are independent of browser commands.
use std::path::PathBuf;

/// Local server configuration; requests cannot supply filesystem roots or executable paths.
pub struct Config {
    /// Loopback port; zero requests a fresh ephemeral port for integration tests.
    pub port: u16,
    /// Canonical directory containing the self-hosted built frontend.
    pub frontend: PathBuf,
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
        Ok(Self { port, frontend })
    }
}

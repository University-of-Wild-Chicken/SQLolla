//! Portable loopback-only entry point; no credentials are read from process arguments or environment.

/// Install the pinned TLS backend and start the configured local application host.
#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    rustls::crypto::ring::default_provider()
        .install_default()
        .map_err(|_| "TLS provider initialization failed")?;
    sqlolla_host::server::run(sqlolla_host::config::Config::from_environment()?).await
}

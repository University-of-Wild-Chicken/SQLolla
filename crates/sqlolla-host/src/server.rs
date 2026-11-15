//! Same-origin local HTTP wiring; production database execution is not provided.
use crate::{
    command,
    config::{BrowserOrigin, Config},
    security,
    state::HostState,
};
use axum::{
    Router, middleware,
    routing::{get, post},
};
use std::net::{Ipv4Addr, SocketAddr};
use tower_http::services::ServeDir;

/// Compose API routes and fixed frontend assets under one host admission boundary.
pub fn router(state: HostState, frontend: &std::path::Path) -> Router {
    Router::new()
        .route("/api/status", get(command::status))
        .route("/api/command", post(command::command))
        .merge(crate::ai::router())
        .fallback_service(ServeDir::new(frontend))
        .layer(middleware::from_fn_with_state(
            state.clone(),
            security::guard,
        ))
        .with_state(state)
}

/// Listen on IPv4 loopback only and join the canonical worker during graceful shutdown.
pub async fn run(config: Config) -> Result<(), Box<dyn std::error::Error>> {
    let listener =
        tokio::net::TcpListener::bind(SocketAddr::from((Ipv4Addr::LOCALHOST, config.port))).await?;
    let address = listener.local_addr()?;
    let origin = config
        .public_origin
        .unwrap_or_else(|| BrowserOrigin::loopback(address.port()));
    let (state, worker) = HostState::start(origin).await?;
    println!(
        "SQLolla host listening at http://{address}; browser origin {}",
        state.origin().value()
    );
    let app = router(state, &config.frontend);
    let result = axum::serve(listener, app)
        .with_graceful_shutdown(shutdown_signal())
        .await;
    worker.shutdown().await;
    result.map_err(Into::into)
}

/// A user interruption stops new HTTP admission; active work retains its existing bounded lifecycle.
async fn shutdown_signal() {
    let _ = tokio::signal::ctrl_c().await;
}

//! One bounded worker owns Pipeline; provider network operations never hold its execution lane.
use crate::{
    MAX_REQUEST_BYTES,
    config::BrowserOrigin,
    error::HostError,
    worker::{Message, Worker},
};
use axum::http::StatusCode;
use serde_json::Value;
use std::{sync::Arc, time::Duration};
use tokio::sync::{OwnedSemaphorePermit, Semaphore, mpsc, oneshot};

/// Cloneable browser session state contains no provider credentials.
#[derive(Clone)]
pub struct HostState {
    origin: Arc<BrowserOrigin>,
    csrf: Arc<str>,
    sender: mpsc::Sender<Message>,
    admission: Arc<Semaphore>,
}

impl HostState {
    /// Start the sole canonical pipeline owner and await successful worker initialization.
    pub async fn start(origin: BrowserOrigin) -> Result<(Self, Worker), HostError> {
        let csrf = crate::security::nonce()?;
        let (sender, worker) = Worker::start().await?;
        Ok((
            Self {
                origin: origin.into(),
                csrf: csrf.into(),
                sender,
                admission: Arc::new(Semaphore::new(8)),
            },
            worker,
        ))
    }

    /// Return the configured browser origin, independent of the loopback listener.
    pub fn origin(&self) -> &BrowserOrigin {
        &self.origin
    }

    /// Return the memory-only anti-CSRF capability for same-origin clients.
    pub fn csrf_token(&self) -> &str {
        &self.csrf
    }

    /// Bound active API bodies and provider work independently of the canonical queue.
    pub(crate) fn admit(&self) -> Result<OwnedSemaphorePermit, HostError> {
        self.admission.clone().try_acquire_owned().map_err(|_| {
            HostError::new(
                StatusCode::SERVICE_UNAVAILABLE,
                "host.busy",
                "Eight API requests are already active; retry explicitly",
            )
        })
    }

    /// Forward exact UTF-8 bytes; abandoning an active response does not interrupt the pipeline.
    pub async fn dispatch(&self, bytes: Vec<u8>) -> Result<Value, HostError> {
        validate_bytes(&bytes)?;
        let (reply, response) = oneshot::channel();
        self.sender
            .try_send(Message::Command { bytes, reply })
            .map_err(queue_error)?;
        tokio::time::timeout(Duration::from_secs(120), response)
            .await
            .map_err(|_| {
                HostError::new(
                    StatusCode::GATEWAY_TIMEOUT,
                    "host.response_deadline",
                    "Response deadline exceeded; active canonical work may still finish",
                )
            })?
            .map_err(|_| unavailable())?
    }
}

/// Validate transport capacity and encoding without reparsing or rewriting JSON numbers.
fn validate_bytes(bytes: &[u8]) -> Result<(), HostError> {
    if bytes.len() > MAX_REQUEST_BYTES {
        return Err(HostError::new(
            StatusCode::PAYLOAD_TOO_LARGE,
            "host.request_limit",
            "Request exceeds two MiB",
        ));
    }
    std::str::from_utf8(bytes).map(|_| ()).map_err(|_| {
        HostError::new(
            StatusCode::BAD_REQUEST,
            "host.encoding",
            "Request must contain UTF-8 JSON",
        )
    })
}

/// Distinguish a full admission queue from a stopped canonical worker.
fn queue_error(error: mpsc::error::TrySendError<Message>) -> HostError {
    match error {
        mpsc::error::TrySendError::Full(_) => HostError::new(
            StatusCode::SERVICE_UNAVAILABLE,
            "host.busy",
            "Canonical command queue is full; retry explicitly",
        ),
        mpsc::error::TrySendError::Closed(_) => unavailable(),
    }
}

/// Worker failure carries no SQL or exhaustion implication.
fn unavailable() -> HostError {
    HostError::new(
        StatusCode::SERVICE_UNAVAILABLE,
        "host.worker_unavailable",
        "Canonical worker unavailable",
    )
}

#[cfg(test)]
mod tests;

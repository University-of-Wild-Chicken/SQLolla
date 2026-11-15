//! Dedicated synchronous ownership preserves the existing parser and Alloy process boundaries.
use crate::{MAX_RESPONSE_BYTES, error::HostError};
use axum::http::StatusCode;
use serde_json::Value;
use sqlolla::pipeline::Pipeline;
use std::{
    io::{self, Write},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    thread::JoinHandle,
};
use tokio::sync::{mpsc, oneshot};

/// Bounded worker messages contain canonical inputs only, never provider authentication.
pub(crate) enum Message {
    Command {
        bytes: Vec<u8>,
        reply: oneshot::Sender<Result<Value, HostError>>,
    },
    Stop,
}

/// Explicit shutdown joins the owner so dropping Pipeline reaps its private JVM.
pub struct Worker {
    sender: mpsc::Sender<Message>,
    stopping: Arc<AtomicBool>,
    thread: JoinHandle<()>,
}

impl Worker {
    /// Initialize one Pipeline inside its owner thread with at most four waiting commands.
    pub(crate) async fn start() -> Result<(mpsc::Sender<Message>, Self), HostError> {
        let (sender, receiver) = mpsc::channel(4);
        let (ready, initialized) = oneshot::channel();
        let stopping = Arc::new(AtomicBool::new(false));
        let stopped = stopping.clone();
        let thread = std::thread::Builder::new()
            .name("sqlolla-canonical".into())
            .spawn(move || run(receiver, stopped, ready))
            .map_err(|_| startup_error())?;
        if initialized.await.unwrap_or(false) {
            return Ok((
                sender.clone(),
                Self {
                    sender,
                    stopping,
                    thread,
                },
            ));
        }
        let _ = thread.join();
        Err(startup_error())
    }

    /// Finish active bounded work, discard queued requests and drop the canonical process owner.
    pub async fn shutdown(self) {
        self.stopping.store(true, Ordering::Release);
        let _ = self.sender.try_send(Message::Stop);
        let _ = tokio::task::spawn_blocking(move || self.thread.join()).await;
    }
}

/// Closed response receivers cancel only queued work; active computations retain existing budgets.
fn run(
    mut receiver: mpsc::Receiver<Message>,
    stopping: Arc<AtomicBool>,
    ready: oneshot::Sender<bool>,
) {
    let Ok(mut pipeline) = Pipeline::new() else {
        let _ = ready.send(false);
        return;
    };
    if ready.send(true).is_err() {
        return;
    }
    while !stopping.load(Ordering::Acquire) {
        let Some(Message::Command { bytes, reply }) = receiver.blocking_recv() else {
            break;
        };
        dispatch_dequeued(bytes, reply, &stopping, |input| pipeline.handle(input));
    }
}

/// Admission ends after dequeue; shutdown or an abandoned queued reply prevents execution.
fn dispatch_dequeued(
    bytes: Vec<u8>,
    reply: oneshot::Sender<Result<Value, HostError>>,
    stopping: &AtomicBool,
    execute: impl FnOnce(&[u8]) -> Value,
) {
    if stopping.load(Ordering::Acquire) || reply.is_closed() {
        return;
    }
    let _ = reply.send(bounded_response(execute(&bytes)));
}

/// Check serialized capacity without allocating a second complete response buffer.
fn bounded_response(value: Value) -> Result<Value, HostError> {
    let mut count = CountWriter(0);
    serde_json::to_writer(&mut count, &value).map_err(|_| {
        HostError::new(
            StatusCode::PAYLOAD_TOO_LARGE,
            "host.response_limit",
            "Complete response exceeds sixteen MiB",
        )
    })?;
    Ok(value)
}

/// Serialization counter refuses the whole response at the declared boundary.
struct CountWriter(usize);

impl Write for CountWriter {
    /// Count bytes only; no credential, SQL or evidence text is written to a log.
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        self.0 = self
            .0
            .checked_add(bytes.len())
            .filter(|size| *size <= MAX_RESPONSE_BYTES)
            .ok_or_else(|| io::Error::other("response capacity"))?;
        Ok(bytes.len())
    }
    /// The counter has no external buffered state.
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

/// Startup failures omit local command details and never reveal request content.
fn startup_error() -> HostError {
    HostError::new(
        StatusCode::SERVICE_UNAVAILABLE,
        "host.startup",
        "Canonical worker could not start",
    )
}

#[cfg(test)]
mod tests;

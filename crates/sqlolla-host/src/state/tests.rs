//! Pure admission tests do not need a provider key, parser or solver.
use super::*;

/// Construct a deliberately manual worker queue to test backpressure deterministically.
fn state() -> (HostState, mpsc::Receiver<Message>) {
    let (sender, receiver) = mpsc::channel(4);
    (
        HostState {
            origin: BrowserOrigin::loopback(8787).into(),
            csrf: "test".into(),
            sender,
            admission: Arc::new(Semaphore::new(8)),
        },
        receiver,
    )
}

/// Reject invalid text and oversized requests before touching the execution queue.
#[tokio::test]
async fn invalid_transport_never_reaches_worker() {
    let (state, mut receiver) = state();
    assert_eq!(
        state.dispatch(vec![255]).await.unwrap_err().code,
        "host.encoding"
    );
    let oversized = vec![b' '; MAX_REQUEST_BYTES + 1];
    assert_eq!(
        state.dispatch(oversized).await.unwrap_err().code,
        "host.request_limit"
    );
    assert!(receiver.try_recv().is_err());
}

/// Lossless bytes reach the canonical owner and its exact JSON number reaches the caller.
#[tokio::test]
async fn exact_integer_bytes_and_results_survive_dispatch() {
    let (state, mut receiver) = state();
    let raw = br#"{"id":9223372036854775807}"#.to_vec();
    let expected = raw.clone();
    let task = tokio::spawn(async move { state.dispatch(raw).await.unwrap() });
    let Message::Command { bytes, reply } = receiver.recv().await.unwrap() else {
        panic!()
    };
    assert_eq!(bytes, expected);
    reply
        .send(Ok(serde_json::from_slice(&bytes).unwrap()))
        .unwrap();
    assert_eq!(task.await.unwrap()["id"].as_i64(), Some(i64::MAX));
}

/// A closed receiver is a transport failure without any solver exhaustion implication.
#[tokio::test]
async fn stopped_worker_is_explicit() {
    let (state, receiver) = state();
    drop(receiver);
    assert_eq!(
        state.dispatch(b"{}".to_vec()).await.unwrap_err().code,
        "host.worker_unavailable"
    );
}

/// Full queues and total API capacity are refused rather than silently truncating work.
#[tokio::test]
async fn queue_and_api_capacity_are_bounded() {
    let (state, _receiver) = state();
    for _ in 0..4 {
        let (reply, _) = oneshot::channel();
        state
            .sender
            .try_send(Message::Command {
                bytes: vec![],
                reply,
            })
            .unwrap();
    }
    assert_eq!(
        state.dispatch(b"{}".to_vec()).await.unwrap_err().code,
        "host.busy"
    );
    let permits: Vec<_> = (0..8).map(|_| state.admit().unwrap()).collect();
    assert_eq!(state.admit().unwrap_err().code, "host.busy");
    drop(permits);
    assert!(state.admit().is_ok());
}

/// Abandoning the waiting future closes the reply capability so queued work can be skipped.
#[tokio::test]
async fn abandoned_dispatch_closes_reply() {
    let (state, mut receiver) = state();
    let task = tokio::spawn(async move { state.dispatch(b"{}".to_vec()).await });
    let Message::Command { reply, .. } = receiver.recv().await.unwrap() else {
        panic!()
    };
    task.abort();
    let _ = task.await;
    assert!(reply.is_closed());
}

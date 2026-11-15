//! Response resource gates preserve exact values and never publish partial evidence.
use super::*;

/// The byte counter accepts its final byte and rejects overflow without writing data anywhere.
#[test]
fn counter_has_exact_capacity() {
    let mut writer = CountWriter(MAX_RESPONSE_BYTES - 1);
    assert_eq!(writer.write(b"x").unwrap(), 1);
    assert!(writer.write(b"x").is_err());
}

/// Oversized evidence fails as a transport resource error rather than truncating a record.
#[test]
fn complete_responses_preserve_numbers_or_fail() {
    let value = serde_json::json!({"value":i64::MAX,"status":"EXHAUSTED"});
    assert_eq!(bounded_response(value.clone()).unwrap(), value);
    let error = bounded_response(Value::String("x".repeat(MAX_RESPONSE_BYTES))).unwrap_err();
    assert_eq!(error.code, "host.response_limit");
}

/// Shutdown racing after dequeue cannot execute that queued command or fabricate a solver result.
#[test]
fn stopping_after_dequeue_discards_pending_execution() {
    let stopping = AtomicBool::new(false);
    let (reply, mut response) = oneshot::channel();
    let dequeued = (b"canonical command".to_vec(), reply);
    stopping.store(true, Ordering::Release);
    dispatch_dequeued(dequeued.0, dequeued.1, &stopping, |_| {
        panic!("A queued command crossed the shutdown boundary")
    });
    assert!(matches!(
        response.try_recv(),
        Err(oneshot::error::TryRecvError::Closed)
    ));
}

/// Cancellation after dequeue but before admission prevents a queued computation as well.
#[test]
fn abandoned_after_dequeue_never_executes() {
    let (reply, response) = oneshot::channel();
    drop(response);
    dispatch_dequeued(vec![], reply, &AtomicBool::new(false), |_| {
        panic!("An abandoned queued command was executed")
    });
}

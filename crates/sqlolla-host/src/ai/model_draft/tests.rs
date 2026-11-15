use super::*;
use crate::config::BrowserOrigin;

/// Simulate a provider building a model unrelated to the shipped regression fixture.
fn sample() -> Value {
    json!({"context_sha256":"selected", "relations":[{"name":"shipments","bound":1,
        "columns":[{"name":"elapsed","ty":"int64","nullable":false}]}],
        "values":[{"ty":"int64","value":"9223372036854775807"}],
        "rule":{"kind":"max_rows","column":null,"max":0,"value":null},
        "sql":"SELECT elapsed FROM shipments","assumptions":[]})
}

/// Local validation refuses duplicate relations and a domain without a column type's values.
#[test]
fn refuses_invalid_models() {
    let mut missing = sample();
    missing["values"] = json!([]);
    assert!(request(&parse(missing, "selected").unwrap()).is_err());
    let mut duplicate = sample();
    duplicate["relations"] = json!([duplicate["relations"][0], duplicate["relations"][0]]);
    assert!(request(&parse(duplicate, "selected").unwrap()).is_err());
    assert!(parse(sample(), "wrong input").is_err());
}

/// Replay the actual host compiler with the exact provider proposal and no egress.
#[tokio::test]
async fn compiles_new_model_and_rejects_unknown_table() {
    let proposed = parse(sample(), "selected").unwrap();
    let request = request(&proposed).unwrap();
    assert_eq!(json!(request.domain[0])["value"].as_i64(), Some(i64::MAX));
    let (host, worker) = HostState::start(BrowserOrigin::loopback(8787))
        .await
        .unwrap();
    let accepted = draft::compile(&host, request.clone(), &proposed.sql)
        .await
        .unwrap();
    assert_eq!(
        draft::assessment(&accepted)["status"],
        "compiled",
        "{accepted}"
    );
    let denied = draft::compile(&host, request, "SELECT elapsed FROM missing")
        .await
        .unwrap();
    assert_eq!(draft::assessment(&denied)["status"], "refused");
    worker.shutdown().await;
}

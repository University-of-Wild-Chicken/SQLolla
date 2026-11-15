//! Real loopback HTTP, deterministic pipeline and Alloy enumeration without an AI credential.
mod support;
use serde_json::{Value, json};
use support::Client;

/// Run the child test in a dedicated store without mutating the multithreaded parent's environment.
#[test]
fn live_host_roundtrip() {
    let directory = support::directory();
    let result = std::process::Command::new(std::env::current_exe().unwrap())
        .args(["--exact", "live_http_child", "--ignored", "--nocapture"])
        .env("SQLolla_HOST_TEST_CHILD", "1")
        .env("SQLolla_INSTANCE_STORE", directory.join("store"))
        .env("SQLolla_HOST_TEST_DIRECTORY", &directory)
        .output()
        .unwrap();
    support::cleanup(&directory);
    assert!(
        result.status.success(),
        "{}\n{}",
        String::from_utf8_lossy(&result.stdout),
        String::from_utf8_lossy(&result.stderr)
    );
}

/// The parent invokes this ignored entry with isolated process configuration and no provider key.
#[tokio::test]
#[ignore = "Invoked by live_host_roundtrip with an isolated evidence store"]
async fn live_http_child() {
    assert_eq!(std::env::var("SQLolla_HOST_TEST_CHILD").unwrap(), "1");
    rustls::crypto::ring::default_provider()
        .install_default()
        .unwrap();
    let service = support::Service::start().await;
    let client = &service.client;
    transport(client).await;
    concrete(client).await;
    enumeration(
        client,
        "SELECT x FROM r",
        "native_ra_alloy_with_rust_replay",
    )
    .await;
    enumeration(
        client,
        "SELECT x+0 FROM r",
        "evaluator_backed_table_with_rust_replay",
    )
    .await;
    service.stop().await;
}

/// Exercise actual middleware and static serving, including same-origin refusal before dispatch.
async fn transport(client: &Client) {
    let page = client.http.get(&client.origin).send().await.unwrap();
    assert_eq!(page.status(), 200);
    assert_eq!(page.headers()["cache-control"], "no-store");
    assert_eq!(page.text().await.unwrap(), "SQLolla isolated host test");
    let status = client
        .http
        .get(format!("{}/api/status", client.origin))
        .send()
        .await
        .unwrap();
    let value: Value = status.json().await.unwrap();
    assert_eq!(value["schema"], "sqlolla.host.v1");
    assert_eq!(value["model"], "gpt-6.1-sol");
    assert_eq!(value["csrf_token"], client.csrf);
    let refusal = client
        .http
        .post(format!("{}/api/command", client.origin))
        .header("origin", "https://attacker.invalid")
        .json(&json!({}))
        .send()
        .await
        .unwrap();
    assert_eq!(refusal.status(), 403);
    assert_eq!(
        refusal.json::<Value>().await.unwrap()["code"],
        "host.origin"
    );
}

/// Actual SQL execution preserves i64 values and typed NULLs without a JavaScript-number roundtrip.
async fn concrete(client: &Client) {
    let request = json!({"id":i64::MAX,"sql":"SELECT $1, NULL::bigint", "mode":"compare",
        "catalog":{"tables":{},"search_path":["public"]}, "execution":{"max_outcomes":256,"max_rows":256,"settings":{}},
        "parameters":{"1":{"type":"int64","value":i64::MAX}}});
    let result = client.command(&request).await;
    assert_eq!(result["id"].as_i64(), Some(i64::MAX));
    assert_eq!(result["status"], "ok", "{result}");
    assert_eq!(
        result["outcomes"][0]["outcome"]["rows"][0][0]["value"].as_i64(),
        Some(i64::MAX)
    );
    assert_eq!(
        result["outcomes"][0]["outcome"]["rows"][0][1],
        json!({"type":"null","value":"int64"})
    );
    let invalid = client
        .command(&json!({"action":"instances.next","cursor":"../../outside"}))
        .await;
    assert_eq!(invalid["status"], "error");
    assert_eq!(invalid["enumeration_exhausted"], false);
}

/// Replayed real SAT records reopen with their source triple, then advance to actual check UNSAT.
async fn enumeration(client: &Client, sql: &str, assurance: &str) {
    let start = client
        .command(&json!({"action":"instances.start","kind":"counterexample",
        "request":symbolic(sql),"max_instances":8}))
        .await;
    replayed(&start, assurance);
    let open = client
        .command(&json!({"action":"instances.open","bundle_id":start["bundle_id"]}))
        .await;
    assert_eq!(open["record"], start["record"]);
    assert_eq!(open["bundle"]["request"]["sql"], sql);
    assert!(
        open["bundle"]["model"]
            .as_str()
            .unwrap()
            .contains("check QueryProperty")
    );
    assert!(open["bundle"]["ir"].is_object());
    let next = client
        .command(&json!({"action":"instances.next","cursor":start["cursor"]}))
        .await;
    terminal(&next);
    let reopened = client
        .command(&json!({"action":"instances.open","bundle_id":start["bundle_id"]}))
        .await;
    assert_eq!(reopened["records"].as_array().unwrap().len(), 2);
}

/// The returned counterexample keeps its encoding provenance and independent replay label.
fn replayed(start: &Value, assurance: &str) {
    assert_eq!(start["status"], "ok", "{start}");
    assert_eq!(start["assurance_kind"], assurance);
    assert_eq!(start["record"]["solver"]["command_kind"], "check");
    assert_eq!(
        start["record"]["instance"]["validation"],
        "RUST_REPLAY_CONFIRMED"
    );
}

/// Only an explicit check UNSAT result may label this finite journal exhausted.
fn terminal(next: &Value) {
    assert_eq!(next["enumeration_status"], "exhausted", "{next}");
    assert_eq!(next["record"]["solver"]["status"], "UNSAT");
    assert_eq!(next["enumeration_exhausted"], true);
}

/// There is exactly one violating input bag under this declared finite profile.
fn symbolic(sql: &str) -> Value {
    json!({"sql":sql,"mode":"analyze","bounds":{"r":1},"domain":[{"type":"int32","value":1}],
        "property":{"kind":"max_rows","max":0},"catalog":{"search_path":["public"],"tables":{
            "r":{"schema":"public","name":"r","columns":[{"name":"x","ty":"int32","nullable":false}]}}}})
}

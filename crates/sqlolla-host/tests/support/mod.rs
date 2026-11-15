//! Process-isolated HTTP harness owns its assets, archive and graceful worker shutdown.
use serde_json::Value;
use sqlolla_host::{Worker, state::HostState};
use std::path::{Path, PathBuf};
use tokio::{sync::oneshot, task::JoinHandle};

/// Unique owned paths never point at the user's evidence store or frontend tree.
pub fn directory() -> PathBuf {
    let mut nonce = [0u8; 16];
    getrandom::fill(&mut nonce).unwrap();
    let suffix: String = nonce.iter().map(|byte| format!("{byte:02x}")).collect();
    let path = std::env::temp_dir().join(format!("sqlolla-host-test-{suffix}"));
    std::fs::create_dir(&path).unwrap();
    path.canonicalize().unwrap()
}

/// Remove only the exact nonce-named child created by this harness after all workers stop.
pub fn cleanup(path: &Path) {
    let parent = std::env::temp_dir().canonicalize().unwrap();
    assert_eq!(path.parent(), Some(parent.as_path()));
    assert!(
        path.file_name()
            .unwrap()
            .to_str()
            .unwrap()
            .starts_with("sqlolla-host-test-")
    );
    std::fs::remove_dir_all(path).unwrap();
}

/// Real HTTP client retains only a local anti-CSRF capability, never a provider credential.
pub struct Client {
    pub http: reqwest::Client,
    pub origin: String,
    pub csrf: String,
}

impl Client {
    /// Submit the canonical action as UTF-8 bytes and require a complete HTTP JSON response.
    pub async fn command(&self, value: &Value) -> Value {
        let response = self
            .http
            .post(format!("{}/api/command", self.origin))
            .header("origin", &self.origin)
            .header("x-sqlolla-csrf", &self.csrf)
            .header("content-type", "application/json")
            .body(serde_json::to_vec(value).unwrap())
            .send()
            .await
            .unwrap();
        assert_eq!(response.status(), 200);
        response.json().await.unwrap()
    }
}

/// A real router and Pipeline share one explicitly owned lifetime for the entire test.
pub struct Service {
    pub client: Client,
    worker: Worker,
    shutdown: oneshot::Sender<()>,
    server: JoinHandle<()>,
}

impl Service {
    /// Bind only an ephemeral loopback port and prepare a tiny task-owned static directory.
    pub async fn start() -> Self {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let origin = format!("http://{}", listener.local_addr().unwrap());
        let (state, worker) = HostState::start(origin.clone()).await.unwrap();
        let csrf = state.csrf_token().to_owned();
        let path = PathBuf::from(std::env::var_os("SQLolla_HOST_TEST_DIRECTORY").unwrap());
        std::fs::write(path.join("index.html"), "SQLolla isolated host test").unwrap();
        let app = sqlolla_host::server::router(state, &path);
        let (shutdown, stopped) = oneshot::channel();
        let server = tokio::spawn(async move {
            axum::serve(listener, app)
                .with_graceful_shutdown(async {
                    let _ = stopped.await;
                })
                .await
                .unwrap();
        });
        let http = reqwest::Client::builder()
            .no_proxy()
            .timeout(std::time::Duration::from_secs(120))
            .build()
            .unwrap();
        Self {
            client: Client { http, origin, csrf },
            worker,
            shutdown,
            server,
        }
    }

    /// Stop HTTP first, then join Pipeline's owner to reap its private JVM before cleanup.
    pub async fn stop(self) {
        let _ = self.shutdown.send(());
        self.server.await.unwrap();
        self.worker.shutdown().await;
    }
}

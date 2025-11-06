#![allow(dead_code)]

use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use axum::{body::Body, http, Router};
use tokio::net::TcpListener;
use tokio::task::JoinHandle;

/// Utility module for proxy / conversion integration tests.
///
/// This spawns the real router (including `/convert`, `/proxy`, `/chat/completions`, etc.)
/// bound to an ephemeral local port, returning a `TestServer` with convenience helpers.
///
/// By default we put the server in "passthrough" mode unless `managed_mode` is requested.
/// Managed mode = `OPENAI_API_KEY` is set; the proxy will expect Authorization bearer tokens
/// that match internally issued keys (the tests can still simulate this by forging keys
/// if they bypass verification or if they construct a key manager).
///
/// IMPORTANT:
/// - Any test using outbound proxy endpoints that *contact* an upstream will need a reachable
///   `OPENAI_BASE_URL`. For purely local tests (e.g. `/status`, `/convert`), we can supply
///   a dummy base URL such as `http://127.0.0.1:9/v1` (port 9 is discard and will fail fast
///   if accidentally hit). Tests should avoid hitting `/proxy` unless they either:
///     a) mock upstream networking (via a test HTTP server), or
///     b) point `OPENAI_BASE_URL` to that mock server.
///
/// - This module does not supply a mock upstream to keep dependencies minimal.
pub struct TestServer {
    pub base_url: String,
    pub addr: SocketAddr,
    join: JoinHandle<()>,
    client: reqwest::Client,
}

impl TestServer {
    /// Create a reqwest client with sensible defaults for tests.
    fn make_client() -> reqwest::Client {
        reqwest::Client::builder()
            .timeout(Duration::from_secs(10))
            .build()
            .expect("failed building reqwest client")
    }

    /// Perform a GET relative to the server base URL.
    pub async fn get(&self, path: &str) -> reqwest::Result<reqwest::Response> {
        self.client
            .get(format!("{}{}", self.base_url, path))
            .send()
            .await
    }

    /// Perform a POST with JSON body.
    pub async fn post_json<T: serde::Serialize>(
        &self,
        path: &str,
        body: &T,
        auth_bearer: Option<&str>,
    ) -> reqwest::Result<reqwest::Response> {
        let mut rb = self
            .client
            .post(format!("{}{}", self.base_url, path))
            .header(http::header::CONTENT_TYPE, "application/json");
        if let Some(b) = auth_bearer {
            rb = rb.bearer_auth(b);
        }
        rb.json(body).send().await
    }

    /// Low-level POST with raw bytes.
    pub async fn post_bytes(
        &self,
        path: &str,
        bytes: Vec<u8>,
        content_type: &str,
        auth_bearer: Option<&str>,
    ) -> reqwest::Result<reqwest::Response> {
        let mut rb = self
            .client
            .post(format!("{}{}", self.base_url, path))
            .header(http::header::CONTENT_TYPE, content_type);
        if let Some(b) = auth_bearer {
            rb = rb.bearer_auth(b);
        }
        rb.body(bytes).send().await
    }
}

impl Drop for TestServer {
    fn drop(&mut self) {
        self.join.abort();
    }
}

/// Spawn the application router on an ephemeral port.
/// - `managed_mode`: if true, sets `OPENAI_API_KEY` so that the proxy operates
///   in internal-key mode.
/// - `custom_env`: optional slice of `(key, value)` to set additional environment variables.
///
/// NOTE: This function mutates process environment variables. For parallel test
/// execution, prefer serializing tests that call this or use per-test processes.
pub async fn spawn_test_app(managed_mode: bool, custom_env: &[(&str, &str)]) -> TestServer {
    // Always set OPENAI_BASE_URL; required by `openai_base_url()`.
    // Use a dummy default; tests that call upstream endpoints should override.
    if std::env::var("OPENAI_BASE_URL").is_err() {
        std::env::set_var("OPENAI_BASE_URL", "http://127.0.0.1:9/v1");
    }

    if managed_mode {
        if std::env::var("OPENAI_API_KEY").is_err() {
            // Provide a fake internal upstream key; not used unless /proxy is actually invoked.
            std::env::set_var("OPENAI_API_KEY", "sk-internal-test-upstream");
        }
    } else {
        // Ensure it is cleared so the app enters passthrough mode.
        std::env::remove_var("OPENAI_API_KEY");
    }

    for (k, v) in custom_env {
        std::env::set_var(k, v);
    }

    // Build app state via existing builder.
    let app = routiium::server::build_router();

    // Bind ephemeral.
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind ephemeral port");
    let addr = listener.local_addr().expect("local addr");
    let base_url = format!("http://{}", addr);
    let server = axum::serve(listener, app.into_make_service());

    let join = tokio::spawn(async move {
        if let Err(e) = server.await {
            eprintln!("Test server error: {e:?}");
        }
    });

    TestServer {
        base_url,
        addr,
        join,
        client: TestServer::make_client(),
    }
}

/// Convenience helper: spawn in passthrough mode.
pub async fn spawn_passthrough() -> TestServer {
    spawn_test_app(false, &[]).await
}

/// Convenience helper: spawn in managed mode (internal upstream key).
pub async fn spawn_managed() -> TestServer {
    spawn_test_app(true, &[]).await
}

/// Build a minimal Chat Completions style request body for tests.
pub fn sample_chat_request() -> serde_json::Value {
    serde_json::json!({
        "model": "gpt-4o-mini",
        "messages": [
            {"role":"user","content":"Hello test server"}
        ]
    })
}

/// Build a minimal streaming Chat Completions style request body.
pub fn sample_streaming_chat_request() -> serde_json::Value {
    serde_json::json!({
        "model": "gpt-4o-mini",
        "messages": [
            {"role":"user","content":"Stream please"}
        ],
        "stream": true
    })
}

/// Build a minimal Chat Completions body with an internal tool call hint (for MCP or tool tests).
pub fn sample_tool_chat_request(tool_name: &str) -> serde_json::Value {
    serde_json::json!({
        "model": "gpt-4o-mini",
        "messages": [
            {"role":"user","content":"Use a tool"}
        ],
        "tools": [{
            "type":"function",
            "function":{
                "name": tool_name,
                "description":"A test tool",
                "parameters":{"type":"object","properties":{}}
            }
        }]
    })
}

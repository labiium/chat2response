use axum::response::{IntoResponse, Response};
use http::StatusCode;
use tracing_subscriber::{fmt, EnvFilter};

/// Initialize dotenv and structured tracing based on RUST_LOG.
/// Enhanced:
/// - Supports explicit env file paths via ENV_FILE, ENVFILE, DOTENV_PATH
/// - Falls back to .envfile, then default .env
/// - If all fail, tries a tolerant manual parser for ./.env (no overwrite of existing vars)
/// - Logs the source used
pub fn init_tracing() {
    // Try explicit environment file variables first
    let mut env_source: String = "none".into();
    for key in ["ENV_FILE", "ENVFILE", "DOTENV_PATH"] {
        if let Ok(p) = std::env::var(key) {
            let p = p.trim();
            if !p.is_empty()
                && std::path::Path::new(p).is_file()
                && dotenvy::from_filename(p).is_ok()
            {
                env_source = format!("{p} ({key})");
                break;
            }
        }
    }

    // Next, support conventional ".envfile"
    if env_source == "none"
        && std::path::Path::new(".envfile").is_file()
        && dotenvy::from_filename(".envfile").is_ok()
    {
        env_source = ".envfile".into();
    }

    // Default to standard ".env" discovery in current working directory
    if env_source == "none" && dotenvy::dotenv().is_ok() {
        env_source = ".env".into();
    }

    // If still not found, search upward from the executable directory for a .env file.
    if env_source == "none" {
        if let Ok(exe) = std::env::current_exe() {
            let mut dir_opt = exe.parent();
            while let Some(dir) = dir_opt {
                let candidate = dir.join(".env");
                if candidate.is_file() && dotenvy::from_filename(&candidate).is_ok() {
                    env_source = candidate.display().to_string();
                    break;
                }
                dir_opt = dir.parent();
            }
        }
    }

    // Tolerant manual parser: if still none, try reading "./.env" and set keys not already present.
    if env_source == "none" {
        if let Ok(cwd) = std::env::current_dir() {
            let candidate = cwd.join(".env");
            if candidate.is_file() {
                if let Ok(text) = std::fs::read_to_string(&candidate) {
                    let mut loaded = 0usize;
                    for raw in text.lines() {
                        let line = raw.trim();
                        if line.is_empty() || line.starts_with('#') || !line.contains('=') {
                            continue;
                        }
                        let mut parts = line.splitn(2, '=');
                        if let (Some(k), Some(v)) = (parts.next(), parts.next()) {
                            let key = k.trim();
                            if key.is_empty() || std::env::var_os(key).is_some() {
                                continue; // don't overwrite existing env
                            }
                            let mut val = v.trim().to_string();
                            // Strip surrounding single or double quotes if present
                            if (val.starts_with('"') && val.ends_with('"'))
                                || (val.starts_with('\'') && val.ends_with('\''))
                            {
                                val = val[1..val.len().saturating_sub(1)].to_string();
                            }
                            std::env::set_var(key, val);
                            loaded += 1;
                        }
                    }
                    if loaded > 0 {
                        env_source = format!("{} (manual)", candidate.display());
                    }
                }
            }
        }
    }

    // Initialize tracing (respects RUST_LOG potentially provided by the env file)
    let filter = std::env::var("RUST_LOG").unwrap_or_else(|_| "info,tower_http=info".into());
    let subscriber = fmt().with_env_filter(EnvFilter::new(filter)).finish();
    let _ = tracing::subscriber::set_global_default(subscriber);

    // Log where the environment was loaded from for observability
    tracing::info!("Environment loaded from: {}", env_source);
}

/// Get the bind address for the HTTP server from env or default to 0.0.0.0:8088.
pub fn env_bind_addr() -> String {
    std::env::var("BIND_ADDR").unwrap_or_else(|_| "0.0.0.0:8088".into())
}

/// Shared application state used by the HTTP server and handlers.
pub struct AppState {
    pub http: reqwest::Client,
    pub mcp_manager: Option<std::sync::Arc<crate::mcp_client::McpClientManager>>,
    /// Optional API key manager for inbound auth (generation/expiration/revocation handled in crate::auth)
    pub api_keys: Option<std::sync::Arc<crate::auth::ApiKeyManager>>,
}

/// Build an HTTP client honoring proxy and timeout environment variables.
///
/// Environment:
/// - CHAT2RESPONSE_NO_PROXY = 1|true|yes|on  -> disable all proxies
/// - CHAT2RESPONSE_PROXY_URL = <url>         -> proxy for all schemes
/// - HTTP_PROXY / http_proxy                 -> HTTP proxy
/// - HTTPS_PROXY / https_proxy               -> HTTPS proxy
/// - CHAT2RESPONSE_HTTP_TIMEOUT_SECONDS      -> overall request timeout (u64)
pub fn build_http_client_from_env() -> reqwest::Client {
    let mut builder = reqwest::Client::builder();

    // Optional timeout
    if let Ok(secs) = std::env::var("CHAT2RESPONSE_HTTP_TIMEOUT_SECONDS") {
        if let Ok(n) = secs.trim().parse::<u64>() {
            builder = builder.timeout(std::time::Duration::from_secs(n));
        }
    }

    // Proxy configuration
    let no_proxy = std::env::var("CHAT2RESPONSE_NO_PROXY")
        .map(|v| v.trim().to_ascii_lowercase())
        .map(|v| v == "1" || v == "true" || v == "yes" || v == "on")
        .unwrap_or(false);

    if no_proxy {
        builder = builder.no_proxy();
    } else {
        // All-scheme proxy
        if let Ok(url) = std::env::var("CHAT2RESPONSE_PROXY_URL") {
            let u = url.trim();
            if !u.is_empty() {
                if let Ok(p) = reqwest::Proxy::all(u) {
                    builder = builder.proxy(p);
                }
            }
        }
        // Scheme-specific proxies
        if let Ok(http_p) = std::env::var("HTTP_PROXY").or_else(|_| std::env::var("http_proxy")) {
            let u = http_p.trim();
            if !u.is_empty() {
                if let Ok(p) = reqwest::Proxy::http(u) {
                    builder = builder.proxy(p);
                }
            }
        }
        if let Ok(https_p) = std::env::var("HTTPS_PROXY").or_else(|_| std::env::var("https_proxy"))
        {
            let u = https_p.trim();
            if !u.is_empty() {
                if let Ok(p) = reqwest::Proxy::https(u) {
                    builder = builder.proxy(p);
                }
            }
        }
    }

    // User-Agent for observability
    builder = builder.user_agent(format!("chat2response/{}", env!("CARGO_PKG_VERSION")));

    builder.build().unwrap_or_else(|_| reqwest::Client::new())
}

impl Default for AppState {
    fn default() -> Self {
        Self {
            http: build_http_client_from_env(),
            mcp_manager: None,
            api_keys: (|| {
                if let Ok(url) = std::env::var("CHAT2RESPONSE_REDIS_URL") {
                    let u = url.trim().to_string();
                    if !u.is_empty() {
                        if let Ok(m) = crate::auth::ApiKeyManager::new_with_redis_url(&u) {
                            return Some(std::sync::Arc::new(m));
                        }
                    }
                }
                crate::auth::ApiKeyManager::new_default()
                    .ok()
                    .map(std::sync::Arc::new)
            })(),
        }
    }
}

impl AppState {
    /// Create AppState with MCP manager
    pub fn with_mcp_manager(mcp_manager: crate::mcp_client::McpClientManager) -> Self {
        Self {
            http: build_http_client_from_env(),
            mcp_manager: Some(std::sync::Arc::new(mcp_manager)),
            api_keys: (|| {
                if let Ok(url) = std::env::var("CHAT2RESPONSE_REDIS_URL") {
                    let u = url.trim().to_string();
                    if !u.is_empty() {
                        if let Ok(m) = crate::auth::ApiKeyManager::new_with_redis_url(&u) {
                            return Some(std::sync::Arc::new(m));
                        }
                    }
                }
                crate::auth::ApiKeyManager::new_default()
                    .ok()
                    .map(std::sync::Arc::new)
            })(),
        }
    }

    /// Create AppState with MCP manager wrapped in Arc
    pub fn with_mcp_manager_arc(
        mcp_manager: std::sync::Arc<crate::mcp_client::McpClientManager>,
    ) -> Self {
        Self {
            http: build_http_client_from_env(),
            mcp_manager: Some(mcp_manager),
            api_keys: (|| {
                if let Ok(url) = std::env::var("CHAT2RESPONSE_REDIS_URL") {
                    let u = url.trim().to_string();
                    if !u.is_empty() {
                        if let Ok(m) = crate::auth::ApiKeyManager::new_with_redis_url(&u) {
                            return Some(std::sync::Arc::new(m));
                        }
                    }
                }
                crate::auth::ApiKeyManager::new_default()
                    .ok()
                    .map(std::sync::Arc::new)
            })(),
        }
    }
    /// Read the OpenAI API key from environment if present. Optional for /proxy.
    pub fn api_key(&self) -> String {
        std::env::var("OPENAI_API_KEY").unwrap_or_default()
    }

    /// Verify incoming Authorization: Bearer header against the API key manager (if configured).
    /// Returns None if no manager was configured.
    pub fn verify_bearer_header(
        &self,
        headers: &http::HeaderMap,
    ) -> Option<crate::auth::Verification> {
        let auth = headers
            .get(http::header::AUTHORIZATION)
            .and_then(|v| v.to_str().ok());
        self.api_keys
            .as_ref()
            .map(|m| crate::auth::verify_bearer(m.as_ref(), auth))
    }
}

/// Build a JSON error response with the given HTTP status and message.
pub fn error_response(status: StatusCode, msg: &str) -> Response {
    let body = serde_json::json!({ "error": { "message": msg } });
    (status, axum::Json(body)).into_response()
}

/// Resolve the OpenAI base URL from environment or use the default public endpoint.
pub fn openai_base_url() -> String {
    std::env::var("OPENAI_BASE_URL").expect("OPENAI_BASE_URL not set (mandatory)")
}

/// Forward a request upstream with streaming enabled and return an SSE response.
///
/// Behavior:
/// - Default mode ("responses"): POST to `{base_url}/responses` with Responses-shaped payload
/// - Chat mode (UPSTREAM_MODE = "chat"|"chat-completions"): rewrite the URL suffix
///   to `/chat/completions` and transform the payload into Chat Completions JSON
///
/// Note: For very long streams, consider a true streaming passthrough (hyper upgrade or
/// axum streaming body). This implementation buffers the upstream SSE into memory.
pub async fn sse_proxy_stream(
    client: &reqwest::Client,
    url: &str,
    payload: &serde_json::Value,
) -> Result<Response, anyhow::Error> {
    use axum::body::Body;
    use bytes::Bytes;
    use futures_util::TryStreamExt;
    use http::header;

    let api_key = std::env::var("OPENAI_API_KEY")
        .ok()
        .filter(|s| !s.is_empty());

    // Determine upstream mode from env
    // Accepts: "responses" (default), "chat", "chat-completions", "chat_completions"
    let upstream_mode = std::env::var("UPSTREAM_MODE")
        .or_else(|_| std::env::var("CHAT2RESPONSE_UPSTREAM"))
        .unwrap_or_else(|_| "responses".to_string())
        .to_lowercase();

    // Rewrite URL (only for chat mode): .../responses -> .../chat/completions
    let mut real_url = url.to_string();
    let is_chat_mode = matches!(
        upstream_mode.as_str(),
        "chat" | "chat-completions" | "chat_completions"
    );
    if is_chat_mode {
        if let Some(pos) = real_url.rfind("/responses") {
            real_url.replace_range(pos.., "/chat/completions");
        }
    }

    // Rewrite payload for chat mode:
    // - If payload has `input`, convert to Chat's `messages`
    // - Map `max_output_tokens` -> `max_tokens`
    // - Remove Responses-only fields (e.g., conversation)
    // - Ensure `stream: true` for SSE
    let mut body = payload.clone();
    if is_chat_mode {
        if let Some(obj) = body.as_object_mut() {
            if obj.get("messages").is_none() {
                if let Some(input) = obj.remove("input") {
                    match input {
                        serde_json::Value::Array(_) => {
                            obj.insert("messages".to_string(), input);
                        }
                        serde_json::Value::String(s) => {
                            obj.insert(
                                "messages".to_string(),
                                serde_json::json!([{"role":"user","content": s}]),
                            );
                        }
                        _ => {}
                    }
                }
            }
            if let Some(max_out) = obj.remove("max_output_tokens") {
                obj.insert("max_tokens".to_string(), max_out);
            }
            obj.remove("conversation");
            obj.insert("stream".to_string(), serde_json::Value::Bool(true));
        }
    }

    let mut rb = client
        .post(&real_url)
        .header(header::ACCEPT, "text/event-stream")
        .header(header::CONTENT_TYPE, "application/json")
        .header(header::CONNECTION, "close")
        .json(&body);
    if let Some(k) = api_key {
        if !k.is_empty() {
            rb = rb.bearer_auth(k);
        }
    }
    let resp = rb.send().await?;

    let status = resp.status();
    if !status.is_success() {
        let bytes = resp.bytes().await.unwrap_or_default();
        return Ok((status, bytes).into_response());
    }

    // Stream SSE passthrough without buffering the entire response.
    let upstream_ct = resp.headers().get(header::CONTENT_TYPE).cloned();
    let stream = resp
        .bytes_stream()
        .map_err(|e| std::io::Error::other(e.to_string()))
        .map_ok(Bytes::from);

    let mut builder = http::Response::builder().status(StatusCode::OK);
    if let Some(ct) = upstream_ct {
        builder = builder.header(header::CONTENT_TYPE, ct);
    } else {
        builder = builder.header(header::CONTENT_TYPE, "text/event-stream");
    }
    let response = builder
        .header("Cache-Control", "no-cache")
        .header("Connection", "keep-alive")
        .body(Body::from_stream(stream))
        .unwrap();

    Ok(response)
}

/// Best-effort derivation of a single-string "input" from a Responses-shaped payload.
fn derive_input_string(payload: &serde_json::Value) -> String {
    // Prefer last user message text; else concatenate text-like parts; else empty string.
    let mut derived: Option<String> = None;

    if let Some(msgs) = payload.get("messages").and_then(|m| m.as_array()) {
        // Pick last user; else last message
        let mut candidate = None;
        for m in msgs.iter().rev() {
            if let Some(role) = m.get("role").and_then(|r| r.as_str()) {
                if role == "user" {
                    candidate = Some(m);
                    break;
                }
            }
            if candidate.is_none() {
                candidate = Some(m);
            }
        }
        if let Some(m) = candidate {
            if let Some(content) = m.get("content") {
                match content {
                    serde_json::Value::String(s) => derived = Some(s.clone()),
                    serde_json::Value::Array(parts) => {
                        let mut pieces = Vec::new();
                        for p in parts {
                            if let Some(ty) = p.get("type").and_then(|t| t.as_str()) {
                                if ty == "text" || ty == "input_text" {
                                    if let Some(t) = p.get("text").and_then(|t| t.as_str()) {
                                        pieces.push(t.to_string());
                                    }
                                }
                            }
                        }
                        if !pieces.is_empty() {
                            derived = Some(pieces.join("\n"));
                        }
                    }
                    _ => {}
                }
            }
        }
    }

    derived.unwrap_or_default()
}

/// Non-streaming POST helper with a single retry when upstream requires top-level 'input'.
///
/// Behavior:
/// - Sends JSON payload as-is with Bearer auth.
/// - If 400 and the response body hints that 'input' is required, derive an 'input' string
///   from messages and retry once with that field added.
/// - Returns the upstream response (first or retried).
pub async fn post_responses_with_input_retry(
    client: &reqwest::Client,
    url: &str,
    payload: &serde_json::Value,
    bearer: Option<String>,
) -> Result<Response, anyhow::Error> {
    let effective_bearer = bearer
        .and_then(|s| if s.is_empty() { None } else { Some(s) })
        .or_else(|| {
            std::env::var("OPENAI_API_KEY")
                .ok()
                .filter(|s| !s.is_empty())
        });
    let mut req = client
        .post(url)
        .header(http::header::CONTENT_TYPE, "application/json");
    if let Some(key) = effective_bearer.clone() {
        req = req.bearer_auth(key);
    }
    let first = req.try_clone().unwrap().json(payload).send().await?;
    let status = first.status();
    if status != http::StatusCode::BAD_REQUEST {
        let bytes = first.bytes().await.unwrap_or_default();
        return Ok((status, bytes).into_response());
    }
    let body_bytes = first.bytes().await.unwrap_or_default();
    let body_text = String::from_utf8_lossy(&body_bytes);
    // Heuristic: look for "input" and "missing" in the error text.
    let needs_input = body_text.contains("'input'")
        || body_text.contains("\"input\"")
        || body_text.contains("Field required") && body_text.contains("input");
    if !needs_input {
        return Ok((status, body_bytes).into_response());
    }

    // Retry with derived input injected (do not overwrite if already present).
    let mut patched = payload.clone();
    let s = derive_input_string(&patched);
    if let Some(obj) = patched.as_object_mut() {
        if !obj.contains_key("input") {
            obj.insert("input".into(), serde_json::Value::String(s));
        }
    }

    let mut second_req = client
        .post(url)
        .header(http::header::CONTENT_TYPE, "application/json");
    if let Some(key) = effective_bearer {
        second_req = second_req.bearer_auth(key);
    }
    let second = second_req.json(&patched).send().await?;

    let status2 = second.status();
    let bytes2 = second.bytes().await.unwrap_or_default();
    Ok((status2, bytes2).into_response())
}

/// Simple HTTP GET helper supporting optional Bearer auth and proxy (via provided client).
pub async fn http_get_with_bearer(
    client: &reqwest::Client,
    url: &str,
    bearer: Option<&str>,
) -> Result<Response, anyhow::Error> {
    let mut rb = client.get(url);
    if let Some(tok) = bearer {
        if !tok.is_empty() {
            rb = rb.bearer_auth(tok);
        }
    }
    let resp = rb.send().await?;
    let status = resp.status();
    let bytes = resp.bytes().await.unwrap_or_default();
    Ok((status, bytes).into_response())
}

pub async fn sse_proxy_stream_with_bearer(
    client: &reqwest::Client,
    url: &str,
    payload: &serde_json::Value,
    bearer: Option<&str>,
) -> Result<Response, anyhow::Error> {
    use axum::body::Body;
    use bytes::Bytes;
    use futures_util::TryStreamExt;
    use http::header;

    // Determine upstream mode from env
    // Accepts: "responses" (default), "chat", "chat-completions", "chat_completions"
    let upstream_mode = std::env::var("UPSTREAM_MODE")
        .or_else(|_| std::env::var("CHAT2RESPONSE_UPSTREAM"))
        .unwrap_or_else(|_| "responses".to_string())
        .to_lowercase();

    // Rewrite URL (only for chat mode): .../responses -> .../chat/completions
    let mut real_url = url.to_string();
    let is_chat_mode = matches!(
        upstream_mode.as_str(),
        "chat" | "chat-completions" | "chat_completions"
    );
    if is_chat_mode {
        if let Some(pos) = real_url.rfind("/responses") {
            real_url.replace_range(pos.., "/chat/completions");
        }
    }

    // Rewrite payload for chat mode:
    // - If payload has `input`, convert to Chat's `messages`
    // - Map `max_output_tokens` -> `max_tokens`
    // - Remove Responses-only fields (e.g., conversation)
    // - Ensure `stream: true` for SSE
    let mut body = payload.clone();
    if is_chat_mode {
        if let Some(obj) = body.as_object_mut() {
            if obj.get("messages").is_none() {
                if let Some(input) = obj.remove("input") {
                    match input {
                        serde_json::Value::Array(_) => {
                            obj.insert("messages".to_string(), input);
                        }
                        serde_json::Value::String(s) => {
                            obj.insert(
                                "messages".to_string(),
                                serde_json::json!([{"role":"user","content": s}]),
                            );
                        }
                        _ => {}
                    }
                }
            }
            if let Some(max_out) = obj.remove("max_output_tokens") {
                obj.insert("max_tokens".to_string(), max_out);
            }
            obj.remove("conversation");
            obj.insert("stream".to_string(), serde_json::Value::Bool(true));
        }
    }

    // Small tracing for diagnostics
    // Determine effective bearer (explicit Authorization header overrides env OPENAI_API_KEY fallback)
    let effective_bearer = bearer
        .and_then(|b| {
            let t = b.trim();
            if t.is_empty() {
                None
            } else {
                Some(t.to_string())
            }
        })
        .or_else(|| {
            std::env::var("OPENAI_API_KEY")
                .ok()
                .filter(|s| !s.is_empty())
        });

    let has_bearer = effective_bearer.is_some();
    tracing::debug!(
        upstream_mode = %upstream_mode,
        is_chat_mode = is_chat_mode,
        has_bearer = has_bearer,
        "sse_proxy_stream_with_bearer: preparing upstream request"
    );

    // Build upstream request (closure so we can retry easily)
    let build_req = || {
        let mut b = client
            .post(&real_url)
            .header(header::ACCEPT, "text/event-stream")
            .header(header::CONTENT_TYPE, "application/json")
            .header(header::CONNECTION, "close")
            .json(&body);
        if let Some(k) = &effective_bearer {
            b = b.bearer_auth(k);
        }
        b
    };

    let mut resp_opt: Option<reqwest::Response> = None;
    let mut last_err: Option<anyhow::Error> = None;
    for delay_ms in [0u64, 100, 200, 400] {
        if delay_ms > 0 {
            tokio::time::sleep(std::time::Duration::from_millis(delay_ms)).await;
        }
        match build_req().send().await {
            Ok(r) => {
                resp_opt = Some(r);
                break;
            }
            Err(e) => {
                tracing::warn!(error=%e, attempt_delay_ms=%delay_ms, "sse upstream send attempt failed");
                last_err = Some(anyhow::Error::new(e));
                continue;
            }
        }
    }
    let resp = match resp_opt {
        Some(r) => r,
        None => {
            return Err(
                last_err.unwrap_or_else(|| anyhow::anyhow!("upstream streaming request failed"))
            );
        }
    };
    let status = resp.status();
    if !status.is_success() {
        let bytes = resp.bytes().await.unwrap_or_default();
        return Ok((status, bytes).into_response());
    }

    // Stream SSE passthrough without buffering the entire response.
    let upstream_ct = resp.headers().get(header::CONTENT_TYPE).cloned();
    let stream = resp
        .bytes_stream()
        .map_err(|e| std::io::Error::other(e.to_string()))
        .map_ok(Bytes::from);

    let mut builder = http::Response::builder().status(StatusCode::OK);
    if let Some(ct) = upstream_ct {
        builder = builder.header(header::CONTENT_TYPE, ct);
    } else {
        builder = builder.header(header::CONTENT_TYPE, "text/event-stream");
    }
    let response = builder
        .header("Cache-Control", "no-cache")
        .header("Connection", "keep-alive")
        .body(Body::from_stream(stream))
        .unwrap();

    Ok(response)
}

#[allow(dead_code)]
/// Build a CORS layer from environment variables.
///
/// Environment variables:
/// - CORS_ALLOWED_ORIGINS: "*" or comma-separated origins (e.g., "https://a.com, https://b.com")
/// - CORS_ALLOWED_METHODS: "*" or comma-separated methods (e.g., "GET,POST,OPTIONS")
/// - CORS_ALLOWED_HEADERS: "*" or comma-separated request header names
/// - CORS_ALLOW_CREDENTIALS: enable with 1,true,yes,on
/// - CORS_MAX_AGE: max age in seconds (u64)
///
/// Defaults are permissive (Any) to match prior behavior when not configured.
pub fn cors_layer_from_env() -> tower_http::cors::CorsLayer {
    use std::time::Duration;

    let mut layer = tower_http::cors::CorsLayer::new();

    // Allowed origins
    if let Ok(origins) = std::env::var("CORS_ALLOWED_ORIGINS") {
        let s = origins.trim();
        if s == "*" {
            layer = layer.allow_origin(tower_http::cors::Any);
        } else {
            let mut vals = Vec::new();
            for part in s.split(',') {
                let p = part.trim();
                if p.is_empty() {
                    continue;
                }
                if let Ok(hv) = http::HeaderValue::from_str(p) {
                    vals.push(hv);
                }
            }
            if !vals.is_empty() {
                layer = layer.allow_origin(tower_http::cors::AllowOrigin::list(vals));
            } else {
                layer = layer.allow_origin(tower_http::cors::Any);
            }
        }
    } else {
        layer = layer.allow_origin(tower_http::cors::Any);
    }

    // Allowed methods
    if let Ok(methods) = std::env::var("CORS_ALLOWED_METHODS") {
        let s = methods.trim();
        if s == "*" {
            layer = layer.allow_methods(tower_http::cors::Any);
        } else {
            let mut vals = Vec::new();
            for part in s.split(',') {
                let p = part.trim().to_ascii_uppercase();
                if p.is_empty() {
                    continue;
                }
                if let Ok(m) = http::Method::from_bytes(p.as_bytes()) {
                    vals.push(m);
                }
            }
            if !vals.is_empty() {
                layer = layer.allow_methods(tower_http::cors::AllowMethods::list(vals));
            } else {
                layer = layer.allow_methods(tower_http::cors::Any);
            }
        }
    } else {
        layer = layer.allow_methods(tower_http::cors::Any);
    }

    // Allowed headers
    if let Ok(headers) = std::env::var("CORS_ALLOWED_HEADERS") {
        let s = headers.trim();
        if s == "*" {
            layer = layer.allow_headers(tower_http::cors::Any);
        } else {
            let mut vals = Vec::new();
            for part in s.split(',') {
                let p = part.trim();
                if p.is_empty() {
                    continue;
                }
                if let Ok(h) = http::header::HeaderName::try_from(p) {
                    vals.push(h);
                }
            }
            if !vals.is_empty() {
                layer = layer.allow_headers(tower_http::cors::AllowHeaders::list(vals));
            } else {
                layer = layer.allow_headers(tower_http::cors::Any);
            }
        }
    } else {
        layer = layer.allow_headers(tower_http::cors::Any);
    }

    // Credentials
    if let Ok(val) = std::env::var("CORS_ALLOW_CREDENTIALS") {
        let v = val.trim().to_ascii_lowercase();
        if v == "1" || v == "true" || v == "yes" || v == "on" {
            layer = layer.allow_credentials(true);
        }
    }

    // Max age
    if let Ok(secs) = std::env::var("CORS_MAX_AGE") {
        if let Ok(n) = secs.trim().parse::<u64>() {
            layer = layer.max_age(Duration::from_secs(n));
        }
    }

    layer
}

use axum::http::{header, HeaderMap};
use axum::{
    extract::Query,
    response::IntoResponse,
    routing::{get, post},
    Json, Router,
};
use axum::{extract::State, response::Response};
use serde::Deserialize;
use std::sync::Arc;

use crate::conversion::to_responses_request_with_mcp;
use crate::models::chat::ChatCompletionRequest;
use crate::util::AppState;

use crate::util::{
    cors_layer_from_env, error_response, openai_base_url, sse_proxy_stream_with_bearer,
};

/// Query parameters for conversion/proxy endpoints.
#[derive(Debug, Deserialize)]
pub struct ConvertQuery {
    /// Optional Responses conversation id to make the call stateful.
    pub conversation_id: Option<String>,
}

/// Passthrough for OpenAI Responses API (`/v1/responses`):
/// Accepts native Responses payload and forwards upstream without transformation.
/// Supports SSE when `stream: true`.
async fn responses_passthrough(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(body): Json<serde_json::Value>,
) -> Response {
    let base = openai_base_url();
    let url = format!("{}/responses", base);

    // Determine managed (internal upstream key) vs passthrough mode
    let env_api_key = std::env::var("OPENAI_API_KEY")
        .ok()
        .filter(|s| !s.trim().is_empty());
    let managed_mode = env_api_key.is_some();

    // Extract client bearer
    let client_bearer = headers
        .get(header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .and_then(|s| {
            let s = s.trim();
            if s.len() >= 7 && s[..6].eq_ignore_ascii_case("bearer") {
                Some(s[6..].trim().to_string())
            } else {
                None
            }
        });

    // Resolve upstream bearer
    let upstream_bearer = if managed_mode {
        if let Some(manager) = &state.api_keys {
            match client_bearer.as_deref().map(|tok| manager.verify(tok)) {
                Some(crate::auth::Verification::Valid { .. }) => env_api_key.clone(),
                Some(crate::auth::Verification::Revoked { .. }) => {
                    return error_response(axum::http::StatusCode::UNAUTHORIZED, "API key revoked");
                }
                Some(crate::auth::Verification::Expired { .. }) => {
                    return error_response(axum::http::StatusCode::UNAUTHORIZED, "API key expired");
                }
                Some(_) => {
                    return error_response(axum::http::StatusCode::UNAUTHORIZED, "Invalid API key");
                }
                None => {
                    return error_response(
                        axum::http::StatusCode::UNAUTHORIZED,
                        "Missing Authorization bearer",
                    );
                }
            }
        } else {
            env_api_key.clone()
        }
    } else {
        if client_bearer.is_none() {
            return error_response(
                axum::http::StatusCode::UNAUTHORIZED,
                "Missing Authorization bearer",
            );
        }
        client_bearer.clone()
    };

    let stream = body
        .get("stream")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);

    let client = &state.http;

    if stream {
        match sse_proxy_stream_with_bearer(client, &url, &body, upstream_bearer.as_deref()).await {
            Ok(resp) => resp,
            Err(e) => error_response(axum::http::StatusCode::BAD_GATEWAY, &e.to_string()),
        }
    } else {
        // Determine upstream mode and translate if necessary (vLLM/Ollama use Chat)
        let mode = crate::util::upstream_mode_from_env();
        let real_url = crate::util::rewrite_responses_url_for_mode(&url, mode);
        let mut effective_body = body.clone();
        if matches!(mode, crate::util::UpstreamMode::Chat) {
            let chat_req = crate::conversion::responses_json_to_chat_request(&effective_body);
            if let Ok(v) = serde_json::to_value(chat_req) {
                effective_body = v;
            }
        }

        let mut req = client
            .post(&real_url)
            .header(header::CONTENT_TYPE, "application/json");
        if let Some(b) = upstream_bearer {
            req = req.bearer_auth(b);
        }
        match req.json(&effective_body).send().await {
            Ok(up) => {
                let status = up.status();
                let bytes = up.bytes().await.unwrap_or_default();
                (status, bytes).into_response()
            }
            Err(e) => error_response(axum::http::StatusCode::BAD_GATEWAY, &e.to_string()),
        }
    }
}

/// Build the Axum router with `/convert`, `/v1/chat/completions`, `/v1/responses` (passthrough),
/// and legacy `/proxy` (deprecated; kept for backward compatibility).
pub fn build_router() -> Router {
    build_router_with_state(AppState::default())
}

/// Build the Axum router with custom AppState.
pub fn build_router_with_state(app_state: AppState) -> Router {
    let state = Arc::new(app_state);

    let router = Router::new()
        .route("/status", get(status))
        .route("/convert", post(convert))
        .route("/v1/chat/completions", post(chat_completions_passthrough))
        .route("/v1/responses", post(responses_passthrough))
        .route("/keys", get(list_keys))
        .route("/keys/generate", post(generate_key))
        .route("/keys/revoke", post(revoke_key))
        .route("/keys/set_expiration", post(set_key_expiration))
        .with_state(state.clone());

    router.layer(cors_layer_from_env())
}

/// Service status endpoint to expose feature flags and available routes.
async fn status() -> impl IntoResponse {
    let proxy_enabled: bool = true;
    let routes = vec![
        "/status",
        "/convert",
        "/v1/chat/completions",
        "/v1/responses",
        "/keys",
        "/keys/generate",
        "/keys/revoke",
        "/keys/set_expiration",
    ];
    Json(serde_json::json!({
        "name": "chat2response",
        "version": env!("CARGO_PKG_VERSION"),
        "proxy_enabled": proxy_enabled,
        "routes": routes
    }))
}

/// Convert a Chat Completions request into a Responses API request payload (JSON).
async fn convert(
    State(state): State<Arc<AppState>>,
    Query(q): Query<ConvertQuery>,
    Json(req): Json<ChatCompletionRequest>,
) -> impl IntoResponse {
    let mcp_manager = state.mcp_manager.as_ref().map(|m| m.as_ref());
    let converted = to_responses_request_with_mcp(&req, q.conversation_id, mcp_manager).await;
    Json(converted)
}

/// Proxy the converted request to OpenAI's Responses endpoint and return native output.
/// - Non-streaming: JSON roundtrip
/// - Streaming: SSE passthrough

// API key management endpoints
///
/// Direct passthrough for native Chat Completions requests (no translation).
/// Accepts the standard OpenAI Chat Completions JSON and forwards it upstream
/// to {OPENAI_BASE_URL}/chat/completions. Streaming is supported if `stream:true`.
async fn chat_completions_passthrough(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(body): Json<serde_json::Value>,
) -> Response {
    let base = openai_base_url();
    let url = format!("{}/chat/completions", base);

    // Determine managed (internal upstream key) vs passthrough mode
    let env_api_key = std::env::var("OPENAI_API_KEY")
        .ok()
        .filter(|s| !s.trim().is_empty());
    let managed_mode = env_api_key.is_some();

    // Extract client bearer (could be internal access token or upstream key)
    let client_bearer = headers
        .get(header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .and_then(|s| {
            let s = s.trim();
            if s.len() >= 7 && s[..6].eq_ignore_ascii_case("bearer") {
                Some(s[6..].trim().to_string())
            } else {
                None
            }
        });

    // Resolve upstream bearer
    let upstream_bearer = if managed_mode {
        if let Some(manager) = &state.api_keys {
            match client_bearer.as_deref().map(|tok| manager.verify(tok)) {
                Some(crate::auth::Verification::Valid { .. }) => env_api_key.clone(),
                Some(crate::auth::Verification::Revoked { .. }) => {
                    return error_response(axum::http::StatusCode::UNAUTHORIZED, "API key revoked");
                }
                Some(crate::auth::Verification::Expired { .. }) => {
                    return error_response(axum::http::StatusCode::UNAUTHORIZED, "API key expired");
                }
                Some(_) => {
                    return error_response(axum::http::StatusCode::UNAUTHORIZED, "Invalid API key");
                }
                None => {
                    return error_response(
                        axum::http::StatusCode::UNAUTHORIZED,
                        "Missing Authorization bearer",
                    );
                }
            }
        } else {
            // No manager: accept and use env key
            env_api_key.clone()
        }
    } else {
        if client_bearer.is_none() {
            return error_response(
                axum::http::StatusCode::UNAUTHORIZED,
                "Missing Authorization bearer",
            );
        }
        client_bearer.clone()
    };

    // Determine if streaming is requested
    let stream = body
        .get("stream")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);

    let client = &state.http;

    if stream {
        match sse_proxy_stream_with_bearer(client, &url, &body, upstream_bearer.as_deref()).await {
            Ok(resp) => resp,
            Err(e) => error_response(axum::http::StatusCode::BAD_GATEWAY, &e.to_string()),
        }
    } else {
        let mut req = client
            .post(&url)
            .header(header::CONTENT_TYPE, "application/json");
        if let Some(b) = upstream_bearer {
            req = req.bearer_auth(b);
        }
        match req.json(&body).send().await {
            Ok(up) => {
                let status = up.status();
                let bytes = up.bytes().await.unwrap_or_default();
                (status, bytes).into_response()
            }
            Err(e) => error_response(axum::http::StatusCode::BAD_GATEWAY, &e.to_string()),
        }
    }
}

#[derive(Debug, Deserialize)]
struct GenerateKeyRequest {
    label: Option<String>,
    ttl_seconds: Option<u64>,
    expires_at: Option<u64>,
    scopes: Option<Vec<String>>,
}

async fn generate_key(
    State(state): State<Arc<AppState>>,
    Json(payload): Json<GenerateKeyRequest>,
) -> axum::response::Response {
    // Env flag to require expiration at creation
    let require_exp = std::env::var("CHAT2RESPONSE_KEYS_REQUIRE_EXPIRATION")
        .map(|v| {
            let v = v.trim().to_ascii_lowercase();
            v == "1" || v == "true" || v == "yes" || v == "on"
        })
        .unwrap_or(false);

    // Optional default TTL (seconds) from env
    let default_ttl_secs: Option<u64> = std::env::var("CHAT2RESPONSE_KEYS_DEFAULT_TTL_SECONDS")
        .ok()
        .and_then(|s| s.trim().parse::<u64>().ok());

    // Compute effective ttl_seconds from either expires_at or ttl_seconds or default
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();

    // Determine ttl based on precedence: expires_at > ttl_seconds > env default
    let ttl_seconds = if let Some(exp) = payload.expires_at {
        if exp <= now {
            return error_response(
                axum::http::StatusCode::BAD_REQUEST,
                "expires_at must be in the future",
            );
        }
        Some(exp.saturating_sub(now))
    } else if let Some(ttl) = payload.ttl_seconds {
        Some(ttl)
    } else {
        default_ttl_secs
    };

    // If required, enforce at least some ttl
    if require_exp && ttl_seconds.is_none() {
        return error_response(
            axum::http::StatusCode::BAD_REQUEST,
            "Expiration required: provide expires_at or ttl_seconds (or configure default TTL)",
        );
    }

    match &state.api_keys {
        Some(mgr) => match mgr.generate_key(
            payload.label,
            ttl_seconds.map(std::time::Duration::from_secs),
            payload.scopes,
        ) {
            Ok(gen) => Json(gen).into_response(),
            Err(e) => error_response(
                axum::http::StatusCode::INTERNAL_SERVER_ERROR,
                &format!("failed to generate key: {}", e),
            ),
        },
        None => error_response(
            axum::http::StatusCode::SERVICE_UNAVAILABLE,
            "API key manager unavailable",
        ),
    }
}

async fn list_keys(State(state): State<Arc<AppState>>) -> axum::response::Response {
    match &state.api_keys {
        Some(mgr) => match mgr.list_keys() {
            Ok(items) => Json(items).into_response(),
            Err(e) => error_response(
                axum::http::StatusCode::INTERNAL_SERVER_ERROR,
                &format!("failed to list keys: {}", e),
            ),
        },
        None => error_response(
            axum::http::StatusCode::SERVICE_UNAVAILABLE,
            "API key manager unavailable",
        ),
    }
}

#[derive(Debug, Deserialize)]
struct RevokeKeyRequest {
    id: String,
}

async fn revoke_key(
    State(state): State<Arc<AppState>>,
    Json(payload): Json<RevokeKeyRequest>,
) -> axum::response::Response {
    match &state.api_keys {
        Some(mgr) => match mgr.revoke(&payload.id) {
            Ok(true) => {
                Json(serde_json::json!({ "revoked": true, "id": payload.id })).into_response()
            }
            Ok(false) => {
                Json(serde_json::json!({ "revoked": false, "id": payload.id })).into_response()
            }
            Err(e) => error_response(
                axum::http::StatusCode::INTERNAL_SERVER_ERROR,
                &format!("failed to revoke: {}", e),
            ),
        },
        None => error_response(
            axum::http::StatusCode::SERVICE_UNAVAILABLE,
            "API key manager unavailable",
        ),
    }
}

#[derive(Debug, Deserialize)]
struct SetExpirationRequest {
    id: String,
    expires_at: Option<u64>,
    ttl_seconds: Option<u64>,
}

async fn set_key_expiration(
    State(state): State<Arc<AppState>>,
    Json(payload): Json<SetExpirationRequest>,
) -> axum::response::Response {
    let new_exp = if let Some(at) = payload.expires_at {
        Some(at)
    } else if let Some(ttl) = payload.ttl_seconds {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        Some(now.saturating_add(ttl))
    } else {
        None
    };
    match &state.api_keys {
        Some(mgr) => match mgr.set_expiration(&payload.id, new_exp) {
            Ok(true) => Json(
                serde_json::json!({ "updated": true, "id": payload.id, "expires_at": new_exp }),
            )
            .into_response(),
            Ok(false) => {
                Json(serde_json::json!({ "updated": false, "id": payload.id })).into_response()
            }
            Err(e) => error_response(
                axum::http::StatusCode::INTERNAL_SERVER_ERROR,
                &format!("failed to set expiration: {}", e),
            ),
        },
        None => error_response(
            axum::http::StatusCode::SERVICE_UNAVAILABLE,
            "API key manager unavailable",
        ),
    }
}

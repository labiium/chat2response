use actix_web::http::header;
use actix_web::{web, HttpRequest, HttpResponse, Responder};
use serde::Deserialize;

use crate::models::chat::ChatCompletionRequest;
use crate::util::AppState;

use crate::util::{error_response, openai_base_url, sse_proxy_stream_with_bearer};

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
    state: web::Data<AppState>,
    req: HttpRequest,
    body: web::Json<serde_json::Value>,
) -> impl Responder {
    let mut body = body.into_inner();

    // Apply system prompt injection if configured
    let system_prompt_guard = state.system_prompt_config.read().await;
    let model = body.get("model").and_then(|v| v.as_str());

    if let Some(prompt) = system_prompt_guard.get_prompt(model, Some("responses")) {
        // Inject system prompt into messages
        if let Some(messages) = body.get_mut("messages").and_then(|v| v.as_array_mut()) {
            let system_msg = serde_json::json!({
                "role": "system",
                "content": prompt
            });

            match system_prompt_guard.injection_mode.as_str() {
                "append" => {
                    let last_system_pos = messages
                        .iter()
                        .rposition(|m| m.get("role").and_then(|r| r.as_str()) == Some("system"));
                    if let Some(pos) = last_system_pos {
                        messages.insert(pos + 1, system_msg);
                    } else {
                        messages.push(system_msg);
                    }
                }
                "replace" => {
                    messages.retain(|m| m.get("role").and_then(|r| r.as_str()) != Some("system"));
                    messages.insert(0, system_msg);
                }
                _ => {
                    // Default: prepend
                    messages.insert(0, system_msg);
                }
            }
        }
    }
    drop(system_prompt_guard);

    let base = openai_base_url();
    let url = format!("{}/responses", base);

    // Determine managed (internal upstream key) vs passthrough mode
    let env_api_key = std::env::var("OPENAI_API_KEY")
        .ok()
        .filter(|s| !s.trim().is_empty());
    let managed_mode = env_api_key.is_some();

    // Extract client bearer
    let client_bearer = req
        .headers()
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
                    return error_response(http::StatusCode::UNAUTHORIZED, "API key revoked");
                }
                Some(crate::auth::Verification::Expired { .. }) => {
                    return error_response(http::StatusCode::UNAUTHORIZED, "API key expired");
                }
                Some(_) => {
                    return error_response(http::StatusCode::UNAUTHORIZED, "Invalid API key");
                }
                None => {
                    return error_response(
                        http::StatusCode::UNAUTHORIZED,
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
                http::StatusCode::UNAUTHORIZED,
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
            Err(e) => error_response(http::StatusCode::BAD_GATEWAY, &e.to_string()),
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
            .header("content-type", "application/json");
        if let Some(b) = upstream_bearer {
            req = req.bearer_auth(b);
        }
        match req.json(&effective_body).send().await {
            Ok(up) => {
                let status = up.status();
                let bytes = up.bytes().await.unwrap_or_default();
                HttpResponse::build(actix_web::http::StatusCode::from_u16(status.as_u16()).unwrap())
                    .body(bytes)
            }
            Err(e) => error_response(http::StatusCode::BAD_GATEWAY, &e.to_string()),
        }
    }
}

/// Configure Actix-web routes with AppState.
pub fn config_routes(cfg: &mut web::ServiceConfig) {
    cfg.service(
        web::scope("")
            .route("/status", web::get().to(status))
            .route("/convert", web::post().to(convert))
            .route(
                "/v1/chat/completions",
                web::post().to(chat_completions_passthrough),
            )
            .route("/v1/responses", web::post().to(responses_passthrough))
            .route("/keys", web::get().to(list_keys))
            .route("/keys/generate", web::post().to(generate_key))
            .route("/keys/revoke", web::post().to(revoke_key))
            .route("/keys/set_expiration", web::post().to(set_key_expiration))
            .route("/reload/mcp", web::post().to(reload_mcp))
            .route(
                "/reload/system_prompt",
                web::post().to(reload_system_prompt),
            )
            .route("/reload/all", web::post().to(reload_all)),
    );
}

/// Service status endpoint to expose feature flags and available routes.
async fn status(state: web::Data<AppState>) -> impl Responder {
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
        "/reload/mcp",
        "/reload/system_prompt",
        "/reload/all",
    ];

    // Get current configuration status
    let mcp_enabled = state.mcp_manager.is_some();
    let mcp_config_path = state.mcp_config_path.as_deref();
    let system_prompt_config_path = state.system_prompt_config_path.as_deref();

    let system_prompt_guard = state.system_prompt_config.read().await;
    let system_prompt_enabled = system_prompt_guard.enabled;
    drop(system_prompt_guard);

    web::Json(serde_json::json!({
        "name": "chat2response",
        "version": env!("CARGO_PKG_VERSION"),
        "proxy_enabled": proxy_enabled,
        "routes": routes,
        "features": {
            "mcp": {
                "enabled": mcp_enabled,
                "config_path": mcp_config_path,
                "reloadable": mcp_config_path.is_some()
            },
            "system_prompt": {
                "enabled": system_prompt_enabled,
                "config_path": system_prompt_config_path,
                "reloadable": system_prompt_config_path.is_some()
            }
        }
    }))
}

/// Convert a Chat Completions request into a Responses API request payload (JSON).
async fn convert(
    state: web::Data<AppState>,
    query: web::Query<ConvertQuery>,
    body: web::Json<ChatCompletionRequest>,
) -> impl Responder {
    let mcp_manager_guard = if let Some(mgr) = state.mcp_manager.as_ref() {
        Some(mgr.read().await)
    } else {
        None
    };

    let system_prompt_guard = state.system_prompt_config.read().await;

    let converted = crate::conversion::to_responses_request_with_mcp_and_prompt(
        &body,
        query.conversation_id.clone(),
        mcp_manager_guard.as_deref(),
        Some(&*system_prompt_guard),
    )
    .await;

    web::Json(converted)
}

/// Direct passthrough for native Chat Completions requests (no translation).
async fn chat_completions_passthrough(
    state: web::Data<AppState>,
    req: HttpRequest,
    body: web::Json<serde_json::Value>,
) -> impl Responder {
    let mut body = body.into_inner();

    // Apply system prompt injection if configured
    let system_prompt_guard = state.system_prompt_config.read().await;
    let model = body.get("model").and_then(|v| v.as_str());

    if let Some(prompt) = system_prompt_guard.get_prompt(model, Some("chat")) {
        // Deserialize to ChatCompletionRequest for injection
        if let Ok(mut req) = serde_json::from_value::<ChatCompletionRequest>(body.clone()) {
            crate::conversion::inject_system_prompt_chat(
                &mut req,
                &prompt,
                &system_prompt_guard.injection_mode,
            );
            if let Ok(modified) = serde_json::to_value(&req) {
                body = modified;
            }
        }
    }
    drop(system_prompt_guard);

    let base = openai_base_url();
    let url = format!("{}/chat/completions", base);

    // Determine managed (internal upstream key) vs passthrough mode
    let env_api_key = std::env::var("OPENAI_API_KEY")
        .ok()
        .filter(|s| !s.trim().is_empty());
    let managed_mode = env_api_key.is_some();

    // Extract client bearer (could be internal access token or upstream key)
    let client_bearer = req
        .headers()
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
                    return error_response(http::StatusCode::UNAUTHORIZED, "API key revoked");
                }
                Some(crate::auth::Verification::Expired { .. }) => {
                    return error_response(http::StatusCode::UNAUTHORIZED, "API key expired");
                }
                Some(_) => {
                    return error_response(http::StatusCode::UNAUTHORIZED, "Invalid API key");
                }
                None => {
                    return error_response(
                        http::StatusCode::UNAUTHORIZED,
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
                http::StatusCode::UNAUTHORIZED,
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
            Err(e) => error_response(http::StatusCode::BAD_GATEWAY, &e.to_string()),
        }
    } else {
        let mut req = client.post(&url).header("content-type", "application/json");
        if let Some(b) = upstream_bearer {
            req = req.bearer_auth(b);
        }
        match req.json(&body).send().await {
            Ok(up) => {
                let status = up.status();
                let bytes = up.bytes().await.unwrap_or_default();
                HttpResponse::build(actix_web::http::StatusCode::from_u16(status.as_u16()).unwrap())
                    .body(bytes)
            }
            Err(e) => error_response(http::StatusCode::BAD_GATEWAY, &e.to_string()),
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
    state: web::Data<AppState>,
    body: web::Json<GenerateKeyRequest>,
) -> impl Responder {
    let payload = body.into_inner();

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
                http::StatusCode::BAD_REQUEST,
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
            http::StatusCode::BAD_REQUEST,
            "Expiration required: provide expires_at or ttl_seconds (or configure default TTL)",
        );
    }

    match &state.api_keys {
        Some(mgr) => match mgr.generate_key(
            payload.label,
            ttl_seconds.map(std::time::Duration::from_secs),
            payload.scopes,
        ) {
            Ok(gen) => HttpResponse::Ok().json(gen),
            Err(e) => error_response(
                http::StatusCode::INTERNAL_SERVER_ERROR,
                &format!("failed to generate key: {}", e),
            ),
        },
        None => error_response(
            http::StatusCode::SERVICE_UNAVAILABLE,
            "API key manager unavailable",
        ),
    }
}

async fn list_keys(state: web::Data<AppState>) -> impl Responder {
    match &state.api_keys {
        Some(mgr) => match mgr.list_keys() {
            Ok(items) => HttpResponse::Ok().json(items),
            Err(e) => error_response(
                http::StatusCode::INTERNAL_SERVER_ERROR,
                &format!("failed to list keys: {}", e),
            ),
        },
        None => error_response(
            http::StatusCode::SERVICE_UNAVAILABLE,
            "API key manager unavailable",
        ),
    }
}

#[derive(Debug, Deserialize)]
struct RevokeKeyRequest {
    id: String,
}

async fn revoke_key(
    state: web::Data<AppState>,
    body: web::Json<RevokeKeyRequest>,
) -> impl Responder {
    let payload = body.into_inner();

    match &state.api_keys {
        Some(mgr) => {
            match mgr.revoke(&payload.id) {
                Ok(true) => HttpResponse::Ok()
                    .json(serde_json::json!({ "revoked": true, "id": payload.id })),
                Ok(false) => HttpResponse::Ok()
                    .json(serde_json::json!({ "revoked": false, "id": payload.id })),
                Err(e) => error_response(
                    http::StatusCode::INTERNAL_SERVER_ERROR,
                    &format!("failed to revoke: {}", e),
                ),
            }
        }
        None => error_response(
            http::StatusCode::SERVICE_UNAVAILABLE,
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
    state: web::Data<AppState>,
    body: web::Json<SetExpirationRequest>,
) -> impl Responder {
    let payload = body.into_inner();

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
            Ok(true) => HttpResponse::Ok().json(
                serde_json::json!({ "updated": true, "id": payload.id, "expires_at": new_exp }),
            ),
            Ok(false) => {
                HttpResponse::Ok().json(serde_json::json!({ "updated": false, "id": payload.id }))
            }
            Err(e) => error_response(
                http::StatusCode::INTERNAL_SERVER_ERROR,
                &format!("failed to set expiration: {}", e),
            ),
        },
        None => error_response(
            http::StatusCode::SERVICE_UNAVAILABLE,
            "API key manager unavailable",
        ),
    }
}

/// Reload MCP configuration from file at runtime
async fn reload_mcp(state: web::Data<AppState>) -> impl Responder {
    let config_path = match &state.mcp_config_path {
        Some(path) => path.clone(),
        None => {
            return error_response(
                http::StatusCode::BAD_REQUEST,
                "No MCP config path configured - cannot reload",
            );
        }
    };

    tracing::info!("Reloading MCP configuration from: {}", config_path);

    // Load new config
    let config = match crate::mcp_config::McpConfig::load_from_file(&config_path) {
        Ok(c) => c,
        Err(e) => {
            tracing::error!("Failed to load MCP config: {}", e);
            return error_response(
                http::StatusCode::INTERNAL_SERVER_ERROR,
                &format!("Failed to load MCP config: {}", e),
            );
        }
    };

    // Create new MCP client manager
    let new_manager = match crate::mcp_client::McpClientManager::new(config).await {
        Ok(m) => m,
        Err(e) => {
            tracing::error!("Failed to initialize MCP client manager: {}", e);
            return error_response(
                http::StatusCode::INTERNAL_SERVER_ERROR,
                &format!("Failed to initialize MCP manager: {}", e),
            );
        }
    };

    // Get connected servers for response
    let connected_servers = new_manager.connected_servers();
    let server_count = connected_servers.len();

    // Replace the manager
    if let Some(manager_arc) = &state.mcp_manager {
        let mut manager_guard = manager_arc.write().await;
        *manager_guard = new_manager;
        tracing::info!(
            "MCP configuration reloaded successfully with {} servers",
            server_count
        );

        HttpResponse::Ok().json(serde_json::json!({
            "success": true,
            "message": "MCP configuration reloaded",
            "servers": connected_servers,
            "count": server_count
        }))
    } else {
        error_response(
            http::StatusCode::SERVICE_UNAVAILABLE,
            "MCP manager not initialized",
        )
    }
}

/// Reload system prompt configuration from file at runtime
async fn reload_system_prompt(state: web::Data<AppState>) -> impl Responder {
    let config_path = match &state.system_prompt_config_path {
        Some(path) => path.clone(),
        None => {
            return error_response(
                http::StatusCode::BAD_REQUEST,
                "No system prompt config path configured - cannot reload",
            );
        }
    };

    tracing::info!(
        "Reloading system prompt configuration from: {}",
        config_path
    );

    // Load new config
    let config = match crate::system_prompt_config::SystemPromptConfig::load_from_file(&config_path)
    {
        Ok(c) => c,
        Err(e) => {
            tracing::error!("Failed to load system prompt config: {}", e);
            return error_response(
                http::StatusCode::INTERNAL_SERVER_ERROR,
                &format!("Failed to load system prompt config: {}", e),
            );
        }
    };

    // Replace the config
    let mut config_guard = state.system_prompt_config.write().await;
    *config_guard = config.clone();

    tracing::info!("System prompt configuration reloaded successfully");

    HttpResponse::Ok().json(serde_json::json!({
        "success": true,
        "message": "System prompt configuration reloaded",
        "enabled": config.enabled,
        "has_global": config.global.is_some(),
        "per_model_count": config.per_model.len(),
        "per_api_count": config.per_api.len(),
        "injection_mode": config.injection_mode
    }))
}

/// Reload both MCP and system prompt configurations
async fn reload_all(state: web::Data<AppState>) -> impl Responder {
    let mut results = serde_json::json!({
        "mcp": { "success": false, "message": "Not attempted" },
        "system_prompt": { "success": false, "message": "Not attempted" }
    });

    // Reload MCP if path is configured
    if let Some(mcp_path) = &state.mcp_config_path {
        tracing::info!("Reloading MCP configuration from: {}", mcp_path);

        match crate::mcp_config::McpConfig::load_from_file(mcp_path) {
            Ok(config) => match crate::mcp_client::McpClientManager::new(config).await {
                Ok(new_manager) => {
                    let connected_servers = new_manager.connected_servers();
                    let server_count = connected_servers.len();

                    if let Some(manager_arc) = &state.mcp_manager {
                        let mut manager_guard = manager_arc.write().await;
                        *manager_guard = new_manager;

                        results["mcp"] = serde_json::json!({
                            "success": true,
                            "message": "MCP configuration reloaded",
                            "servers": connected_servers,
                            "count": server_count
                        });

                        tracing::info!("MCP configuration reloaded successfully");
                    } else {
                        results["mcp"] = serde_json::json!({
                            "success": false,
                            "message": "MCP manager not initialized"
                        });
                    }
                }
                Err(e) => {
                    tracing::error!("Failed to initialize MCP client manager: {}", e);
                    results["mcp"] = serde_json::json!({
                        "success": false,
                        "message": format!("Failed to initialize MCP manager: {}", e)
                    });
                }
            },
            Err(e) => {
                tracing::error!("Failed to load MCP config: {}", e);
                results["mcp"] = serde_json::json!({
                    "success": false,
                    "message": format!("Failed to load MCP config: {}", e)
                });
            }
        }
    } else {
        results["mcp"] = serde_json::json!({
            "success": false,
            "message": "No MCP config path configured"
        });
    }

    // Reload system prompt if path is configured
    if let Some(prompt_path) = &state.system_prompt_config_path {
        tracing::info!(
            "Reloading system prompt configuration from: {}",
            prompt_path
        );

        match crate::system_prompt_config::SystemPromptConfig::load_from_file(prompt_path) {
            Ok(config) => {
                let mut config_guard = state.system_prompt_config.write().await;
                *config_guard = config.clone();

                results["system_prompt"] = serde_json::json!({
                    "success": true,
                    "message": "System prompt configuration reloaded",
                    "enabled": config.enabled,
                    "has_global": config.global.is_some(),
                    "per_model_count": config.per_model.len(),
                    "per_api_count": config.per_api.len(),
                    "injection_mode": config.injection_mode
                });

                tracing::info!("System prompt configuration reloaded successfully");
            }
            Err(e) => {
                tracing::error!("Failed to load system prompt config: {}", e);
                results["system_prompt"] = serde_json::json!({
                    "success": false,
                    "message": format!("Failed to load system prompt config: {}", e)
                });
            }
        }
    } else {
        results["system_prompt"] = serde_json::json!({
            "success": false,
            "message": "No system prompt config path configured"
        });
    }

    HttpResponse::Ok().json(results)
}

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
use crate::models::chat::{ChatCompletionRequest, ChatMessage, Role};
use crate::util::AppState;

use crate::util::{
    cors_layer_from_env, error_response, openai_base_url, post_responses_with_input_retry,
    sse_proxy_stream,
};

/// Query parameters for conversion/proxy endpoints.
#[derive(Debug, Deserialize)]
pub struct ConvertQuery {
    /// Optional Responses conversation id to make the call stateful.
    pub conversation_id: Option<String>,
}

/// Build the Axum router with `/convert` and `/proxy`.
pub fn build_router() -> Router {
    build_router_with_state(AppState::default())
}

/// Build the Axum router with custom AppState.
pub fn build_router_with_state(app_state: AppState) -> Router {
    let state = Arc::new(app_state);

    let router = Router::new()
        .route("/status", get(status))
        .route("/convert", post(convert))
        .with_state(state.clone());

    let router = router.route("/proxy", post(proxy)).with_state(state);

    router.layer(cors_layer_from_env())
}

/// Service status endpoint to expose feature flags and available routes.
async fn status() -> impl IntoResponse {
    let proxy_enabled: bool = true;
    let routes = vec!["/status", "/convert", "/proxy"];
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
async fn proxy(
    State(state): State<Arc<AppState>>,
    Query(q): Query<ConvertQuery>,
    Json(mut req): Json<ChatCompletionRequest>,
) -> Response {
    // Handle MCP tool calls if present
    if let Err(e) = handle_mcp_tool_calls(&mut req, &state).await {
        return error_response(
            axum::http::StatusCode::INTERNAL_SERVER_ERROR,
            &format!("MCP tool call failed: {}", e),
        );
    }

    let mcp_manager = state.mcp_manager.as_ref().map(|m| m.as_ref());
    let converted = to_responses_request_with_mcp(&req, q.conversation_id, mcp_manager).await;
    let stream = converted.stream.unwrap_or(false);

    // Always target Responses upstream
    let base = openai_base_url();
    let url = format!("{base}/responses");
    let client = &state.http;

    if stream {
        // Streaming SSE passthrough (payload as JSON Value)
        let mut payload = match serde_json::to_value(&converted) {
            Ok(v) => v,
            Err(e) => {
                return error_response(
                    axum::http::StatusCode::INTERNAL_SERVER_ERROR,
                    &format!("serialize error: {e}"),
                )
            }
        };
        normalize_message_contents(&mut payload);
        maybe_add_input(&mut payload);

        match sse_proxy_stream(client, &url, &payload).await {
            Ok(resp) => resp,
            Err(e) => error_response(axum::http::StatusCode::BAD_GATEWAY, &e.to_string()),
        }
    } else {
        // Non-streaming JSON roundtrip
        let mut payload = match serde_json::to_value(&converted) {
            Ok(v) => v,
            Err(e) => {
                return error_response(
                    axum::http::StatusCode::INTERNAL_SERVER_ERROR,
                    &format!("serialize error: {e}"),
                )
            }
        };
        normalize_message_contents(&mut payload);
        maybe_add_input(&mut payload);

        match post_responses_with_input_retry(client, &url, &payload, Some(state.api_key())).await {
            Ok(resp) => resp,
            Err(e) => error_response(axum::http::StatusCode::BAD_GATEWAY, &e.to_string()),
        }
    }
}

/// Handle MCP tool calls in the request by executing them and adding the results as messages
async fn handle_mcp_tool_calls(
    req: &mut ChatCompletionRequest,
    state: &AppState,
) -> Result<(), anyhow::Error> {
    use crate::mcp_client::McpTool;
    use serde_json::Value;

    let mcp_manager = match &state.mcp_manager {
        Some(manager) => manager,
        None => return Ok(()), // No MCP manager, nothing to do
    };

    // Look for assistant messages with tool calls that might be MCP tools
    let mut tool_results = Vec::new();

    for message in &req.messages {
        if message.role == Role::Assistant {
            if let Value::Object(content_obj) = &message.content {
                if let Some(Value::Array(calls)) = content_obj.get("tool_calls") {
                    for call in calls {
                        if let Value::Object(call_obj) = call {
                            if let (Some(Value::String(call_id)), Some(Value::Object(function))) =
                                (call_obj.get("id"), call_obj.get("function"))
                            {
                                if let (Some(Value::String(name)), Some(arguments)) =
                                    (function.get("name"), function.get("arguments"))
                                {
                                    // Check if this is an MCP tool (has server_tool format)
                                    if let Some((server_name, tool_name)) =
                                        McpTool::parse_combined_name(name)
                                    {
                                        match mcp_manager
                                            .call_tool(&server_name, &tool_name, arguments.clone())
                                            .await
                                        {
                                            Ok(result) => {
                                                tool_results.push(ChatMessage {
                                                    role: Role::Tool,
                                                    content: result,
                                                    name: Some(name.clone()),
                                                    tool_call_id: Some(call_id.clone()),
                                                });
                                            }
                                            Err(e) => {
                                                tracing::warn!(
                                                    "MCP tool call failed: {} - {}",
                                                    name,
                                                    e
                                                );
                                                tool_results.push(ChatMessage {
                                                    role: Role::Tool,
                                                    content: Value::String(format!("Error: {}", e)),
                                                    name: Some(name.clone()),
                                                    tool_call_id: Some(call_id.clone()),
                                                });
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    // Add tool results to the messages
    req.messages.extend(tool_results);

    Ok(())
}

/// Derive and inject an 'input' field for upstreams that expect a single-string input.
/// Enabled when CHAT2RESPONSE_UPSTREAM_INPUT is set to a truthy value: 1,true,yes,on
fn maybe_add_input(v: &mut serde_json::Value) {
    // Gate behind CHAT2RESPONSE_UPSTREAM_INPUT env (truthy: 1,true,yes,on)
    let enabled = std::env::var("CHAT2RESPONSE_UPSTREAM_INPUT")
        .map(|v| {
            let v = v.trim().to_ascii_lowercase();
            v == "1" || v == "true" || v == "yes" || v == "on"
        })
        .unwrap_or(false);
    if !enabled {
        return;
    }

    let obj = match v.as_object_mut() {
        Some(o) => o,
        None => return,
    };

    // Do not overwrite if already present
    if obj.contains_key("input") {
        return;
    }

    // Try to derive a reasonable input string from messages
    let mut derived: Option<String> = None;

    if let Some(msgs) = obj.get("messages").and_then(|m| m.as_array()) {
        // Prefer the last 'user' message; else fall back to the last message
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
                    serde_json::Value::String(s) => {
                        derived = Some(s.clone());
                    }
                    serde_json::Value::Array(parts) => {
                        // Collect any "text" or "input_text" fragments
                        let mut pieces: Vec<String> = Vec::new();
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

    // Fallback empty string if nothing could be derived; upstream may still accept it
    let input_val = serde_json::Value::String(derived.unwrap_or_default());
    obj.insert("input".to_string(), input_val);
}

/// Normalize message content: if a message's "content" is a string,
/// convert it into an array with a single text part:
/// { "type": "text", "text": "<the string>" }.
/// Leaves arrays as-is.
fn normalize_message_contents(v: &mut serde_json::Value) {
    let obj = match v.as_object_mut() {
        Some(o) => o,
        None => return,
    };
    let messages = match obj.get_mut("messages") {
        Some(m) => m,
        None => return,
    };
    let arr = match messages.as_array_mut() {
        Some(a) => a,
        None => return,
    };
    for m in arr.iter_mut() {
        if let Some(mo) = m.as_object_mut() {
            if let Some(content) = mo.get_mut("content") {
                match content {
                    serde_json::Value::String(s) => {
                        let new_val = serde_json::json!([{"type":"text","text": s.clone()}]);
                        *content = new_val;
                    }
                    serde_json::Value::Array(_parts) => {
                        // Leave as-is; upstream can handle content parts already.
                    }
                    _ => {
                        // Unsupported content shape; leave as-is.
                    }
                }
            }
        }
    }
}

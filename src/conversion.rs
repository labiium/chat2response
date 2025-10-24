use crate::models::chat;
use crate::models::responses as resp;
use serde_json::{Map, Value};

pub fn responses_json_to_chat_request(v: &serde_json::Value) -> chat::ChatCompletionRequest {
    let model = v
        .get("model")
        .and_then(|s| s.as_str())
        .unwrap_or_default()
        .to_string();

    // Prefer top-level "messages"; fall back to "input.messages"
    let messages_val = v
        .get("messages")
        .cloned()
        .or_else(|| v.get("input").and_then(|i| i.get("messages")).cloned())
        .unwrap_or_else(|| serde_json::Value::Array(vec![]));

    let mut messages: Vec<chat::ChatMessage> = Vec::new();
    if let serde_json::Value::Array(arr) = messages_val {
        for m in arr {
            let role_str = m.get("role").and_then(|r| r.as_str()).unwrap_or("user");
            let role = match role_str {
                "system" => chat::Role::System,
                "user" => chat::Role::User,
                "assistant" => chat::Role::Assistant,
                "tool" => chat::Role::Tool,
                "function" => chat::Role::Function,
                _ => chat::Role::User,
            };
            let content = m.get("content").cloned().unwrap_or(serde_json::Value::Null);
            let name = m
                .get("name")
                .and_then(|n| n.as_str())
                .map(|s| s.to_string());
            let tool_call_id = m
                .get("tool_call_id")
                .and_then(|n| n.as_str())
                .map(|s| s.to_string());
            messages.push(chat::ChatMessage {
                role,
                content,
                name,
                tool_call_id,
            });
        }
    }

    // Decoding/sampling
    let temperature = v.get("temperature").and_then(|x| x.as_f64());
    let top_p = v.get("top_p").and_then(|x| x.as_f64());
    let max_tokens = v
        .get("max_output_tokens")
        .and_then(|x| x.as_u64())
        .map(|n| n as u32);
    let stop = v.get("stop").cloned();
    let presence_penalty = v.get("presence_penalty").and_then(|x| x.as_f64());
    let frequency_penalty = v.get("frequency_penalty").and_then(|x| x.as_f64());
    let logit_bias = v
        .get("logit_bias")
        .and_then(|lb| lb.as_object())
        .map(|obj| {
            let mut map = std::collections::HashMap::<String, f64>::new();
            for (k, val) in obj {
                if let Some(f) = val.as_f64() {
                    map.insert(k.clone(), f);
                }
            }
            map
        });
    let user = v
        .get("user")
        .and_then(|x| x.as_str())
        .map(|s| s.to_string());
    let n = v.get("n").and_then(|x| x.as_u64()).map(|u| u as u32);

    // Tools
    let tools = v.get("tools").and_then(|t| t.as_array()).map(|arr| {
        arr.iter()
            .filter_map(|tdef| {
                let ttype = tdef.get("type").and_then(|s| s.as_str()).unwrap_or("");
                if ttype == "function" {
                    if let Some(fun) = tdef.get("function") {
                        let name = fun
                            .get("name")
                            .and_then(|s| s.as_str())
                            .map(|s| s.to_string())?;
                        let description = fun
                            .get("description")
                            .and_then(|s| s.as_str())
                            .map(|s| s.to_string());
                        let parameters = fun
                            .get("parameters")
                            .cloned()
                            .unwrap_or(serde_json::json!({}));
                        Some(chat::ToolDefinition::Function {
                            function: chat::FunctionDef {
                                name,
                                description,
                                parameters,
                            },
                        })
                    } else {
                        None
                    }
                } else {
                    None
                }
            })
            .collect::<Vec<_>>()
    });

    let tool_choice = v.get("tool_choice").cloned();

    // Response format
    let response_format = v
        .get("response_format")
        .and_then(|rf| rf.as_object())
        .map(|obj| {
            let kind = obj
                .get("type")
                .and_then(|s| s.as_str())
                .unwrap_or("")
                .to_string();
            let mut extra = std::collections::HashMap::new();
            for (k, val) in obj {
                if k != "type" {
                    extra.insert(k.clone(), val.clone());
                }
            }
            chat::ResponseFormat { kind, extra }
        });

    // Streaming flag
    let stream = v.get("stream").and_then(|x| x.as_bool());

    chat::ChatCompletionRequest {
        model,
        messages,
        temperature,
        top_p,
        max_tokens,
        stop,
        presence_penalty,
        frequency_penalty,
        logit_bias,
        user,
        n,
        tools,
        tool_choice,
        response_format,
        stream,
    }
}

#[cfg(test)]
mod tests_responses_to_chat {
    use super::*;
    use serde_json::json;

    #[test]
    fn converts_responses_to_chat_basic() {
        let v = json!({
            "model": "gpt-4o-mini",
            "messages": [
                {"role":"system","content":"You are helpful."},
                {"role":"user","content":"Hi"},
                {"role":"assistant","content":"Hello"}
            ],
            "max_output_tokens": 123,
            "tools": [{
                "type":"function",
                "function": {
                    "name":"lookup",
                    "description":"Lookup a value",
                    "parameters":{"type":"object","properties":{"q":{"type":"string"}},"required":["q"]}
                }
            }],
            "tool_choice": {"type":"function","function":{"name":"lookup"}},
            "response_format": {"type":"json_object","schema":{"type":"object"}},
            "stream": false
        });
        let out = responses_json_to_chat_request(&v);
        assert_eq!(out.model, "gpt-4o-mini");
        assert_eq!(out.messages.len(), 3);
        assert_eq!(out.max_tokens, Some(123));
        assert!(out.tools.as_ref().unwrap().len() == 1);
        assert!(out.tool_choice.is_some());
        assert!(out.response_format.is_some());
        assert_eq!(out.stream, Some(false));
    }

    #[test]
    fn falls_back_to_input_messages() {
        let v = json!({
            "model": "gpt-4o-mini",
            "input": {
                "messages": [
                    {"role":"user","content":"From input.messages"}
                ]
            }
        });
        let out = responses_json_to_chat_request(&v);
        assert_eq!(out.messages.len(), 1);
        assert_eq!(super::role_to_string(&out.messages[0].role), "user");
    }
}

/// Convert an OpenAI Chat Completions request into a Responses API request (wrapping chat messages under `input.messages`).
///
/// Mapping highlights:
/// - messages: forwarded 1:1 under `input.messages` (Responses chat-form).
///   Role mapping: system|user|assistant|tool; legacy "function" is mapped to "tool".
/// - max_tokens -> max_output_tokens (Responses naming).
/// - tools (function) and tool_choice: forwarded preserving JSON schema.
/// - response_format: forwarded as an object; `{ "type": <kind>, ...extras }`.
/// - stream: forwarded verbatim (used by the proxy to request SSE).
/// - conversation: optional Responses-side conversation identifier for stateful calls.
pub fn to_responses_request(
    src: &chat::ChatCompletionRequest,
    conversation: Option<String>,
) -> resp::ResponsesRequest {
    let messages = map_messages(&src.messages);

    let tools = src
        .tools
        .as_ref()
        .map(|ts| ts.iter().map(map_tool).collect::<Vec<_>>());

    let response_format = src.response_format.as_ref().map(map_response_format);

    resp::ResponsesRequest {
        model: src.model.clone(),
        messages,
        // Sampling / decoding
        temperature: src.temperature,
        top_p: src.top_p,
        max_output_tokens: src.max_tokens,
        stop: src.stop.clone(),
        presence_penalty: src.presence_penalty,
        frequency_penalty: src.frequency_penalty,
        logit_bias: src.logit_bias.clone(),
        user: src.user.clone(),
        n: src.n,
        // Tools
        tools,
        tool_choice: src.tool_choice.clone(),
        // Output shaping
        response_format,
        // Streaming
        stream: src.stream,
        // Stateful conversation id (optional)
        conversation,
    }
}

/// Convert an OpenAI Chat Completions request into a Responses API request with MCP tools merged in.
pub async fn to_responses_request_with_mcp(
    src: &chat::ChatCompletionRequest,
    conversation: Option<String>,
    mcp_manager: Option<&crate::mcp_client::McpClientManager>,
) -> resp::ResponsesRequest {
    let mut request = to_responses_request(src, conversation);

    // Add MCP tools if manager is available
    if let Some(manager) = mcp_manager {
        if let Ok(mcp_tools) = manager.list_all_tools().await {
            let mcp_tool_definitions: Vec<resp::ResponsesToolDefinition> = mcp_tools
                .iter()
                .map(|tool| {
                    // Convert MCP tool to Responses tool definition
                    resp::ResponsesToolDefinition::Function {
                        function: resp::ResponsesToolFunction {
                            name: format!("{}_{}", tool.server_name, tool.name),
                            description: tool.description.clone(),
                            parameters: tool.input_schema.clone(),
                        },
                    }
                })
                .collect();

            // Merge with existing tools
            let mut all_tools = request.tools.unwrap_or_default();
            all_tools.extend(mcp_tool_definitions);
            request.tools = if all_tools.is_empty() {
                None
            } else {
                Some(all_tools)
            };
        }
    }

    request
}

/// Convert an OpenAI Chat Completions request with MCP tools and system prompt injection
pub async fn to_responses_request_with_mcp_and_prompt(
    src: &chat::ChatCompletionRequest,
    conversation: Option<String>,
    mcp_manager: Option<&crate::mcp_client::McpClientManager>,
    system_prompt_config: Option<&crate::system_prompt_config::SystemPromptConfig>,
) -> resp::ResponsesRequest {
    let mut request = to_responses_request(src, conversation);

    // Add MCP tools if manager is available
    if let Some(manager) = mcp_manager {
        if let Ok(mcp_tools) = manager.list_all_tools().await {
            let mcp_tool_definitions: Vec<resp::ResponsesToolDefinition> = mcp_tools
                .iter()
                .map(|tool| {
                    // Convert MCP tool to Responses tool definition
                    resp::ResponsesToolDefinition::Function {
                        function: resp::ResponsesToolFunction {
                            name: format!("{}_{}", tool.server_name, tool.name),
                            description: tool.description.clone(),
                            parameters: tool.input_schema.clone(),
                        },
                    }
                })
                .collect();

            // Merge with existing tools
            let mut all_tools = request.tools.unwrap_or_default();
            all_tools.extend(mcp_tool_definitions);
            request.tools = if all_tools.is_empty() {
                None
            } else {
                Some(all_tools)
            };
        }
    }

    // Inject system prompt if configured
    if let Some(config) = system_prompt_config {
        if let Some(prompt) = config.get_prompt(Some(&request.model), Some("responses")) {
            inject_system_prompt(&mut request.messages, &prompt, &config.injection_mode);
        }
    }

    request
}

/// Inject system prompt into Chat Completions request
pub fn inject_system_prompt_chat(req: &mut chat::ChatCompletionRequest, prompt: &str, mode: &str) {
    let system_message = chat::ChatMessage {
        role: chat::Role::System,
        content: serde_json::Value::String(prompt.to_string()),
        name: None,
        tool_call_id: None,
    };

    match mode {
        "append" => {
            // Find last system message position or append at end
            let last_system_pos = req
                .messages
                .iter()
                .rposition(|m| matches!(m.role, chat::Role::System));

            if let Some(pos) = last_system_pos {
                req.messages.insert(pos + 1, system_message);
            } else {
                req.messages.push(system_message);
            }
        }
        "replace" => {
            // Remove all existing system messages and prepend new one
            req.messages
                .retain(|m| !matches!(m.role, chat::Role::System));
            req.messages.insert(0, system_message);
        }
        _ => {
            // Default: prepend
            req.messages.insert(0, system_message);
        }
    }
}

/// Inject system prompt into Responses messages
pub fn inject_system_prompt(messages: &mut Vec<resp::ResponsesMessage>, prompt: &str, mode: &str) {
    let system_message = resp::ResponsesMessage {
        role: "system".to_string(),
        content: serde_json::Value::String(prompt.to_string()),
        name: None,
        tool_call_id: None,
    };

    match mode {
        "append" => {
            // Find last system message position or append at end
            let last_system_pos = messages.iter().rposition(|m| m.role == "system");

            if let Some(pos) = last_system_pos {
                messages.insert(pos + 1, system_message);
            } else {
                messages.push(system_message);
            }
        }
        "replace" => {
            // Remove all existing system messages and prepend new one
            messages.retain(|m| m.role != "system");
            messages.insert(0, system_message);
        }
        _ => {
            // Default: prepend
            messages.insert(0, system_message);
        }
    }
}

fn map_messages(src: &[chat::ChatMessage]) -> Vec<resp::ResponsesMessage> {
    src.iter()
        .map(|m| resp::ResponsesMessage {
            role: role_to_string(&m.role).to_string(),
            content: m.content.clone(),
            name: m.name.clone(),
            tool_call_id: m.tool_call_id.clone(),
        })
        .collect()
}

fn role_to_string(role: &chat::Role) -> &'static str {
    match role {
        chat::Role::System => "system",
        chat::Role::User => "user",
        chat::Role::Assistant => "assistant",
        chat::Role::Tool => "tool",
        // Legacy Chat role; Responses expects "tool" for tool outputs.
        chat::Role::Function => "tool",
    }
}

fn map_tool(t: &chat::ToolDefinition) -> resp::ResponsesToolDefinition {
    match t {
        chat::ToolDefinition::Function { function } => resp::ResponsesToolDefinition::Function {
            function: resp::ResponsesToolFunction {
                name: function.name.clone(),
                description: function.description.clone(),
                parameters: function.parameters.clone(),
            },
        },
    }
}

fn map_response_format(rf: &chat::ResponseFormat) -> Value {
    // Build an object: { "type": rf.kind, ...rf.extra } with "type" from kind
    let mut obj = Map::<String, Value>::new();
    obj.insert("type".to_string(), Value::String(rf.kind.clone()));
    for (k, v) in rf.extra.iter() {
        // Guard against accidental override of "type" inside extras.
        if k != "type" {
            obj.insert(k.clone(), v.clone());
        }
    }
    Value::Object(obj)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::chat::{
        ChatCompletionRequest, ChatMessage, FunctionDef, ResponseFormat, Role, ToolDefinition,
    };
    use serde_json::json;
    use std::collections::HashMap;

    #[test]
    fn maps_basic_fields() {
        let req = ChatCompletionRequest {
            model: "gpt-4o-mini".into(),
            messages: vec![
                ChatMessage {
                    role: Role::System,
                    content: json!("You are helpful."),
                    name: None,
                    tool_call_id: None,
                },
                ChatMessage {
                    role: Role::User,
                    content: json!("Hello"),
                    name: None,
                    tool_call_id: None,
                },
            ],
            temperature: Some(0.3),
            top_p: Some(0.95),
            max_tokens: Some(128),
            stop: None,
            presence_penalty: Some(0.0),
            frequency_penalty: Some(0.0),
            logit_bias: None,
            user: Some("unit".into()),
            n: Some(1),
            tools: None,
            tool_choice: None,
            response_format: None,
            stream: Some(false),
        };

        let out = to_responses_request(&req, Some("conv-xyz".into()));
        assert_eq!(out.model, "gpt-4o-mini");
        assert_eq!(out.messages.len(), 2);
        assert_eq!(out.messages[0].role, "system");
        assert_eq!(out.messages[1].role, "user");
        assert_eq!(out.max_output_tokens, Some(128));
        assert_eq!(out.temperature, Some(0.3));
        assert_eq!(out.top_p, Some(0.95));
        assert_eq!(out.conversation.as_deref(), Some("conv-xyz"));
        assert_eq!(out.stream, Some(false));
    }

    #[test]
    fn maps_tools_and_response_format() {
        let mut extra = HashMap::new();
        extra.insert(
            "schema".into(),
            json!({"type":"object","properties":{"x":{"type":"string"}}}),
        );

        let req = ChatCompletionRequest {
            model: "gpt-4o-mini".into(),
            messages: vec![ChatMessage {
                role: Role::User,
                content: json!("Call a tool"),
                name: None,
                tool_call_id: None,
            }],
            temperature: None,
            top_p: None,
            max_tokens: None,
            stop: None,
            presence_penalty: None,
            frequency_penalty: None,
            logit_bias: None,
            user: None,
            n: None,
            tools: Some(vec![ToolDefinition::Function {
                function: FunctionDef {
                    name: "lookup".into(),
                    description: Some("Lookup a value".into()),
                    parameters: json!({
                        "type": "object",
                        "properties": { "key": { "type": "string" } },
                        "required": ["key"]
                    }),
                },
            }]),
            tool_choice: Some(json!({"type":"function","function":{"name":"lookup"}})),
            response_format: Some(ResponseFormat {
                kind: "json_object".into(),
                extra,
            }),
            stream: Some(true),
        };

        let out = to_responses_request(&req, None);
        assert!(out.tools.is_some());
        let tools = out.tools.unwrap();
        assert_eq!(tools.len(), 1);
        #[allow(irrefutable_let_patterns)]
        if let resp::ResponsesToolDefinition::Function { function } = &tools[0] {
            assert_eq!(function.name, "lookup");
            assert!(function.description.as_deref() == Some("Lookup a value"));
        } else {
            panic!("expected function tool");
        }

        let rf = out.response_format.expect("response_format missing");
        assert_eq!(rf.get("type").and_then(|v| v.as_str()), Some("json_object"));
        assert!(rf.get("schema").is_some());
        assert_eq!(out.stream, Some(true));
    }

    #[test]
    fn maps_function_role_to_tool() {
        let req = ChatCompletionRequest {
            model: "gpt-4o-mini".into(),
            messages: vec![ChatMessage {
                role: Role::Function,
                content: json!("result"),
                name: Some("fn".into()),
                tool_call_id: Some("t1".into()),
            }],
            temperature: None,
            top_p: None,
            max_tokens: None,
            stop: None,
            presence_penalty: None,
            frequency_penalty: None,
            logit_bias: None,
            user: None,
            n: None,
            tools: None,
            tool_choice: None,
            response_format: None,
            stream: None,
        };

        let out = to_responses_request(&req, None);
        assert_eq!(out.messages.len(), 1);
        assert_eq!(out.messages[0].role, "tool");
        assert_eq!(out.messages[0].name.as_deref(), Some("fn"));
        assert_eq!(out.messages[0].tool_call_id.as_deref(), Some("t1"));
    }
}

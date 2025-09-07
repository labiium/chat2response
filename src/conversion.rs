use crate::models::chat;
use crate::models::responses as resp;
use serde_json::{Map, Value};

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

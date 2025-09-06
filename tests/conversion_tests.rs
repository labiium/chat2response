use chat2response::models::chat::{
    ChatCompletionRequest, ChatMessage, FunctionDef, ResponseFormat, Role, ToolDefinition,
};
use chat2response::to_responses_request;
use serde_json::json;
use std::collections::HashMap;

#[test]
fn basic_role_and_message_mapping() {
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
            ChatMessage {
                role: Role::Assistant,
                content: json!("Hi!"),
                name: None,
                tool_call_id: None,
            },
            ChatMessage {
                role: Role::Tool,
                content: json!({"result": "ok"}),
                name: Some("my_tool".into()),
                tool_call_id: Some("call-1".into()),
            },
            // Legacy/alias role expected to be mapped to "tool" in Responses
            ChatMessage {
                role: Role::Function,
                content: json!("fn output"),
                name: Some("legacy_fn".into()),
                tool_call_id: Some("call-2".into()),
            },
        ],
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
    assert_eq!(out.model, "gpt-4o-mini");
    assert_eq!(out.messages.len(), 5);

    // Role mapping checks
    assert_eq!(out.messages[0].role, "system");
    assert_eq!(out.messages[1].role, "user");
    assert_eq!(out.messages[2].role, "assistant");
    assert_eq!(out.messages[3].role, "tool");
    assert_eq!(out.messages[4].role, "tool"); // function -> tool

    // Name/tool_call_id propagation
    assert_eq!(out.messages[3].name.as_deref(), Some("my_tool"));
    assert_eq!(out.messages[3].tool_call_id.as_deref(), Some("call-1"));
    assert_eq!(out.messages[4].name.as_deref(), Some("legacy_fn"));
    assert_eq!(out.messages[4].tool_call_id.as_deref(), Some("call-2"));
}

#[test]
fn sampling_limits_and_stopping_map_correctly() {
    let mut bias = HashMap::new();
    bias.insert("50256".into(), -100.0);

    let req = ChatCompletionRequest {
        model: "gpt-4o-mini".into(),
        messages: vec![ChatMessage {
            role: Role::User,
            content: json!("Say hi"),
            name: None,
            tool_call_id: None,
        }],
        temperature: Some(0.7),
        top_p: Some(0.9),
        max_tokens: Some(55),
        // Single stop as string
        stop: Some(json!("\n")),
        presence_penalty: Some(0.1),
        frequency_penalty: Some(0.2),
        logit_bias: Some(bias.clone()),
        user: Some("tester".into()),
        n: Some(2),
        tools: None,
        tool_choice: None,
        response_format: None,
        stream: Some(false),
    };

    let out = to_responses_request(&req, Some("conv-abc".into()));

    assert_eq!(out.temperature, Some(0.7));
    assert_eq!(out.top_p, Some(0.9));
    assert_eq!(out.max_output_tokens, Some(55));
    assert_eq!(out.stop, Some(json!("\n")));
    assert_eq!(out.presence_penalty, Some(0.1));
    assert_eq!(out.frequency_penalty, Some(0.2));
    assert_eq!(out.logit_bias, Some(bias));
    assert_eq!(out.user.as_deref(), Some("tester"));
    assert_eq!(out.n, Some(2));
    assert_eq!(out.stream, Some(false));
    assert_eq!(out.conversation.as_deref(), Some("conv-abc"));
}

#[test]
fn stop_array_supported_and_preserved() {
    let req = ChatCompletionRequest {
        model: "gpt-4o-mini".into(),
        messages: vec![ChatMessage {
            role: Role::User,
            content: json!("Give list"),
            name: None,
            tool_call_id: None,
        }],
        temperature: None,
        top_p: None,
        max_tokens: Some(10),
        // Stop as array
        stop: Some(json!(["END", "---"])),
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
    assert_eq!(out.max_output_tokens, Some(10));
    assert_eq!(out.stop, Some(json!(["END", "---"])));
}

#[test]
fn tools_and_tool_choice_are_forwarded() {
    let req = ChatCompletionRequest {
        model: "gpt-4o-mini".into(),
        messages: vec![ChatMessage {
            role: Role::User,
            content: json!("Use the tool please"),
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
                    "type":"object",
                    "properties": { "key": { "type":"string" } },
                    "required": ["key"]
                }),
            },
        }]),
        tool_choice: Some(json!({"type":"function","function":{"name":"lookup"}})),
        response_format: None,
        stream: Some(true),
    };

    let out = to_responses_request(&req, None);

    // Tools mapping
    let tools = out.tools.expect("missing tools");
    assert_eq!(tools.len(), 1);
    match &tools[0] {
        chat2response::models::responses::ResponsesToolDefinition::Function { function } => {
            assert_eq!(function.name, "lookup");
            assert_eq!(function.description.as_deref(), Some("Lookup a value"));
            assert!(function.parameters.is_object());
        }
    }

    // Tool choice forwarded
    assert_eq!(
        out.tool_choice,
        Some(json!({"type":"function","function":{"name":"lookup"}}))
    );
    // Stream forwarded
    assert_eq!(out.stream, Some(true));
}

#[test]
fn response_format_forwarding_and_type_override_protection() {
    // extras contains a conflicting "type"; converter should keep kind as the "type" field
    let mut extras = HashMap::new();
    extras.insert("schema".to_string(), json!({"type":"object"}));
    extras.insert("type".to_string(), json!("should_not_override"));

    let req = ChatCompletionRequest {
        model: "gpt-4o-mini".into(),
        messages: vec![ChatMessage {
            role: Role::User,
            content: json!("Return JSON please"),
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
        tools: None,
        tool_choice: None,
        response_format: Some(ResponseFormat {
            kind: "json_object".into(),
            extra: extras,
        }),
        stream: None,
    };

    let out = to_responses_request(&req, None);
    let rf = out.response_format.expect("missing response_format");
    assert_eq!(rf.get("type").and_then(|v| v.as_str()), Some("json_object"));
    assert!(rf.get("schema").is_some());
    // Ensure extras "type" did not override
    assert_ne!(
        rf.get("type").and_then(|v| v.as_str()),
        Some("should_not_override")
    );
}

#[test]
fn content_array_is_preserved_for_multimodal_shape() {
    let content = json!([
        { "type": "text", "text": "Describe this image" },
        { "type": "image_url", "image_url": { "url": "https://example.com/cat.png" } }
    ]);

    let req = ChatCompletionRequest {
        model: "gpt-4o-mini".into(),
        messages: vec![ChatMessage {
            role: Role::User,
            content: content.clone(),
            name: None,
            tool_call_id: None,
        }],
        temperature: None,
        top_p: None,
        max_tokens: Some(32),
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

    let out = to_responses_request(&req, Some("conv-42".into()));
    assert_eq!(out.messages.len(), 1);
    assert_eq!(out.messages[0].role, "user");
    assert_eq!(out.messages[0].content, content);
    assert_eq!(out.max_output_tokens, Some(32));
    assert_eq!(out.conversation.as_deref(), Some("conv-42"));
}

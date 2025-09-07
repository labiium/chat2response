use serde::{Deserialize, Serialize};
use serde_with::skip_serializing_none;
use std::collections::HashMap;

/// Chat Completions role enumeration.
///
/// Uses lowercase serialization to match the OpenAI Chat API:
/// "system" | "user" | "assistant" | "tool" | "function"
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Role {
    System,
    User,
    Assistant,
    Tool,
    /// Legacy alias present in some Chat Completions payloads.
    /// When converting to Responses, this typically maps to "tool".
    Function,
}

/// Minimal Chat message model compatible with the Chat Completions API.
///
/// Notes:
/// - `content` may be a string or an array of message parts; we accept `serde_json::Value`
///   to allow both shapes (and future-proof for multimodal content).
/// - `name` and `tool_call_id` are optional fields that may appear on assistant or tool messages.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[skip_serializing_none]
pub struct ChatMessage {
    pub role: Role,
    /// Chat API allows a string or an array of content parts (for multimodal).
    pub content: serde_json::Value,
    /// Optional name for function/tool messages.
    #[serde(default)]
    pub name: Option<String>,
    /// Optional tool call identifier (tool result correlation).
    #[serde(default)]
    pub tool_call_id: Option<String>,
}

/// JSON Schema for a function tool definition in Chat Completions.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[skip_serializing_none]
pub struct FunctionDef {
    pub name: String,
    #[serde(default)]
    pub description: Option<String>,
    /// JSON Schema object describing the function parameters.
    pub parameters: serde_json::Value,
}

/// Chat Completions tool definition (subset).
///
/// Example:
/// {
///   "type": "function",
///   "function": { "name": "...", "description": "...", "parameters": { ... } }
/// }
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ToolDefinition {
    Function { function: FunctionDef },
}

/// Response format hint for structured outputs in Chat Completions.
///
/// Example: { "type": "json_object", "schema": { ... } }
#[derive(Debug, Clone, Serialize, Deserialize)]
#[skip_serializing_none]
pub struct ResponseFormat {
    /// e.g., "json_object"
    #[serde(rename = "type")]
    pub kind: String,
    /// Additional fields such as "schema" may be present.
    #[serde(flatten)]
    pub extra: HashMap<String, serde_json::Value>,
}

/// Chat Completions request (commonly used subset).
///
/// This model intentionally uses flexible types (e.g., `serde_json::Value` for `stop`)
/// to accept both strings and arrays where the API allows it.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[skip_serializing_none]
pub struct ChatCompletionRequest {
    pub model: String,
    pub messages: Vec<ChatMessage>,

    // Sampling / decoding
    #[serde(default)]
    pub temperature: Option<f64>,
    #[serde(default)]
    pub top_p: Option<f64>,
    #[serde(default)]
    pub max_tokens: Option<u32>,
    /// Accepts a single string or an array of strings.
    #[serde(default)]
    pub stop: Option<serde_json::Value>,
    #[serde(default)]
    pub presence_penalty: Option<f64>,
    #[serde(default)]
    pub frequency_penalty: Option<f64>,
    #[serde(default)]
    pub logit_bias: Option<HashMap<String, f64>>,
    #[serde(default)]
    pub user: Option<String>,
    #[serde(default)]
    pub n: Option<u32>,

    // Tools
    #[serde(default)]
    pub tools: Option<Vec<ToolDefinition>>,
    #[serde(default)]
    pub tool_choice: Option<serde_json::Value>,

    // Formatting
    #[serde(default)]
    pub response_format: Option<ResponseFormat>,

    // Streaming
    #[serde(default)]
    pub stream: Option<bool>,
}

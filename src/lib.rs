#![forbid(unsafe_code)]
#![doc = r#"
Chat2Response

Translate OpenAI Chat Completions requests into Responses API payloads and proxy them to OpenAI's Responses endpoint.

Crate highlights
- Library: pure conversion via `to_responses_request(&ChatCompletionRequest, Option<String>)`.
- HTTP server (in `server`): `/convert` and `/proxy` (always available; proxy forwards to `OPENAI_BASE_URL`).
- Models: minimal but robust request models for Chat Completions and Responses APIs.

Modules
- `models`: Data structures for Chat and Responses.
- `conversion`: Mapping logic from Chat → Responses.
- `server`: Axum router/handlers (optional binary uses this).
- `util`: Shared helpers (tracing, env, SSE utilities).

Note: Keep the mapping rules aligned with OpenAI docs; the Responses API evolves over time.
"#]

pub mod auth;

pub mod conversion;
pub mod mcp_client;
pub mod mcp_config;
pub mod models;
pub mod server;
pub mod system_prompt_config;
pub mod util;

// Re-export the primary conversion function for ergonomic library use.
pub use crate::auth::{ApiKeyInfo, ApiKeyManager, GeneratedKey, Verification};

pub use crate::conversion::to_responses_request;

// Re-export model namespaces for convenience (downstream users can do `use chat2response::chat`).
pub use crate::models::{chat, responses};

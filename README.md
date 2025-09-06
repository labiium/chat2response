# Chat2Response

[![License: Apache-2.0](https://img.shields.io/badge/License-Apache%202.0-blue.svg)](LICENSE) [![Rust Edition: 2021](https://img.shields.io/badge/Rust-2021-orange.svg)](https://www.rust-lang.org/)
[Repository](https://github.com/labiium/chat2response) • [Issues](https://github.com/labiium/chat2response/issues) • [Contributing (PR checklist)](CONTRIBUTING.md#tldr--contributor-checklist) • [Security](SECURITY.md) • [Code of Conduct](CODE_OF_CONDUCT.md) • [License](LICENSE)

For pull requests, see the basic PR checklist in Contributing.

Translate OpenAI Chat Completions requests into the Responses API format. By default, the built-in proxy forwards those converted requests to OpenAI’s Responses endpoint (including streaming). Optionally, you can enable a Chat Completions upstream compatibility mode by setting UPSTREAM_MODE=chat (or chat-completions, or CHAT2RESPONSE_UPSTREAM=chat), which rewrites the upstream to /chat/completions and adapts the payload shape.

- Crate name: chat2response
- Binary: chat2response
- Minimum Rust: 1.72+ (Rust 2021, Tokio 1.x)

----------------------------------------------------------------

1) Response API vs. Chat Completions API — key differences (with sources)

Scope and mental model
- Chat Completions: stateless chat-style interface. The client resends full message history per call and receives choices back. Tool/function calling exists, but orchestration is client-managed. Source: OpenAI Chat Completions docs.
  - https://platform.openai.com/docs/guides/text-generation
  - https://platform.openai.com/docs/api-reference/chat
- Responses API: unified, extensible interface for textual outputs, tools, and multimodal inputs, with first-class streaming events and optional server-side conversation state. Source: OpenAI Responses docs.
  - https://platform.openai.com/docs/guides/responses
  - https://platform.openai.com/docs/api-reference/responses

Input model
- Chat Completions: messages: [{ role, content }], content commonly text or content parts for multimodal models. Entire history is sent each request. Source: Chat API reference.
- Responses: generalizes input via input and also supports a chat-form messages shape; can associate requests with a server-side conversation to avoid resending history. Source: Responses guide and API reference.

Output model
- Chat Completions: returns chat.completion with choices[*].message for non-streaming; deltas over SSE for streaming. Source: Chat API reference.
- Responses: returns response with a normalized output array and typed streaming events (text deltas, tool activity, etc.), enabling consistent multimodal and tool traces. Source: Responses guide and streaming examples.

State and memory
- Chat Completions: the client maintains state (message history), sending it on every call. Source: Chat usage guidance.
- Responses: supports stateful interactions via conversation identifiers in addition to stateless calls. Source: Responses guide (conversations).

Tools and retrieval
- Chat Completions: tool/function calling available; integration around inputs/outputs is client-side. Source: Chat tool calling docs.
- Responses: unifies built-in capabilities and tool traces into the event stream and response schema, simplifying agentic flows. Source: Responses guide (tools/events).

Structured outputs
- Both support structured output and JSON. Responses integrates schema guidance uniformly across modalities and tool runs; Chat Completions uses response_format in newer models. Sources:
  - Chat: https://platform.openai.com/docs/guides/structured-outputs
  - Responses: https://platform.openai.com/docs/guides/structured-outputs (Responses section)

Migration stance
- New development is encouraged to target the Responses API; mappings from Chat (messages, sampling controls) are direct for common parameters. Sources: Responses migration notes and API docs.

Notes
- Parameter names differ in places (e.g., Chat’s max_tokens vs. Responses’ max_output_tokens). This crate provides a conservative, explicit mapping aligned with the public docs at the time of writing (2024-10).

----------------------------------------------------------------

2) What this crate provides

- /convert
  - Accepts a Chat Completions request; returns the equivalent Responses API request payload (JSON). No outbound network.
- /proxy (forwards to OpenAI’s Responses endpoint; always available; requires OPENAI_API_KEY)
  - Same input as /convert, but forwards the converted request to OpenAI’s Responses endpoint and returns the native output. Supports streaming passthrough (SSE).
- Library API
  - chat2response::to_responses_request(...) converts in-process without any server.
- MCP stdio server (always included)
  - Exposes two tools to MCP clients: convert and proxy.

Design
- Axum + tokio for HTTP; reqwest for outbound; serde for models; tower-http for CORS/logging; tracing for logs.
- Field coverage for common Chat parameters: messages, model, temperature, top_p, max_tokens, stop, presence_penalty, frequency_penalty, logit_bias, user, n, tools (function), tool_choice, response_format, stream.
- Conversion rules:
  - messages[] → Responses chat-form messages.
  - max_tokens → max_output_tokens.
  - tools → Responses tools (function schemas preserved).
  - tool_choice → forwarded.
  - response_format (e.g., { "type": "json_object" }) → forwarded.
  - stream → forwarded; /proxy relays SSE.
  - Optional query ?conversation_id=... sets Responses conversation for stateful flows.

----------------------------------------------------------------

Quick start

Build the binary

```/dev/null/terminal.sh#L1-8
# Release build
cargo build --release

# Or run directly
cargo run --release
```

Converter-only server (no outbound)

```/dev/null/terminal.sh#L1-6
# Starts HTTP server at 0.0.0.0:8088 by default
./target/release/chat2response

# Or:
cargo run --release
```

Proxy mode (forwards to OpenAI Responses)

```/dev/null/terminal.sh#L1-6
# Requires OPENAI_API_KEY
OPENAI_API_KEY=sk-... cargo run --release

# Optional: override base URL
OPENAI_BASE_URL=https://api.openai.com/v1 OPENAI_API_KEY=sk-... cargo run --release
```

MCP stdio server

```/dev/null/terminal.sh#L1-12
# Converter-only MCP (stdio)
CHAT2RESPONSE_MCP=1 cargo run --release

# MCP + Proxy (needs OPENAI_API_KEY)
CHAT2RESPONSE_MCP=1 OPENAI_API_KEY=sk-... cargo run --release

# Example Claude desktop config (conceptual):
# { "mcpServers": { "chat2response": { "command": "/abs/path/to/chat2response",
#   "env": { "CHAT2RESPONSE_MCP": "1", "OPENAI_API_KEY": "sk-..." }, "args": [] } } }
```

----------------------------------------------------------------

HTTP API

- POST /convert[?conversation_id=...]
  - Body: Chat Completions request JSON
  - Returns: Responses request JSON (not executed)
- POST /proxy[?conversation_id=...] (requires OPENAI_API_KEY)
  - Body: Chat Completions request JSON
  - Returns: Responses native output (JSON) or SSE stream if stream==true

Example: convert

```/dev/null/request.json#L1-9
{
  "model": "gpt-4o-mini",
  "messages": [
    { "role": "system", "content": "You are helpful." },
    { "role": "user", "content": "Say hi" }
  ],
  "max_tokens": 32
}
```

```/dev/null/terminal.sh#L1-7
curl -sS localhost:8088/convert \
  -H 'content-type: application/json' \
  -d @request.json | jq
```

Example: proxy (non-streaming)

```/dev/null/terminal.sh#L1-9
# Server running (requires OPENAI_API_KEY)

curl -sS localhost:8088/proxy \
  -H 'content-type: application/json' \
  -d '{
        "model":"gpt-4o-mini",
        "messages":[{"role":"user","content":"Hello"}]
      }' | jq
```

Example: proxy (streaming)

```/dev/null/terminal.sh#L1-7
curl -N localhost:8088/proxy \
  -H 'content-type: application/json' \
  -d '{"model":"gpt-4o-mini","messages":[{"role":"user","content":"Stream please"}],"stream":true}'
```

----------------------------------------------------------------

Library usage

```/dev/null/usage.rs#L1-34
use chat2response::{to_responses_request};
use chat2response::models::chat::{ChatCompletionRequest, ChatMessage, Role};

fn main() {
    let req = ChatCompletionRequest {
        model: "gpt-4o-mini".into(),
        messages: vec![
            ChatMessage { role: Role::System, content: serde_json::json!("You are helpful."), name: None, tool_call_id: None },
            ChatMessage { role: Role::User, content: serde_json::json!("Hello"), name: None, tool_call_id: None },
        ],
        temperature: Some(0.2),
        top_p: None,
        max_tokens: Some(32),
        stop: None,
        presence_penalty: None,
        frequency_penalty: None,
        logit_bias: None,
        user: Some("example".into()),
        n: None,
        tools: None,
        tool_choice: None,
        response_format: None,
        stream: Some(false),
    };

    let converted = to_responses_request(&req, Some("conv-123".into()));
    println!("{}", serde_json::to_string_pretty(&converted).unwrap());
}
```

----------------------------------------------------------------

Configuration

Environment variables
- BIND_ADDR: default 0.0.0.0:8088
- OPENAI_API_KEY: required for /proxy (or MCP proxy tool)
- OPENAI_BASE_URL: default https://api.openai.com/v1
- UPSTREAM_MODE: default "responses"; set to "chat" or "chat-completions" to forward upstream to the Chat Completions endpoint (/chat/completions) with payload adaptation
- CHAT2RESPONSE_UPSTREAM: alias of UPSTREAM_MODE
- CHAT2RESPONSE_UPSTREAM_INPUT: when truthy (1,true,yes,on), inject a derived top-level "input" string for upstreams that require it (non-streaming and streaming paths)
- CORS_ALLOWED_ORIGINS: "*" or comma-separated list (e.g., "https://a.com, https://b.com")
- CORS_ALLOWED_METHODS: "*" or comma-separated methods (e.g., "GET,POST,OPTIONS")
- CORS_ALLOWED_HEADERS: "*" or comma-separated header names
- CORS_ALLOW_CREDENTIALS: enable with 1,true,yes,on
- CORS_MAX_AGE: seconds for preflight caching (e.g., 600)

```/dev/null/.env#L1-6
BIND_ADDR=0.0.0.0:8088
OPENAI_API_KEY=sk-your-key
# OPENAI_BASE_URL=https://api.openai.com/v1
```

CORS and logging
- CORS is configurable via env: CORS_ALLOWED_ORIGINS, CORS_ALLOWED_METHODS, CORS_ALLOWED_HEADERS, CORS_ALLOW_CREDENTIALS, CORS_MAX_AGE. Defaults are permissive ("Any"); set explicit values for production.
- Use RUST_LOG for log levels, e.g., RUST_LOG=debug,hyper=info,tower_http=info.

----------------------------------------------------------------

Conversion details (implemented)

- messages: forwarded 1:1 as Responses chat-form messages. Role mapping includes legacy "function" → "tool".
- max_tokens → max_output_tokens
- Sampling: temperature, top_p, presence_penalty, frequency_penalty forwarded.
- stop: forwarded (string or array)
- logit_bias: forwarded
- user, n: forwarded
- tools (function): schema preserved; tool_choice forwarded
- response_format: forwarded as provided (e.g., { "type": "json_object" })
- stream: forwarded; /proxy relays SSE to the caller unchanged
- conversation_id (query): mapped to Responses conversation

Limitations and extensions
- If you need Responses built-in tools (e.g., web/file search, computer-use), extend models/responses.rs with corresponding tool variants and pass them through.
- If your Chat clients send multimodal parts in content arrays, you may switch to Responses input (multi-part) shape instead of messages; current default preserves chat shape.

----------------------------------------------------------------

Testing

```/dev/null/terminal.sh#L1-5
# Run the full test suite
cargo test

# Expect green; tests cover parameter and role mappings.
```

The tests verify:
- Role mapping and message preservation
- max_tokens → max_output_tokens
- conversation id propagation
- response_format passthrough
- Tools/function schema mapping

----------------------------------------------------------------

Security

- Do not embed API keys in code or commit history.
- In multi-tenant scenarios, place the proxy behind your own auth and rate limits; consider per-tenant credentials and audit logging.
- For MCP mode, the proxy tool requires OPENAI_API_KEY in the environment or a secure secret source.

----------------------------------------------------------------

License

Apache-2.0

----------------------------------------------------------------

References (selected)

- OpenAI API Reference: Chat Completions
  - https://platform.openai.com/docs/api-reference/chat
- OpenAI API Reference: Responses
  - https://platform.openai.com/docs/api-reference/responses
- Guides
  - Responses: https://platform.openai.com/docs/guides/responses
  - Structured outputs: https://platform.openai.com/docs/guides/structured-outputs

----------------------------------------------------------------

End-to-end tests (Python)

These tests spawn the Rust HTTP server via cargo and exercise the HTTP endpoints end-to-end. They also create a pseudo .env in a temporary directory to configure the server (bind address, API base URL, etc.).

Requirements
- Rust toolchain with cargo
- Python 3.9+
- Install test dependencies:
  - pip install -r e2e/requirements.txt

Test files
- e2e/test_e2e_http.py
- e2e/test_e2e_openai_client.py
- e2e/test_e2e_chat_compat.py

What the tests do
- test_convert_endpoint_e2e: Starts the server, POSTs to /convert, and validates the Responses payload fields.
- test_convert_multimodal_and_tools_e2e: Verifies multimodal content, tools mapping, tool_choice, and response_format forwarding.
- test_proxy_endpoint_e2e_with_mock_upstream: Launches a local mock of the OpenAI Responses endpoint, starts the server (proxy always available), and asserts that /proxy forwards and authenticates correctly.

Running the tests
- Full suite:
```/dev/null/terminal.sh#L1-3
python -m pip install -r e2e/requirements.txt
pytest -q e2e
```
- Individual tests:
```/dev/null/terminal.sh#L1-5
pytest -q e2e/test_e2e_http.py::test_convert_endpoint_e2e
pytest -q e2e/test_e2e_http.py::test_convert_multimodal_and_tools_e2e
pytest -q e2e/test_e2e_http.py::test_proxy_endpoint_e2e_with_mock_upstream
```

About the pseudo .env
- Each test writes a temporary .env with variables such as:
  - BIND_ADDR=127.0.0.1:<dynamic_port>
  - OPENAI_API_KEY=sk-test (proxy test only)
  - OPENAI_BASE_URL=http://127.0.0.1:<mock_port>/v1 (proxy test only)
- The server loads this file at startup via dotenv, so you do not need to export these in your shell during tests.

Notes
- The proxy tests use a local mock upstream server and do not contact the real OpenAI API.
- OpenAI Python client base_url usage:
  - For Responses API tests (test_e2e_openai_client.py), point the client to the mock upstream: base_url="http://127.0.0.1:<mock_port>/v1".
  - The server’s upstream target for Responses is controlled by OPENAI_BASE_URL in the pseudo .env (e.g., OPENAI_BASE_URL=http://127.0.0.1:<mock_port>/v1).
- Ensure cargo is available in PATH; tests automatically skip if cargo is missing.
# Chat2Response

[![License: Apache-2.0](https://img.shields.io/badge/License-Apache%202.0-blue.svg)](LICENSE)

Convert OpenAI Chat Completions requests to the new Responses API format.

Chat2Response bridges the gap between OpenAI's legacy Chat Completions API and the powerful new Responses API. Get all the benefits of the modern API without rewriting your existing Chat Completions code.

## CLI Usage

Run the server:
```bash
chat2response [mcp.json] [--keys-backend=redis://...|sled:<path>|memory]
```

- `mcp.json` positional: if provided as the first non-flag argument, the server loads and connects to MCP servers defined in that file.
- `--keys-backend`: selects the API key storage backend at runtime:
  - `redis://...` uses Redis with an r2d2 connection pool (pool size via `CHAT2RESPONSE_REDIS_POOL_MAX`).
  - `sled:<path>` uses an embedded sled database at the given path.
  - `memory` uses an in-memory, non-persistent store.

Backend precedence (highest to lowest):
1) `--keys-backend=...` (CLI)
2) `CHAT2RESPONSE_REDIS_URL` (if set, Redis is used)
3) `sled` (embedded; used when no Redis URL is provided)
4) `memory` (fallback)

Examples:
- Basic server (no MCP):
```bash
chat2response
```
- With MCP configuration file:
```bash
chat2response mcp.json
```
- Use Redis explicitly (CLI overrides env):
```bash
chat2response --keys-backend=redis://127.0.0.1/
```
- Use sled at a custom path:
```bash
chat2response --keys-backend=sled:./data/keys.db
```
- Force in-memory store (useful for demos/tests):
```bash
chat2response --keys-backend=memory
```
- With environment variables:
```bash
CHAT2RESPONSE_REDIS_URL=redis://127.0.0.1/ chat2response
```

## Why Use This?

- Easy migration: keep your Chat Completions code, get Responses API benefits
- Better streaming with typed events and tool traces
- Unified interface for text, tools, and multimodal inputs
- Lightweight single binary
- Add Responses API support to vLLM, Ollama, and other providers
- MCP integration for enhanced tool capabilities

## Quick Start

### Install and Run

```bash
# Install from crates.io (preferred)
cargo install chat2response

# Clone and build (alternative)
git clone https://github.com/labiium/chat2response
cd chat2response
cargo install --path .

# Start the server (basic mode)
OPENAI_API_KEY=sk-your-key chat2response

# Start with MCP integration
OPENAI_API_KEY=sk-your-key chat2response mcp.json
```

Server runs at `http://localhost:8088`.

### Run with Docker (GHCR)

Official images are published to GitHub Container Registry (GHCR):
- Image: `ghcr.io/labiium/chat2response`
- Tags:
  - `edge` — latest commit on `main`/`master`
  - Release tags — `vX.Y.Z`, `X.Y`, `X`, and `latest` on versioned releases
  - `sha-<short>` — content‐addressed builds

Pull:
```bash
docker pull ghcr.io/labiium/chat2response:edge
```

Basic run (sled backend with persistent volume):
```bash
docker run --rm -p 8088:8088 \
  -e OPENAI_BASE_URL=https://api.openai.com/v1 \
  -e OPENAI_API_KEY=sk-your-key \
  -v chat2response-data:/data \
  ghcr.io/labiium/chat2response:edge
```

Use Redis backend:
```bash
# macOS/Windows (Docker Desktop)
docker run --rm -p 8088:8088 \
  -e OPENAI_BASE_URL=https://api.openai.com/v1 \
  -e CHAT2RESPONSE_REDIS_URL=redis://host.docker.internal:6379/ \
  ghcr.io/labiium/chat2response:edge --keys-backend=redis://host.docker.internal:6379/

# Linux (access local Redis)
docker run --rm --network=host \
  -e OPENAI_BASE_URL=https://api.openai.com/v1 \
  -e CHAT2RESPONSE_REDIS_URL=redis://127.0.0.1:6379/ \
  ghcr.io/labiium/chat2response:edge --keys-backend=redis://127.0.0.1:6379/
```

With MCP configuration:
```bash
# Ensure mcp.json exists locally
docker run --rm -p 8088:8088 \
  -e OPENAI_BASE_URL=https://api.openai.com/v1 \
  -v $(pwd)/mcp.json:/app/mcp.json:ro \
  -v chat2response-data:/data \
  ghcr.io/labiium/chat2response:edge /app/mcp.json
```

Notes:
- Defaults inside the image:
  - `BIND_ADDR=0.0.0.0:8088` (listens on all interfaces)
  - `CHAT2RESPONSE_SLED_PATH=/data/keys.db` (mount a volume for persistence)
- You can pass CLI flags exactly as with the binary (e.g., `--keys-backend=...`).
- For corporate proxies, set `HTTP_PROXY`/`HTTPS_PROXY` env vars.

### Convert Only (No API Calls)

```bash
curl -X POST http://localhost:8088/convert \
  -H "Content-Type: application/json" \
  -d '{
    "model": "gpt-4o",
    "messages": [{"role": "user", "content": "Hello!"}]
  }'
```

Returns the equivalent Responses API request (no OpenAI call made).

### Full Proxy (Calls OpenAI)

```bash
curl -X POST http://localhost:8088/proxy \
  -H "Content-Type: application/json" \
  -d '{
    "model": "gpt-4o",
    "messages": [{"role": "user", "content": "Hello!"}],
    "stream": true
  }'
```

Converts your request and forwards it to OpenAI's Responses endpoint.

## Key Features

### Automatic Conversion
- `messages` → Responses format
- `max_tokens` → `max_output_tokens`
- `tools` → Responses tool schema
- All parameters mapped correctly

### Streaming Support
Get real-time responses with proper event types:
```bash
# Streaming works out of the box
curl -N http://localhost:8088/proxy \
  -H "Content-Type: application/json" \
  -d '{"model":"gpt-4o","messages":[{"role":"user","content":"Stream this"}],"stream":true}'
```

### Library Usage
Use as a Rust library for in-process conversion:

```rust
use chat2response::to_responses_request;
use chat2response::models::chat::ChatCompletionRequest;

let chat_request = ChatCompletionRequest { /* ... */ };
let responses_request = to_responses_request(&chat_request, None);
```

## Configuration

Set these environment variables:

```bash
# Required
OPENAI_BASE_URL=https://api.openai.com/v1        # Upstream base URL (mandatory)

# Optional (Upstream behavior)
OPENAI_API_KEY=sk-your-key                       # Used if Authorization header is not provided
BIND_ADDR=0.0.0.0:8088                           # Server address
UPSTREAM_MODE=responses                          # "responses" (default) or "chat" for Chat Completions upstream
CHAT2RESPONSE_UPSTREAM_INPUT=0                   # If 1/true, derive and send top-level "input" when upstream requires it

# Optional (Proxy/network)
CHAT2RESPONSE_PROXY_URL=                         # Proxy for all schemes (e.g., http://user:pass@host:port)
HTTP_PROXY=                                      # Standard env var for HTTP proxy
HTTPS_PROXY=                                     # Standard env var for HTTPS proxy
CHAT2RESPONSE_NO_PROXY=0                         # If 1/true, disable all proxy usage
CHAT2RESPONSE_HTTP_TIMEOUT_SECONDS=60            # Global HTTP client timeout (seconds)

# Optional (CORS)
CORS_ALLOWED_ORIGINS=*                           # "*" or comma-separated origins
CORS_ALLOWED_METHODS=*                           # "*" or comma-separated (GET,POST,...)
CORS_ALLOWED_HEADERS=*                           # "*" or comma-separated header names
CORS_ALLOW_CREDENTIALS=0                         # If 1/true, allow credentials
CORS_MAX_AGE=3600                                # Preflight max-age (seconds)

# Optional (API key management & auth)
# Backend selection is runtime-based:
# - If CHAT2RESPONSE_REDIS_URL is set, Redis is used (r2d2 pool).
# - Else, if built with the `sled` feature, sled is used (when present).
# - Else, in-memory store is used (non-persistent, for dev/tests).
CHAT2RESPONSE_REDIS_URL=                         # e.g., redis://127.0.0.1/
CHAT2RESPONSE_REDIS_POOL_MAX=16                  # r2d2 pool max size for Redis
CHAT2RESPONSE_SLED_PATH=./data/keys.db           # Path for sled data (only when built with sled feature)

# Key lifecycle policy
CHAT2RESPONSE_KEYS_REQUIRE_EXPIRATION=1          # If 1/true, keys must have expiration at creation
CHAT2RESPONSE_KEYS_ALLOW_NO_EXPIRATION=0         # If 1/true, allow non-expiring keys (not recommended)
CHAT2RESPONSE_KEYS_DEFAULT_TTL_SECONDS=86400     # Default TTL (seconds) used if not explicitly provided
```

### API Key Backends
- Redis: enable by setting `CHAT2RESPONSE_REDIS_URL` at runtime. Pool size via `CHAT2RESPONSE_REDIS_POOL_MAX`.
- Sled: available when compiled with the `sled` feature; set `CHAT2RESPONSE_SLED_PATH` for database path. Used only if Redis URL is not set.
- Memory: fallback non-persistent store for development/testing when neither Redis is configured nor sled is available.

### API Key Policy
- Tokens are opaque: `sk_<id>.<secret>` (ID is 32 hex chars, secret is 64 hex chars).
- Secrets are never stored; verification uses salted `SHA-256(salt || secret)` with constant-time compare.
- By default, expiration is required at creation (`CHAT2RESPONSE_KEYS_REQUIRE_EXPIRATION=1`), using either `ttl_seconds` or `expires_at`. You can allow non-expiring keys only if `CHAT2RESPONSE_KEYS_ALLOW_NO_EXPIRATION=1`.
- A default TTL can be set via `CHAT2RESPONSE_KEYS_DEFAULT_TTL_SECONDS`.

## API Endpoints

| Endpoint | Purpose | Requires API Key |
|----------|---------|------------------|
| `POST /convert` | Convert request format only | No |
| `POST /proxy` | Convert + forward to OpenAI | Yes (`X-API-Key`) |
| `GET /keys` | List API keys (`id`, `label`, `created_at`, `expires_at`, `revoked_at`, `scopes`) | No (protect via network ACL) |
| `POST /keys/generate` | Create a new API key; body supports `label`, `ttl_seconds` or `expires_at`, and `scopes` | No (protect via network ACL) |
| `POST /keys/revoke` | Revoke an API key; body: `{ "id": "<key-id>" }` | No (protect via network ACL) |
| `POST /keys/set_expiration` | Set/clear expiration; body: `{ "id": "...", "expires_at": <epoch>|null, "ttl_seconds": <u64> }` | No (protect via network ACL) |

Notes:
- The `/proxy` route is authenticated via the `X-API-Key` header. You can pass the raw token or `Bearer <token>`. Example: `-H "X-API-Key: sk_<id>.<secret>"`.
- Key management endpoints do not implement separate admin auth; deploy behind a trusted network, reverse proxy ACL, or service mesh policy.

Both endpoints accept standard Chat Completions JSON and support `?conversation_id=...` for stateful conversations.

## Chat vs Responses API

Chat Completions (Legacy)
- Stateless — send full history each time
- Limited streaming events
- Client manages conversation state

Responses API (Modern)
- Optional server-side conversation state
- Rich streaming with typed events
- Unified tool and multimodal handling
- Better for AI agents and complex flows

Chat2Response lets you get Responses API benefits while keeping your existing Chat Completions code.

## Use with vLLM, Ollama & Local Models

Many popular inference servers only support the Chat Completions API:
- vLLM — High-performance inference server
- Ollama — Local model runner
- Text Generation WebUI — Popular local interface
- FastChat — Multi-model serving
- LocalAI — Local OpenAI alternative

Place Chat2Response in front of these services to instantly add Responses API support:

```bash
# Point to your local vLLM server
OPENAI_BASE_URL=http://localhost:8000/v1 \
UPSTREAM_MODE=chat \
chat2response
```

Now your local models support the modern Responses API format. Your applications get better streaming, tool traces, and conversation state while your local inference server keeps running unchanged.

## MCP Integration

Chat2Response can connect to Model Context Protocol (MCP) servers to provide additional tools to the LLM. When MCP servers are configured, their tools are automatically merged with any tools specified in the original request.

### Setting up MCP

1) Create an MCP configuration file (`mcp.json`):

```json
{
  "mcpServers": {
    "filesystem": {
      "command": "npx",
      "args": ["-y", "@modelcontextprotocol/server-filesystem", "/tmp"]
    },
    "brave-search": {
      "command": "npx",
      "args": ["-y", "@modelcontextprotocol/server-brave-search"],
      "env": {
        "BRAVE_API_KEY": "your-brave-api-key-here"
      }
    },
    "postgres": {
      "command": "npx",
      "args": ["-y", "@modelcontextprotocol/server-postgres"],
      "env": {
        "POSTGRES_CONNECTION_STRING": "postgresql://user:password@localhost:5432/database"
      }
    }
  }
}
```

2) Start the server with MCP support:

```bash
OPENAI_BASE_URL=https://api.openai.com/v1 chat2response mcp.json
```

3) Available MCP Servers:
- `@modelcontextprotocol/server-filesystem` — File system operations
- `@modelcontextprotocol/server-brave-search` — Web search via Brave
- `@modelcontextprotocol/server-postgres` — PostgreSQL database access
- `@modelcontextprotocol/server-github` — GitHub API integration
- Many more available on npm

### How MCP Tools Work

- MCP tools are automatically discovered and added to the available tool list.
- Tool names are prefixed with the server name (e.g., `filesystem_read_file`).
- The LLM can call MCP tools just like regular function tools.
- Tool execution happens automatically and results are injected into the conversation.
- Multiple MCP servers can run simultaneously.

### Example Request with MCP Tools

When MCP servers are connected, your regular Chat Completions request automatically gains access to their tools:

```bash
curl -X POST http://localhost:8088/proxy \
  -H "Content-Type: application/json" \
  -d '{
    "model": "gpt-4o",
    "messages": [
      {"role": "user", "content": "Search for recent news about AI and save the results to a file"}
    ]
  }'
```

The LLM will automatically have access to both `brave-search_search` and `filesystem_write_file` tools.

## Installation

From crates.io (binary):
```bash
cargo install chat2response
```

Run the CLI:
```bash
OPENAI_API_KEY=sk-your-key chat2response [mcp.json] [--keys-backend=redis://...|sled:<path>|memory]
```

From source:
```bash
git clone https://github.com/labiium/chat2response
cd chat2response
cargo build --release
```

As a library:
```toml
[dependencies]
chat2response = "0.1"
```

## Testing

```bash
# Unit and integration tests
cargo test

# With sled backend compiled (optional feature) and custom sled path:
cargo test --features sled
CHAT2RESPONSE_SLED_PATH=./tmp/keys.db cargo test --features sled

# With Redis backend at runtime (ensure a local Redis instance is available):
export CHAT2RESPONSE_REDIS_URL=redis://127.0.0.1/
cargo test

# Lints (fail on warnings)
cargo clippy --all-targets -- -D warnings

# End-to-end tests (requires Python)
pip install -r e2e/requirements.txt
pytest e2e/
```

### Manual API Key Flows

```bash
# Generate a key (1-day TTL)
curl -s -X POST http://localhost:8088/keys/generate \
  -H "Content-Type: application/json" \
  -d '{"label":"svc","ttl_seconds":86400}'

# Use it with /proxy
curl -s -X POST "http://localhost:8088/proxy" \
  -H "Content-Type: application/json" \
  -H "X-API-Key: sk_<id>.<secret>" \
  -d '{"model":"gpt-4o","messages":[{"role":"user","content":"Hello"}]}'

# List keys
curl -s http://localhost:8088/keys

# Revoke a key
curl -s -X POST http://localhost:8088/keys/revoke \
  -H "Content-Type: application/json" \
  -d '{"id":"<key-id>"}'

# Set expiration (1 hour from now)
curl -s -X POST http://localhost:8088/keys/set_expiration \
  -H "Content-Type: application/json" \
  -d '{"id":"<key-id>","ttl_seconds":3600}'
```

## Examples

<details>
<summary>Convert with tools</summary>

```bash
curl -X POST http://localhost:8088/convert \
  -H "Content-Type: application/json" \
  -d '{
    "model": "gpt-4o",
    "messages": [{"role": "user", "content": "What is the weather?"}],
    "tools": [{
      "type": "function",
      "function": {
        "name": "get_weather",
        "description": "Get weather info",
        "parameters": {
          "type": "object",
          "properties": {"location": {"type": "string"}},
          "required": ["location"]
        }
      }
    }]
  }'
```
</details>

<details>
<summary>Proxy with conversation ID</summary>

```bash
curl -X POST "http://localhost:8088/proxy?conversation_id=chat-123" \
  -H "Content-Type: application/json" \
  -d '{
    "model": "gpt-4o",
    "messages": [{"role": "user", "content": "Remember this conversation"}]
  }'
```
</details>

## License

Apache-2.0

## Links

- [OpenAI Responses API Guide](https://platform.openai.com/docs/guides/responses)
- [OpenAI Chat Completions API](https://platform.openai.com/docs/api-reference/chat)

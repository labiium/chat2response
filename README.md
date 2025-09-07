# Chat2Response

[![License: Apache-2.0](https://img.shields.io/badge/License-Apache%202.0-blue.svg)](LICENSE)

**Convert OpenAI Chat Completions requests to the new Responses API format**

Chat2Response bridges the gap between OpenAI's legacy Chat Completions API and the powerful new Responses API. Get all the benefits of the modern API without rewriting your existing Chat Completions code.

## Why Use This?

**🔄 Easy Migration** - Keep your existing Chat Completions code, get Responses API benefits  
**⚡ Better Streaming** - Improved streaming with typed events and tool traces  
**🎯 Unified Interface** - Handle text, tools, and multimodal inputs consistently  
**🚀 Lightweight** - Single binary with minimal resource usage  
**🏗️ vLLM & Local Models** - Add Responses API support to vLLM, Ollama, and other providers  
**🔧 MCP Integration** - Connect to Model Context Protocol servers for enhanced tool capabilities  

## Quick Start

### Install and Run

```bash
# Clone and build
git clone https://github.com/labiium/chat2response
cd chat2response
cargo build --release

# Start the server (basic mode)
OPENAI_API_KEY=sk-your-key ./target/release/chat2response

# Start with MCP integration
OPENAI_API_KEY=sk-your-key ./target/release/chat2response mcp.json
```

Server runs at `http://localhost:8088`

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

### 🔄 Automatic Conversion
- `messages` → Responses format
- `max_tokens` → `max_output_tokens`  
- `tools` → Responses tool schema
- All parameters mapped correctly

### 📡 Streaming Support
Get real-time responses with proper event types:
```bash
# Streaming works out of the box
curl -N http://localhost:8088/proxy \
  -H "Content-Type: application/json" \
  -d '{"model":"gpt-4o","messages":[{"role":"user","content":"Stream this"}],"stream":true}'
```

### 📚 Library Usage
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
# Required for /proxy endpoint
OPENAI_API_KEY=sk-your-key

# Optional settings
BIND_ADDR=0.0.0.0:8088              # Server address
OPENAI_BASE_URL=https://api.openai.com/v1  # OpenAI base URL
UPSTREAM_MODE=responses              # Use "chat" for Chat Completions upstream
```

## API Endpoints

| Endpoint | Purpose | Requires API Key |
|----------|---------|------------------|
| `POST /convert` | Convert request format only | No |
| `POST /proxy` | Convert + forward to OpenAI | Yes |

Both endpoints accept standard Chat Completions JSON and support `?conversation_id=...` for stateful conversations.

## Chat vs Responses API

**Chat Completions (Legacy)**
- Stateless - send full history each time
- Limited streaming events
- Client manages conversation state

**Responses API (Modern)** 
- Optional server-side conversation state
- Rich streaming with typed events
- Unified tool and multimodal handling
- Better for AI agents and complex flows

Chat2Response lets you get Responses API benefits while keeping your existing Chat Completions code.

## Use with vLLM, Ollama & Local Models

Many popular inference servers only support Chat Completions API:
- **vLLM** - High-performance inference server
- **Ollama** - Local model runner  
- **Text Generation WebUI** - Popular local interface
- **FastChat** - Multi-model serving
- **LocalAI** - Local OpenAI alternative

Place Chat2Response in front of these services to instantly add Responses API support:

```bash
# Point to your local vLLM server
OPENAI_BASE_URL=http://localhost:8000/v1 \
UPSTREAM_MODE=chat \
./target/release/chat2response
```

Now your local models support the modern Responses API format! Your applications get better streaming, tool traces, and conversation state while your local inference server keeps running unchanged.

## MCP Integration

Chat2Response can connect to Model Context Protocol (MCP) servers to provide additional tools to the LLM. When MCP servers are configured, their tools are automatically merged with any tools specified in the original request.

### Setting up MCP

1. **Create an MCP configuration file** (`mcp.json`):

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

2. **Start the server with MCP support**:

```bash
OPENAI_API_KEY=sk-your-key ./target/release/chat2response mcp.json
```

3. **Available MCP Servers**:
   - `@modelcontextprotocol/server-filesystem` - File system operations
   - `@modelcontextprotocol/server-brave-search` - Web search via Brave
   - `@modelcontextprotocol/server-postgres` - PostgreSQL database access
   - `@modelcontextprotocol/server-github` - GitHub API integration
   - Many more available on npm

### How MCP Tools Work

- MCP tools are automatically discovered and added to the available tool list
- Tool names are prefixed with the server name (e.g., `filesystem_read_file`)
- The LLM can call MCP tools just like regular function tools
- Tool execution happens automatically and results are injected into the conversation
- Multiple MCP servers can run simultaneously

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

**From Source:**
```bash
git clone https://github.com/labiium/chat2response
cd chat2response
cargo build --release
```

**As Library:**
```toml
[dependencies]
chat2response = "0.1"
```

## Testing

```bash
# Run all tests
cargo test

# Run end-to-end tests (requires Python)
pip install -r e2e/requirements.txt
pytest e2e/
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
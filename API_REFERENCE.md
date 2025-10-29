# Chat2Response – API Reference

Chat2Response is an HTTP service that:
- Converts OpenAI Chat Completions requests into the modern OpenAI Responses API payloads.
- Proxies both Chat Completions and Responses requests to one or more upstream providers.
- Optionally injects system prompts per model/API at runtime.
- Provides API key issuance/validation (managed mode), analytics collection, and runtime config reloads.

This document details all HTTP routes, expected authentication, parameters, and examples.

Base URL: http://localhost:PORT (default PORT configured by your deployment)

Content-Type: application/json unless otherwise specified


## Authentication

Chat2Response supports two modes:

1) Managed mode (recommended)
- Condition: Server has OPENAI_API_KEY set.
- Client sends an internal access key using Authorization: Bearer sk_<id>.<secret>.
- The proxy validates the token (issue/revoke/expire via the “Keys” endpoints) and substitutes the upstream provider key (OPENAI_API_KEY or per-backend key_env).
- Use the Keys endpoints to issue/revoke client tokens.

2) Passthrough mode
- Condition: OPENAI_API_KEY is NOT set on the server.
- Client sends their provider API key directly using Authorization: Bearer <provider_api_key>.
- The proxy forwards that upstream unchanged.

Common headers:
- Authorization: Bearer <token>
- Content-Type: application/json
- For streaming: Accept: text/event-stream and include "stream": true in body.

Error responses:
- Status: appropriate 4xx/5xx
- Body: {"error":{"message":"human-readable error"}}


## Routing and Multi-backend

You can route requests by model prefix and optionally translate payloads when the upstream only supports Chat Completions:

- CHAT2RESPONSE_BACKENDS rules (semicolon-separated):
  - prefix=<model_prefix>
  - base|base_url=<upstream_base_url>
  - key_env|api_key_env=<ENV_VAR_WITH_API_KEY> (optional)
  - mode=responses|chat (optional; default from env; for /v1/responses non-stream calls, “chat” will translate the payload into Chat Completions form)

Example:
CHAT2RESPONSE_BACKENDS="gpt-4o,base=https://api.openai.com/v1,mode=responses;local-,base=http://localhost:8000/v1,key_env=LOCAL_API_KEY,mode=chat"


## System Prompt Injection

If a system prompt config is loaded, Chat2Response can inject system prompts:
- For /v1/responses: injects a {"role":"system","content":"..."} message into messages based on injection_mode: prepend (default), append, or replace.
- For /v1/chat/completions: injects a system message by re-serializing the chat payload.

Configuration is hot-reloadable (see Reload endpoints).


# Endpoints

The service registers these routes:

- GET /status
- POST /convert
- POST /v1/chat/completions
- POST /v1/responses
- GET /keys
- POST /keys/generate
- POST /keys/revoke
- POST /keys/set_expiration
- POST /reload/mcp
- POST /reload/system_prompt
- POST /reload/all
- GET /analytics/stats
- GET /analytics/events
- GET /analytics/aggregate
- GET /analytics/export
- POST /analytics/clear


## GET /status

Returns runtime status, discovered routes, and feature flags.

Auth: None

Response:
- name, version
- routes: list of available routes
- features.mcp: {enabled, config_path, reloadable}
- features.system_prompt: {enabled, config_path, reloadable}
- features.analytics: {enabled, stats?}

Example:
```
curl -s http://localhost:PORT/status | jq
```

Example response:
```json
{
  "name": "chat2response",
  "version": "x.y.z",
  "proxy_enabled": true,
  "routes": ["/status", "/convert", "/v1/chat/completions", "/v1/responses", "..."],
  "features": {
    "mcp": { "enabled": true, "config_path": "mcp.json", "reloadable": true },
    "system_prompt": { "enabled": true, "config_path": "system_prompt.json", "reloadable": true },
    "analytics": { "enabled": true, "stats": { "total_events": 123, "...": "..." } }
  }
}
```


## POST /convert

Converts a Chat Completions request into an OpenAI Responses API payload. No network call is performed.

Auth: None

Query parameters:
- conversation_id (optional): If provided, used in conversion to make the call stateful for the Responses API.

Body:
- A valid Chat Completions JSON request.

Response:
- Converted Responses-shaped JSON.

Example:
```
curl -s -X POST "http://localhost:PORT/convert?conversation_id=abc123" \
  -H "Content-Type: application/json" \
  -d '{
    "model":"gpt-4o-mini",
    "messages":[{"role":"user","content":"Hello"}]
  }' | jq
```


## POST /v1/chat/completions

Pass-through for native Chat Completions requests. Optionally injects system prompts.

Auth:
- Managed mode: Authorization: Bearer sk_<id>.<secret> (validated; upstream API key supplied by server).
- Passthrough mode: Authorization: Bearer <provider_api_key> (forwarded upstream).

Body:
- Standard Chat Completions JSON.
- stream (bool, optional): When true, the proxy streams Server-Sent Events.

Streaming:
- Set "stream": true
- Optionally set Accept: text/event-stream
- The proxy streams upstream tokens/events back to the client.

Example (managed mode):
```
curl -N -X POST http://localhost:PORT/v1/chat/completions \
  -H "Authorization: Bearer sk_abc.def" \
  -H "Content-Type: application/json" \
  -d '{
    "model":"gpt-4o-mini",
    "stream": true,
    "messages":[{"role":"user","content":"Tell me a joke"}]
  }'
```

Example (passthrough mode):
```
curl -s -X POST http://localhost:PORT/v1/chat/completions \
  -H "Authorization: Bearer $OPENAI_API_KEY" \
  -H "Content-Type: application/json" \
  -d '{
    "model":"gpt-4o-mini",
    "messages":[{"role":"user","content":"Explain HTTP/2"}]
  }'
```


## POST /v1/responses

Pass-through for native OpenAI Responses API requests. Optionally injects system prompts. Supports multi-backend routing, and when the configured backend mode=chat for a matched model prefix, non-stream requests are translated to Chat Completions upstream.

Auth:
- Managed mode: Authorization: Bearer sk_<id>.<secret>
- Passthrough mode: Authorization: Bearer <provider_api_key>

Body:
- Standard Responses API payload (e.g., model, input/messages, tools, conversation, stream, etc.)

Streaming:
- Set "stream": true
- The proxy uses SSE to stream upstream events.

Example (non-stream):
```
curl -s -X POST http://localhost:PORT/v1/responses \
  -H "Authorization: Bearer sk_abc.def" \
  -H "Content-Type: application/json" \
  -d '{
    "model":"gpt-4o",
    "input":[{"role":"user","content":"Summarize this in bullet points"}],
    "stream": false
  }' | jq
```

Example (streaming):
```
curl -N -X POST http://localhost:PORT/v1/responses \
  -H "Authorization: Bearer sk_abc.def" \
  -H "Content-Type: application/json" \
  -H "Accept: text/event-stream" \
  -d '{
    "model":"gpt-4o",
    "input":[{"role":"user","content":"Write a short poem"}],
    "stream": true
  }'
```


## Keys – API key management (Managed Mode)

These endpoints manage internal access tokens that clients use in managed mode. There is no separate admin auth here; deploy behind a trusted network boundary or enforce ACLs at your reverse proxy.

Shared types (typical):
- GeneratedKey: { id, token, created_at, expires_at?, label?, scopes? }
- ApiKeyInfo: { id, label?, created_at, expires_at?, revoked_at?, scopes? }

Environment variables:
- CHAT2RESPONSE_KEYS_REQUIRE_EXPIRATION: "1|true|yes|on" to require expiration when generating.
- CHAT2RESPONSE_KEYS_DEFAULT_TTL_SECONDS: default TTL in seconds when not provided in request.

### GET /keys

Lists known keys (no tokens/secrets, only metadata).

Auth: None (protect via network ACL)

Response: Array<ApiKeyInfo>

Example:
```
curl -s http://localhost:PORT/keys | jq
```

### POST /keys/generate

Creates a new client access key.

Auth: None (protect via network ACL)

Body:
- label (string, optional)
- ttl_seconds (u64, optional)
- expires_at (unix seconds, optional; takes precedence over ttl_seconds)
- scopes (array<string>, optional)

Responses:
- 200 OK + GeneratedKey on success
- 400 if expiration is required by policy and not provided

Example:
```
curl -s -X POST http://localhost:PORT/keys/generate \
  -H "Content-Type: application/json" \
  -d '{
    "label":"demo",
    "ttl_seconds": 86400,
    "scopes": ["inference"]
  }' | jq
```

### POST /keys/revoke

Revokes a key by id.

Auth: None (protect via network ACL)

Body:
- id (string) – the key id to revoke

Response:
- {"revoked": true|false, "id": "<id>"}

Example:
```
curl -s -X POST http://localhost:PORT/keys/revoke \
  -H "Content-Type: application/json" \
  -d '{"id":"<key-id>"}' | jq
```

### POST /keys/set_expiration

Sets or clears expiration for a key.

Auth: None (protect via network ACL)

Body:
- id (string)
- expires_at (unix seconds, optional)
- ttl_seconds (u64, optional) – if provided, new expiration = now + ttl_seconds
  - Precedence: expires_at > ttl_seconds. If neither is present, clears expiration.

Response:
- {"updated": true|false, "id": "<id>", "expires_at": <unix|null>}

Example:
```
curl -s -X POST http://localhost:PORT/keys/set_expiration \
  -H "Content-Type: application/json" \
  -d '{"id":"<key-id>", "ttl_seconds": 604800}' | jq
```


## Reload – Runtime configuration reloads

### POST /reload/mcp

Reloads the MCP configuration file and reconnects servers.

Auth: None (protect via network ACL)

Prerequisite: The server must have been started with an MCP config path.

Response (success):
```json
{
  "success": true,
  "message": "MCP configuration reloaded",
  "servers": [{"name":"...","status":"..."}],
  "count": 2
}
```

### POST /reload/system_prompt

Reloads system prompt configuration.

Auth: None (protect via network ACL)

Prerequisite: The server must have been started with a system prompt config path.

Response (success):
```json
{
  "success": true,
  "message": "System prompt configuration reloaded",
  "enabled": true,
  "has_global": true,
  "per_model_count": 2,
  "per_api_count": 2,
  "injection_mode": "prepend"
}
```

### POST /reload/all

Reloads both MCP and system prompt configurations (when configured).

Auth: None (protect via network ACL)

Response (example):
```json
{
  "mcp": {
    "success": true,
    "message": "MCP configuration reloaded",
    "servers": [],
    "count": 0
  },
  "system_prompt": {
    "success": true,
    "message": "System prompt configuration reloaded",
    "enabled": true,
    "has_global": true,
    "per_model_count": 1,
    "per_api_count": 2,
    "injection_mode": "prepend"
  }
}
```


## Analytics

If analytics initializes successfully from the environment, these endpoints are enabled.

Notes:
- Time parameters are Unix seconds.
- Defaults:
  - events/aggregate default to the last 1 hour if not specified
  - export defaults to last 24 hours and "json" format

### GET /analytics/stats

High-level analytics stats.

Auth: None (protect via network ACL)

Response: Stats JSON (implementation-defined, includes totals, limits, etc.)

Example:
```
curl -s http://localhost:PORT/analytics/stats | jq
```

### GET /analytics/events

Query raw events in a time range.

Auth: None (protect via network ACL)

Query parameters:
- start (u64, optional) – default now - 3600
- end (u64, optional) – default now
- limit (u64, optional) – maximum number of events

Response:
```json
{
  "events": [ /* ... */ ],
  "count": 42,
  "start": 1730000000,
  "end": 1730003600
}
```

Example:
```
curl -s "http://localhost:PORT/analytics/events?start=1730000000&end=1730007200&limit=100" | jq
```

### GET /analytics/aggregate

Aggregated metrics over a time range.

Auth: None (protect via network ACL)

Query parameters:
- start (u64, optional) – default now - 3600
- end (u64, optional) – default now

Response: Aggregates JSON (counts, token totals, duration averages, cost totals, model breakdowns, etc.)

Example:
```
curl -s "http://localhost:PORT/analytics/aggregate?start=1730000000&end=1730007200" | jq
```

### GET /analytics/export

Export events for a time range.

Auth: None (protect via network ACL)

Query parameters:
- start (u64, optional) – default now - 86400
- end (u64, optional) – default now
- format (string, optional) – "json" (default) or "csv"

Responses:
- JSON: application/json attachment
- CSV: text/csv attachment with header row

Examples:
```
curl -s -OJ "http://localhost:PORT/analytics/export?format=json"
curl -s -OJ "http://localhost:PORT/analytics/export?format=csv&start=1730000000&end=1730086400"
```

### POST /analytics/clear

Clears all analytics data.

Auth: None (protect via network ACL)

Response:
```json
{ "success": true, "message": "Analytics data cleared" }
```

Example:
```
curl -s -X POST http://localhost:PORT/analytics/clear | jq
```


# Practical Examples

## Convert only (no upstream call)
```
curl -s -X POST "http://localhost:PORT/convert" \
  -H "Content-Type: application/json" \
  -d '{
    "model": "gpt-4o-mini",
    "messages": [{"role":"user", "content":"Summarize HTTP/1.1 vs HTTP/2"}]
  }' | jq
```

## Responses API with state and streaming
```
curl -N -X POST http://localhost:PORT/v1/responses \
  -H "Authorization: Bearer sk_abc.def" \
  -H "Content-Type: application/json" \
  -H "Accept: text/event-stream" \
  -d '{
    "model": "gpt-4o",
    "conversation": {"id":"conv_123"},
    "input": [{"role":"user","content":"Stream me a limerick about routers"}],
    "stream": true
  }'
```

## Chat Completions passthrough with system prompt injection
```
curl -s -X POST http://localhost:PORT/v1/chat/completions \
  -H "Authorization: Bearer sk_abc.def" \
  -H "Content-Type: application/json" \
  -d '{
    "model": "gpt-4o-mini",
    "messages": [{"role":"user","content":"Write a haiku about latencies"}]
  }' | jq
```


# Status Codes

- 200 OK – Success
- 400 Bad Request – Invalid input (e.g., malformed JSON, invalid parameters)
- 401 Unauthorized – Missing/invalid/revoked/expired token
- 502 Bad Gateway – Upstream error or connectivity issue
- 503 Service Unavailable – Dependent component unavailable (e.g., key manager or analytics disabled)


# Environment Variables (selected)

- OPENAI_API_KEY – Enables managed mode; used as default upstream key if not overridden by routing.
- CHAT2RESPONSE_BACKENDS – Multi-backend routing config; see “Routing and Multi-backend”.
- CHAT2RESPONSE_KEYS_REQUIRE_EXPIRATION – Require expiration when generating keys ("1|true|yes|on").
- CHAT2RESPONSE_KEYS_DEFAULT_TTL_SECONDS – Default TTL for key generation.
- CHAT2RESPONSE_PRICING_CONFIG – Optional pricing JSON file to enable cost tracking aligned with your provider list.

Notes:
- In managed mode, an Authorization bearer is mandatory and is validated; the upstream provider key is selected by routing (key_env if configured, else OPENAI_API_KEY).
- In passthrough mode, the client must send a valid upstream provider key as the bearer.


# Compatibility

- Works with OpenAI native endpoints.
- For local backends (vLLM, Ollama, etc.) that expose Chat Completions only, set mode=chat for their model prefixes; non-stream Responses POSTs get translated upstream automatically.


# Change Log

See README.md and release notes for additions to endpoints and behavior.
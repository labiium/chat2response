# Routiium Router Usage Guide

The router layer lets Routiium resolve human-friendly model aliases into concrete upstream endpoints and policies. This guide explains how routing decisions are made, which configuration hooks are available, and how to wire everything up in Docker.

---

## 1. How Routing Works

1. Every inbound `/v1/responses`, `/v1/chat/completions`, or `/convert` call flows through `resolve_upstream` (`src/server.rs`).  
2. If a `RouterClient` is configured (either via `--router-config` or `ROUTIIUM_ROUTER_URL`), Routiium builds a `RouteRequest` from the payload using `extract_route_request` (`src/router_client.rs`). That request includes:
   - The alias the client asked for (`body.model`).
   - API surface (`responses` or `chat`).
   - Capability flags (text, tools, vision, etc.).
   - Temperature/JSON mode hints and rough token estimates.
   - Optional conversation signals whose detail level is controlled by `ROUTIIUM_ROUTER_PRIVACY_MODE`.
3. The router returns a `RoutePlan` describing the target upstream (`base_url`, `mode`, `model_id`, optional `auth_env`, headers, limits, cache TTL, policy revision, stickiness token, etc.).  
4. Routiium forwards the request upstream using that plan, adds observability headers (e.g. `x-route-id`, `x-resolved-model`, `router-schema`), and submits router feedback when supported.
5. If the router rejects the alias or is unreachable and `ROUTIIUM_ROUTER_STRICT` is **not** set, Routiium falls back to the legacy `ROUTIIUM_BACKENDS` prefix rules or the global `OPENAI_BASE_URL`.

The Router contract is documented in detail in [`ROUTER_API_SPEC.md`](ROUTER_API_SPEC.md); `examples/router_service.rs` is a runnable reference implementation.

---

## 2. Router Modes

| Mode | How to enable | When to use |
| ---- | ------------- | ----------- |
| **Local alias map** | `routiium --router-config=router_aliases.json` | Simple deployments where a static JSON map is sufficient. |
| **Remote HTTP router** | Set `ROUTIIUM_ROUTER_URL=https://router.yourdomain/` (optional `ROUTIIUM_ROUTER_TIMEOUT_MS`, `ROUTIIUM_ROUTER_MTLS`, etc.) | Dynamic policies, catalog metadata, and multi-tenant routing. |
| **Legacy prefix fallback** | No router configured; set `ROUTIIUM_BACKENDS` | Emergency fallback or ultra-simple setups. |

`--router-config` takes precedence over `ROUTIIUM_ROUTER_URL`. When neither is specified, Routiium only uses `ROUTIIUM_BACKENDS` (if provided) or the global upstream.

---

## 3. Request Privacy Levels

`ROUTIIUM_ROUTER_PRIVACY_MODE` controls how much of the conversation is sent to the router:

| Value | Description |
| ----- | ----------- |
| `features` (default) | Sends metadata only (modalities, tool usage, token estimates). |
| `summary` | Adds a short summary of the latest user message. |
| `full` | Includes the system prompt and the last five turns so routers can enforce richer policies. |

The router’s `RoutePlan.content_used` field (and the `X-Content-Used` response header) records what the router actually consumed for auditing.

---

## 4. Plans, Caching, and Headers

- Each `RoutePlan` carries cache metadata (`cache.ttl_ms`, `cache.valid_until`, `cache.freeze_key`). Routiium also exposes `ROUTIIUM_CACHE_TTL_MS` to override the default 15 s cache horizon for remote routers.  
- Plans that include `stickiness.plan_token` cause Routiium to send that token back to the router on the next turn so multi-turn conversations stay on the same upstream.  
- Observability headers forwarded to clients:
  - `x-route-id`: Router-generated identifier (helps correlate downstream logs).
  - `x-resolved-model`: Actual upstream model ID.
  - `x-policy-rev` and `router-schema`: Policy metadata + schema version.
  - `x-content-used`: Privacy attestation from the router.
  - `x-route-cache`: `hit`, `miss`, or `stale` when the router exposed cache hints.
- When strict mode is disabled (default), failed router lookups fall back to `ROUTIIUM_BACKENDS`. Enabling `ROUTIIUM_ROUTER_STRICT=1` converts router errors into 502s so callers notice misconfigured aliases immediately.

---

## 5. Setup Recipes

### 5.1 Local Alias Map

1. Copy `router_aliases.json.example` to `router_aliases.json` and edit each alias block:
   ```jsonc
   {
     "edu-fast": {
       "base_url": "https://api.openai.com/v1",
       "mode": "responses",
       "model_id": "gpt-4o-mini-2024-07-18",
       "auth_env": "OPENAI_API_KEY"
     }
   }
   ```
   - `mode` must be `responses` or `chat`.
   - `auth_env` tells Routiium which environment variable holds the provider key.
2. Launch Routiium with `--router-config=/path/to/router_aliases.json`.  
3. Hit `/status` and confirm `router` shows `local policy`.  

> Local alias maps are static; restart Routiium after editing the JSON file.

### 5.2 Remote Router Service

1. Run or deploy a Router that follows `ROUTER_API_SPEC.md`. You can start the built-in example locally:
   ```bash
   cargo run --example router_service
   ```
   This serves `/route/plan`, `/route/feedback`, and `/catalog/models` on `http://127.0.0.1:9090`.
2. Point Routiium at it:
   ```bash
   ROUTIIUM_ROUTER_URL=http://127.0.0.1:9090 \
   ROUTIIUM_ROUTER_TIMEOUT_MS=50 \
   ROUTIIUM_CACHE_TTL_MS=60000 \
   routiium --system-prompt-config=system_prompt.json
   ```
3. Optional env knobs:
   - `ROUTIIUM_ROUTER_STRICT=1` – fail the request if the router rejects an alias.
   - `ROUTIIUM_ROUTER_MTLS=1` – enable mutual TLS (expect OS-level certs).
   - `ROUTIIUM_ROUTER_TIMEOUT_MS` – per-request timeout (ms).
   - `ROUTIIUM_CACHE_TTL_MS` – maximum cache TTL (ms) for remote plans.

Use the response headers or `/status` (`"router": { "mode": "remote", ... }`) to verify the connection. Router outages produce `WARN` logs; combine with strict mode to surface issues quickly.

---

## 6. Docker & Docker Compose

### 6.1 Local Alias Mode in Docker

1. Copy your alias file into the repo root (e.g. `router_aliases.json`).  
2. Mount it read-only and pass the flag via Compose:

```yaml
services:
  routiium:
    build: .
    env_file: .env
    command: ["--router-config=/app/router_aliases.json","--system-prompt-config=/app/system_prompt.json"]
    volumes:
      - routiium-data:/data
      - ./system_prompt.json:/app/system_prompt.json:ro
      - ./router_aliases.json:/app/router_aliases.json:ro
```

The container reads aliases at startup; restart it when you change the file.

### 6.2 Remote Router Mode in Docker

Add a router service (either your own implementation or the provided example) and point Routiium at it via env vars:

```yaml
services:
  router:
    build:
      context: .
      dockerfile: Dockerfile.router  # build your router image (example below)
    ports:
      - "9090:9090"

  routiium:
    build: .
    depends_on:
      - router
    env_file: .env
    environment:
      ROUTIIUM_ROUTER_URL: "http://router:9090"
      ROUTIIUM_ROUTER_TIMEOUT_MS: "50"
      ROUTIIUM_ROUTER_PRIVACY_MODE: "features"
      ROUTIIUM_ROUTER_STRICT: "1"
      ROUTIIUM_CACHE_TTL_MS: "60000"
    command: ["--system-prompt-config=/app/system_prompt.json"]
    volumes:
      - routiium-data:/data
      - ./system_prompt.json:/app/system_prompt.json:ro
```

To containerize the example router, you can reuse the Rust builder pattern:

```dockerfile
# Dockerfile.router
FROM rust:1.82-bookworm AS builder
WORKDIR /build
COPY Cargo.toml Cargo.lock ./
COPY src ./src
COPY examples ./examples
RUN cargo build --release --example router_service

FROM debian:bookworm-slim
WORKDIR /app
COPY --from=builder /build/target/release/examples/router_service /usr/local/bin/router_service
EXPOSE 9090
ENTRYPOINT ["router_service"]
```

Expose the router on the same Docker network so Routiium can reach `http://router:9090`.

---

## 7. Verification & Troubleshooting

- `curl http://localhost:8088/status | jq '.router'` – confirms whether Routiium is using a local or remote router, cache stats, and strict mode.  
- Inspect response headers from any `/v1/*` call (`x-route-id`, `router-schema`, `x-resolved-model`). Missing headers usually mean the fallback path was used.  
- Enable `ROUTIIUM_ROUTER_STRICT=1` in staging to catch typos early; disable it in production when you prefer graceful degradation to legacy routing.  
- Router logs should show `RouteRequest.alias` values that match `model` fields from clients; mismatches mean upstream clients are referencing unknown aliases.  
- If you see `Router plan unavailable… falling back to legacy routing` in logs, verify network reachability, schema compatibility, and policy revisions.

---

## 8. Reference: Key Router Environment Variables

| Env var | Default | Purpose |
| ------- | ------- | ------- |
| `ROUTIIUM_ROUTER_URL` | unset | Base URL for the remote Router API (`http(s)://...`). |
| `ROUTIIUM_ROUTER_TIMEOUT_MS` | `15` | HTTP timeout (ms) for `/route/plan` & `/catalog/models`. |
| `ROUTIIUM_ROUTER_PRIVACY_MODE` | `features` | Controls how much conversation content is sent to the router (`features`, `summary`, `full`). |
| `ROUTIIUM_ROUTER_STRICT` | unset | When truthy (`1`, `true`, `yes`, `on`), fail client requests if routing fails. |
| `ROUTIIUM_ROUTER_MTLS` | unset | Enable mutual TLS for router calls (certs must already exist on the host). |
| `ROUTIIUM_CACHE_TTL_MS` | `15000` | Cache horizon for router plans when using `HttpRouterClient`. |
| `ROUTIIUM_BACKENDS` | unset | Semicolon-separated fallback rules (`prefix=edu,base=https://...,key_env=OPENAI_API_KEY,mode=responses`). |

Keep provider keys (e.g., `OPENAI_API_KEY`, `ANTHROPIC_API_KEY`, `GROQ_API_KEY`) available in the environment so router plans referencing `auth_env` succeed.

---

With this configuration surface you can start with a static alias map, grow into a remote policy service, and still keep clear observability and fallback behaviour in Docker or bare-metal deployments.***

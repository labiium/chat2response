# Routiium Analytics System

## Overview

The analytics system provides comprehensive tracking and analysis capabilities for all API requests processed by routiium. It captures detailed metrics about requests, responses, performance, authentication, and routing.

## Architecture

### Components

1. **Analytics Module** (`src/analytics.rs`)
   - Core data models and storage backends
   - Support for Redis, Sled, and in-memory storage
   - Event recording, querying, and aggregation

2. **Analytics Middleware** (`src/analytics_middleware.rs`)
   - Request/response capture framework
   - Context propagation through request lifecycle
   - Automatic metric collection

3. **Analytics Endpoints** (`src/server.rs`)
   - REST API for querying and exporting analytics
   - JSON and CSV export formats
   - Real-time statistics

### Storage Backends

#### JSONL File (Default)
- Append-only JSONL log stored at `data/analytics.jsonl` by default
- Simple to inspect with tools like `jq` or import into external systems
- Parent directories are created automatically
- Data persists until you clear the file via the `/analytics/clear` endpoint or by removing the file

Configuration:
```bash
# Optional override
export ROUTIIUM_ANALYTICS_JSONL_PATH=/var/log/routiium/analytics.jsonl
```

#### Redis (Recommended for Production)
- Persistent storage with automatic expiration (TTL)
- Efficient time-based range queries using sorted sets
- Model and endpoint indexing for fast filtering
- Scales horizontally

Configuration:
```bash
export ROUTIIUM_ANALYTICS_REDIS_URL=redis://localhost:6379
export ROUTIIUM_ANALYTICS_TTL_SECONDS=2592000  # 30 days
```

#### Sled (Embedded Database)
- Single-file embedded database
- Good for single-server deployments
- No external dependencies
- Automatic persistence

Configuration:
```bash
export ROUTIIUM_ANALYTICS_SLED_PATH=./analytics.db
export ROUTIIUM_ANALYTICS_TTL_SECONDS=2592000
```

#### Memory (Development Only)
- Fast, in-memory storage
- Limited by available RAM
- Data lost on restart
- Automatic size limiting

Configuration:
```bash
export ROUTIIUM_ANALYTICS_FORCE_MEMORY=true
export ROUTIIUM_ANALYTICS_MAX_EVENTS=10000
```

## Data Model

### AnalyticsEvent

Each event captures a complete request/response cycle:

```rust
pub struct AnalyticsEvent {
    pub id: String,                    // Unique event UUID
    pub timestamp: u64,                // Unix timestamp (seconds)
    pub request: RequestMetadata,
    pub response: Option<ResponseMetadata>,
    pub performance: PerformanceMetrics,
    pub auth: AuthMetadata,
    pub routing: RoutingMetadata,
}
```

### RequestMetadata
- `endpoint`: API path (e.g., "/v1/chat/completions")
- `method`: HTTP method
- `model`: Model name requested
- `stream`: Whether streaming was requested
- `size_bytes`: Request payload size
- `message_count`: Number of messages in request
- `input_tokens`: Total input tokens (if available)
- `user_agent`: Client user agent
- `client_ip`: Client IP address (from X-Forwarded-For or connection)

### ResponseMetadata
- `status_code`: HTTP status code
- `size_bytes`: Response size
- `output_tokens`: Total output tokens (if available)
- `success`: Boolean success flag
- `error_message`: Error description if failed

### PerformanceMetrics
- `duration_ms`: Total request duration in milliseconds
- `ttfb_ms`: Time to first byte (for streaming)
- `upstream_duration_ms`: Upstream request time

### AuthMetadata
- `authenticated`: Authentication status
- `api_key_id`: Hashed API key identifier
- `api_key_label`: Human-readable label
- `auth_method`: Authentication method used

### RoutingMetadata
- `backend`: Backend provider (OpenAI, Anthropic, etc.)
- `upstream_mode`: "chat" or "responses"
- `mcp_enabled`: Whether MCP was used
- `mcp_servers`: List of MCP servers invoked
- `system_prompt_applied`: System prompt injection flag

## API Endpoints

### GET /analytics/stats

Returns current analytics system statistics.

**Response:**
```json
{
  "total_events": 1542,
  "backend_type": "redis",
  "ttl_seconds": 2592000,
  "max_events": null
}
```

### GET /analytics/events

Query individual events with optional time range and limit.

**Query Parameters:**
- `start` (optional): Start timestamp (unix seconds, default: now - 1 hour)
- `end` (optional): End timestamp (unix seconds, default: now)
- `limit` (optional): Maximum events to return

**Response:**
```json
{
  "events": [
    {
      "id": "550e8400-e29b-41d4-a716-446655440000",
      "timestamp": 1704067200,
      "request": {
        "endpoint": "/v1/chat/completions",
        "method": "POST",
        "model": "gpt-4o",
        "stream": false,
        "size_bytes": 256,
        "message_count": 3,
        "input_tokens": 42,
        "user_agent": "curl/7.64.1",
        "client_ip": "192.168.1.100"
      },
      "response": {
        "status_code": 200,
        "size_bytes": 512,
        "output_tokens": 128,
        "success": true,
        "error_message": null
      },
      "performance": {
        "duration_ms": 1247,
        "ttfb_ms": null,
        "upstream_duration_ms": 1200
      },
      "auth": {
        "authenticated": true,
        "api_key_id": "key_abc123",
        "api_key_label": "production-key",
        "auth_method": "bearer"
      },
      "routing": {
        "backend": "openai",
        "upstream_mode": "chat",
        "mcp_enabled": false,
        "mcp_servers": [],
        "system_prompt_applied": true
      }
    }
  ],
  "count": 1,
  "start": 1704067200,
  "end": 1704153600
}
```

### GET /analytics/aggregate

Get aggregated metrics over a time period.

**Query Parameters:**
- `start` (optional): Start timestamp (default: now - 1 hour)
- `end` (optional): End timestamp (default: now)

**Response:**
```json
{
  "total_requests": 1542,
  "successful_requests": 1523,
  "failed_requests": 19,
  "total_input_tokens": 45230,
  "total_output_tokens": 89441,
  "avg_duration_ms": 1247.3,
  "models_used": {
    "gpt-4o": 892,
    "gpt-4o-mini": 650
  },
  "endpoints_hit": {
    "/v1/chat/completions": 892,
    "/v1/responses": 650
  },
  "backends_used": {
    "openai": 1542
  },
  "period_start": 1704067200,
  "period_end": 1704153600
}
```

### GET /analytics/export

Export analytics data in JSON or CSV format.

**Query Parameters:**
- `start` (optional): Start timestamp (default: now - 24 hours)
- `end` (optional): End timestamp (default: now)
- `format` (optional): "json" or "csv" (default: "json")

**CSV Columns:**
- id
- timestamp
- endpoint
- method
- model
- stream
- status_code
- success
- duration_ms
- input_tokens
- output_tokens
- backend
- upstream_mode

**Response Headers:**
- `Content-Type`: application/json or text/csv
- `Content-Disposition`: attachment with filename

### POST /analytics/clear

Clear all analytics data from storage.

**Response:**
```json
{
  "success": true,
  "message": "Analytics data cleared"
}
```

## Use Cases

### Cost Tracking
Monitor token usage across models and time periods to estimate API costs:
```bash
curl "http://localhost:8088/analytics/aggregate?start=1704067200&end=1704153600" | \
  jq '.total_input_tokens, .total_output_tokens, .models_used'
```

### Performance Monitoring
Track request latency and identify slow endpoints:
```bash
curl "http://localhost:8088/analytics/aggregate" | jq '.avg_duration_ms'
```

### Usage Analytics
Understand which models and endpoints are most popular:
```bash
curl "http://localhost:8088/analytics/aggregate" | \
  jq '.models_used, .endpoints_hit'
```

### Error Analysis
Identify failed requests and error patterns:
```bash
curl "http://localhost:8088/analytics/events?limit=1000" | \
  jq '.events[] | select(.response.success == false)'
```

### Data Export for External Tools
Export to CSV for analysis in Excel, Tableau, or other tools:
```bash
curl "http://localhost:8088/analytics/export?format=csv&start=1704067200" -o analytics.csv
```

## Integration Examples

### Prometheus Metrics
You can poll the aggregate endpoint and convert to Prometheus format:
```python
import requests
import time

def get_metrics():
    now = int(time.time())
    hour_ago = now - 3600
    resp = requests.get(f"http://localhost:8088/analytics/aggregate?start={hour_ago}&end={now}")
    data = resp.json()
    
    print(f"# HELP routiium_requests_total Total requests")
    print(f"# TYPE routiium_requests_total counter")
    print(f"routiium_requests_total {data['total_requests']}")
    
    print(f"# HELP routiium_tokens_input_total Total input tokens")
    print(f"# TYPE routiium_tokens_input_total counter")
    print(f"routiium_tokens_input_total {data['total_input_tokens']}")
    
    print(f"# HELP routiium_tokens_output_total Total output tokens")
    print(f"# TYPE routiium_tokens_output_total counter")
    print(f"routiium_tokens_output_total {data['total_output_tokens']}")
```

### Grafana Dashboard
Create time-series visualizations by querying aggregated data at regular intervals.

### Cost Calculation
Calculate estimated costs based on token usage:
```python
import requests

PRICING = {
    "gpt-4o": {"input": 0.005, "output": 0.015},  # per 1K tokens
    "gpt-4o-mini": {"input": 0.00015, "output": 0.0006}
}

resp = requests.get("http://localhost:8088/analytics/aggregate")
data = resp.json()

total_cost = 0
for model, count in data["models_used"].items():
    if model in PRICING:
        # Estimate average tokens per request
        avg_in = data["total_input_tokens"] / data["total_requests"]
        avg_out = data["total_output_tokens"] / data["total_requests"]
        
        model_cost = (
            (avg_in / 1000 * PRICING[model]["input"]) +
            (avg_out / 1000 * PRICING[model]["output"])
        ) * count
        
        total_cost += model_cost
        print(f"{model}: ${model_cost:.4f}")

print(f"Total estimated cost: ${total_cost:.4f}")
```

## Best Practices

1. **Set Appropriate TTL**: Configure TTL based on your compliance and storage requirements
   - Development: 7 days (604800 seconds)
   - Production: 30-90 days (2592000-7776000 seconds)

2. **Use Redis for Production**: Redis provides the best performance and reliability for production workloads

3. **Regular Exports**: Set up scheduled exports for long-term archival:
   ```bash
   0 0 * * * curl "http://localhost:8088/analytics/export?format=csv" -o "/backups/analytics-$(date +\%Y-\%m-\%d).csv"
   ```

4. **Monitor Storage Usage**: Check analytics stats regularly to ensure storage isn't growing unbounded

5. **Filter Sensitive Data**: Analytics intentionally doesn't store message content or full API keys

## Privacy and Security

The analytics system is designed with privacy in mind:

- **No message content**: Only metadata is stored, never actual message text
- **Hashed API keys**: Only key IDs and labels are stored, not the actual keys
- **IP anonymization**: Consider using a reverse proxy to strip client IPs if needed
- **Automatic expiration**: TTL ensures data doesn't persist indefinitely
- **Clear endpoint**: Ability to delete all analytics data on demand

## Performance Considerations

- **Redis indexing**: Events are indexed by timestamp, model, and endpoint for fast queries
- **Batch exports**: Large exports may take time; use appropriate time ranges
- **Memory limits**: In memory mode, old events are automatically pruned
- **Async recording**: Analytics recording is non-blocking and won't slow down requests

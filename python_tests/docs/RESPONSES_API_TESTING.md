# Responses API Testing Guide

Comprehensive testing methodology for the `/v1/responses` endpoint in routiium.

## Overview

The `/v1/responses` endpoint is a critical feature that accepts chat-format requests and forwards them to OpenAI's Responses API. Unlike the `/v1/chat/completions` endpoint which proxies chat requests directly, the `/v1/responses` endpoint interfaces with a different OpenAI API that has distinct response structures and behaviors.

## Why Native SDK Testing is Preferred

The OpenAI Python SDK (`openai>=2.0.0`) **natively supports** the Responses API endpoint via `client.responses.create()`. This provides 1:1 implementation testing that matches real-world client usage.

```python
# ✅ CORRECT - Use native OpenAI SDK
response = client.responses.create(
    model="gpt-4o-mini",
    messages=[{"role": "user", "content": "Hello"}]
)

# Response has different structure than chat completions
print(response.output_text)  # Direct text output
print(response.output)        # Array of output items
print(response.usage.input_tokens)  # Token usage
```

## Architecture

```
┌────────────────────────────┐
│  Test Suite                │
│  (OpenAI SDK)              │
│  client.responses.create() │
└────────┬───────────────────┘
         │ HTTP: POST /v1/responses
         │ { "model": "...", "messages": [...] }
         ▼
┌─────────────────────────┐
│  routiium Server   │
│  Port 8099              │
└────────┬────────────────┘
         │ Forward to OpenAI
         │ Responses API
         ▼
┌─────────────────────────┐
│  OpenAI Responses API   │
│  (Backend)              │
└─────────────────────────┘
```

## Test Coverage

### 1. Basic Non-Streaming Request (`test_basic_responses_endpoint`)

**Purpose:** Validate core functionality of `/v1/responses` endpoint.

**What it tests:**
- HTTP 200 response status
- Response contains required fields: `id`, `object`, `choices`, `usage`
- Message structure: `role`, `content`
- Content is non-empty and valid
- Token usage statistics are present

**Complexity:**
- Time: O(n) where n = response size
- Space: O(n) for response storage

**Example:**
```python
response = client.responses.create(
    model="gpt-4o-mini",
    messages=[{"role": "user", "content": "Say hello"}],
)
assert response.id is not None
assert response.output_text is not None
assert len(response.output) > 0
```

### 2. System Message Handling (`test_responses_endpoint_with_system_message`)

**Purpose:** Verify system messages are properly forwarded.

**What it tests:**
- Multi-message conversations work
- System message influences response
- Message order is preserved

**Example:**
```python
response = client.responses.create(
    model="gpt-4o-mini",
    messages=[
        {"role": "system", "content": "You are a math tutor."},
        {"role": "user", "content": "What is 5 + 3?"}
    ],
)
assert response.output_text is not None
```

### 3. Streaming Mode (`test_responses_endpoint_streaming`)

**Purpose:** Validate Server-Sent Events (SSE) streaming.

**What it tests:**
- SSE format compliance
- Multiple chunks received
- Content deltas are properly structured
- Stream terminates with `[DONE]`
- Full content can be assembled from chunks

**SSE Format:**
```
data: {"id":"resp-123","choices":[{"delta":{"content":"Hello"}}]}

data: {"id":"resp-123","choices":[{"delta":{"content":" world"}}]}

data: [DONE]
```

**Complexity:**
- Time: O(n) where n = number of chunks
- Space: O(n) for chunk storage

### 4. Parameter Handling (`test_responses_endpoint_with_parameters`)

**Purpose:** Verify request parameters are forwarded correctly.

**Tested parameters:**
- `temperature`: Controls randomness (0.0-2.0)
- `max_tokens`: Limits response length
- `top_p`: Nucleus sampling parameter
- `user`: Custom user identifier

**Validation:**
- Parameters don't cause errors
- `max_tokens` is respected (with small buffer for tokenizer differences)
- Response completes successfully

### 5. Metadata Preservation (`test_responses_endpoint_metadata_preservation`)

**Purpose:** Ensure response metadata is complete.

**Verified fields:**
- `id`: Unique response identifier
- `model`: Model used for generation
- `created`: Timestamp (if present)
- `usage.prompt_tokens`: Input token count
- `usage.completion_tokens`: Output token count
- `usage.total_tokens`: Sum of input + output

### 6. Error Handling (`test_responses_endpoint_error_handling`)

**Purpose:** Validate proper error propagation.

**Tested scenarios:**
- Invalid model names
- Malformed requests
- Authentication failures

**Expected behavior:**
- HTTP 4xx/5xx status codes
- Informative error messages
- Errors from backend are properly forwarded

### 7. Edge Cases (`test_responses_endpoint_empty_message`)

**Purpose:** Test boundary conditions.

**Cases:**
- Minimal content (e.g., "Hi")
- Empty-like messages
- Unicode characters
- Very long messages

### 8. Performance (`test_responses_endpoint_latency`)

**Purpose:** Measure endpoint performance.

**Metrics:**
- End-to-end latency
- Should complete in < 30 seconds
- Baseline for performance regressions

**Complexity:**
- Time: O(1) - single request measurement
- Space: O(n) where n = response size

## Response Structure Validation

### Expected Responses API Format

**Native SDK Response Object:**
```python
response = client.responses.create(...)

# Response attributes:
response.id                    # "resp-1234567890"
response.object                # "response"
response.model                 # "gpt-4o-mini"
response.output_text           # "Hello! How can I help you today?"
response.output                # [OutputItem(...), ...]
response.usage.input_tokens    # 12
response.usage.output_tokens   # 9
```

**JSON Format (when serialized):**
```json
{
  "id": "resp-1234567890",
  "object": "response",
  "model": "gpt-4o-mini",
  "output_text": "Hello! How can I help you today?",
  "output": [
    {
      "type": "message",
      "content": "Hello! How can I help you today?"
    }
  ],
  "usage": {
    "input_tokens": 12,
    "output_tokens": 9
  }
}
```

### Streaming Format

**Native SDK Streaming:**
```python
stream = client.responses.create(
    model="gpt-4o-mini",
    messages=[{"role": "user", "content": "Hello"}],
    stream=True
)

for chunk in stream:
    if hasattr(chunk, 'output_text_delta') and chunk.output_text_delta:
        print(chunk.output_text_delta, end='', flush=True)
```

**SSE Wire Format:**
```
data: {"id":"resp-123","object":"response.chunk","output_text_delta":"Hello"}

data: {"id":"resp-123","object":"response.chunk","output_text_delta":"!"}

data: {"id":"resp-123","object":"response.chunk","output_text_delta":""}

data: [DONE]
```

## Running Responses API Tests

### Run All Responses API Tests

```bash
cd python_tests
./setup_and_test.sh  # Full setup + all tests

# Or just Responses API tests:
pytest tests/test_routiium_integration.py::TestResponsesAPI -v -s
```

### Run Specific Test

```bash
pytest tests/ -k test_basic_responses_endpoint -v -s
```

### Run with Debugging

```bash
pytest tests/test_routiium_integration.py::TestResponsesAPI -v -s --tb=long
```

### Run Only Non-Streaming Tests

```bash
pytest tests/ -k "TestResponsesAPI and not streaming" -v
```

## Common Issues and Solutions

### Issue 1: Connection Refused

**Symptom:**
```
openai.APIConnectionError: Connection refused
```

**Solution:**
```bash
# Ensure routiium server is running
curl http://127.0.0.1:8099/status

# If not running, start it
cd ..
cargo run --release
```

### Issue 2: 401 Unauthorized

**Symptom:**
```
openai.AuthenticationError: Invalid API key
```

**Solution:**
```bash
# Check .env file has valid API key
cat ../.env | grep OPENAI_API_KEY

# Test API key directly
curl https://api.openai.com/v1/models \
  -H "Authorization: Bearer $OPENAI_API_KEY"
```

### Issue 3: Streaming Timeout

**Symptom:**
```
openai.APITimeoutError: Request timed out
```

**Solution:**
```python
# Increase timeout when creating client
client = OpenAI(timeout=60.0)  # 60 seconds instead of default
```

### Issue 4: Invalid Response Structure

**Symptom:**
```
AttributeError: 'Response' object has no attribute 'output_text'
AssertionError: Response should have output_text
```

**Solution:**
- Verify OpenAI SDK version >= 2.0.0 with Responses API support
- Check if backend is actually OpenAI-compatible
- Verify `ROUTIIUM_BACKENDS` configuration
- Check server logs for upstream errors

## Advanced Testing Scenarios

### Testing with Different Models

```python
@pytest.mark.parametrize("model", [
    "gpt-4o-mini",
    "gpt-4o",
    "gpt-4-turbo-preview",
])
def test_responses_multiple_models(routiium_client, model):
    response = routiium_client.responses.create(
        model=model,
        messages=[{"role": "user", "content": "Hello"}],
    )
    assert response.output_text is not None
```

### Testing Multi-Backend Routing

If `ROUTIIUM_BACKENDS` is configured:

```bash
export ROUTIIUM_BACKENDS="gpt*=https://api.openai.com/v1,mode=responses,key=OPENAI_API_KEY"
```

```python
def test_responses_backend_routing(routiium_client):
    response = routiium_client.responses.create(
        model="gpt-4o-mini",
        messages=[{"role": "user", "content": "Test"}],
    )
    assert response.output_text is not None
```

### Testing with System Prompt Injection

If system prompts are configured:

```python
def test_responses_with_injected_system_prompt(routiium_client, test_model):
    """Test that system prompt injection doesn't break requests."""
    response = routiium_client.responses.create(
        model=test_model,
        messages=[{"role": "user", "content": "Hello"}],
    )
    assert response.output_text is not None
    # Verify injected prompt influenced response (if applicable)
```

## Performance Benchmarks

### Expected Latencies (gpt-4o-mini)

| Operation | Expected Time | Max Acceptable |
|-----------|---------------|----------------|
| Non-streaming request | 2-5 seconds | < 30 seconds |
| Time to first chunk | 500ms - 2s | < 10 seconds |
| Full streaming response | 3-8 seconds | < 30 seconds |

### Token Throughput

| Metric | Typical Value |
|--------|---------------|
| Tokens per second (streaming) | 30-60 TPS |
| Max tokens per request | 4096 (model dependent) |

## Integration with CI/CD

### GitHub Actions Example

```yaml
name: Responses API Tests

on: [push, pull_request]

jobs:
  test-responses-api:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v3
      - uses: dtolnay/rust-toolchain@stable

      - name: Build routiium
        run: cargo build --release

      - name: Start server
        run: ./target/release/routiium &
        env:
          OPENAI_API_KEY: ${{ secrets.OPENAI_API_KEY }}

      - name: Wait for server
        run: |
          for i in {1..30}; do
            curl -f http://127.0.0.1:8099/status && break
            sleep 1
          done

      - name: Run Responses API tests
        run: cd python_tests && ./run_tests.sh -k TestResponsesAPI
        env:
          OPENAI_API_KEY: ${{ secrets.OPENAI_API_KEY }}
```

## Debugging Tips

### Enable Verbose Logging

```bash
export RUST_LOG=debug
cargo run --release
```

### Capture HTTP Traffic

```python
import logging
import httpx

# Enable debug logging for OpenAI SDK
logging.basicConfig(level=logging.DEBUG)
logging.getLogger("openai").setLevel(logging.DEBUG)
logging.getLogger("httpx").setLevel(logging.DEBUG)
```

### Inspect Raw Responses

```python
# Access raw response if needed
response = client.with_raw_response.responses.create(
    model="gpt-4o-mini",
    messages=[{"role": "user", "content": "Hello"}]
)
print(f"Status: {response.http_response.status_code}")
print(f"Headers: {response.http_response.headers}")
print(f"Body: {response.http_response.text}")

# Get parsed response
parsed = response.parse()
print(f"Output: {parsed.output_text}")
```

### Test Against OpenAI Directly

Compare routiium behavior with direct OpenAI calls:

```python
# Direct to OpenAI (for comparison)
from openai import OpenAI

direct_client = OpenAI(
    api_key="your-openai-api-key",
    base_url="https://api.openai.com/v1"
)
response = direct_client.responses.create(
    model="gpt-4o-mini",
    messages=[{"role": "user", "content": "Hello"}]
)
```

## Maintenance and Updates

### When to Update Tests

- OpenAI changes Responses API format
- routiium adds new features to /v1/responses
- Performance requirements change
- New error cases discovered

### Test Stability

All tests should be:
- **Deterministic**: Same inputs → same validation logic
- **Isolated**: Each test independent
- **Fast**: Complete in < 30 seconds each
- **Reliable**: < 1% flake rate

### Code Coverage Goals

Target coverage for Responses API:
- Core functionality: 100%
- Error paths: 90%+
- Edge cases: 80%+

## Conclusion

Proper testing of the `/v1/responses` endpoint uses the native OpenAI Python SDK for 1:1 implementation testing. The test suite validates:

✅ Request forwarding to OpenAI Responses API  
✅ Response structure compliance  
✅ Streaming functionality  
✅ Error handling and propagation  
✅ Parameter forwarding  
✅ Performance characteristics  

This ensures routiium reliably proxies requests to the Responses API while maintaining the expected interface and behavior.
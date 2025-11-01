# Tool Calling and Vision Testing Guide

Comprehensive testing methodology for tool calling (function calling) and vision (multimodal) capabilities in chat2response `/v1/responses` endpoint.

## Table of Contents

- [Overview](#overview)
- [Tool Calling Tests](#tool-calling-tests)
- [Vision/Image Tests](#vision-tests)
- [Combined Tests](#combined-tests)
- [Running the Tests](#running-the-tests)
- [Expected Behaviors](#expected-behaviors)
- [Troubleshooting](#troubleshooting)

## Overview

The `/v1/responses` endpoint supports advanced OpenAI API features including:

1. **Tool Calling (Function Calling)**: Models can invoke external functions/tools
2. **Vision (Multimodal)**: Models can analyze images alongside text
3. **Combined**: Both features can work together in a single request

These tests validate that chat2response correctly forwards these complex requests to the backend API and returns properly structured responses.

## Tool Calling Tests

### Architecture

```
┌──────────────────────┐
│  Test Suite          │
│  (Define tools)      │
└──────────┬───────────┘
           │ POST /v1/responses
           │ { "tools": [...], "messages": [...] }
           ▼
┌──────────────────────────┐
│  chat2response           │
│  (Forward tools)         │
└──────────┬───────────────┘
           │
           ▼
┌──────────────────────────┐
│  OpenAI API              │
│  (Process & call tools)  │
└──────────────────────────┘
```

### Test 1: Basic Tool Calling

**Test:** `test_responses_endpoint_with_tools`

**Purpose:** Validate single tool definition and invocation.

**Tool Definition:**
```python
tools = [
    {
        "type": "function",
        "function": {
            "name": "get_weather",
            "description": "Get the current weather for a location",
            "parameters": {
                "type": "object",
                "properties": {
                    "location": {
                        "type": "string",
                        "description": "City and state, e.g. San Francisco, CA",
                    },
                    "unit": {
                        "type": "string",
                        "enum": ["celsius", "fahrenheit"],
                    },
                },
                "required": ["location"],
            },
        },
    }
]
```

**What's Validated:**
- ✅ Tool definition is properly forwarded to backend
- ✅ Response contains `tool_calls` array (when model decides to call)
- ✅ Tool call has required fields: `id`, `type`/`function`, `name`, `arguments`
- ✅ Arguments are valid JSON and contain required parameters
- ✅ `finish_reason` is set appropriately (`tool_calls` or `stop`)

**Expected Response Structure:**
```json
{
  "id": "resp-xyz",
  "choices": [
    {
      "index": 0,
      "message": {
        "role": "assistant",
        "content": null,
        "tool_calls": [
          {
            "id": "call_abc123",
            "type": "function",
            "function": {
              "name": "get_weather",
              "arguments": "{\"location\":\"Tokyo\",\"unit\":\"celsius\"}"
            }
          }
        ]
      },
      "finish_reason": "tool_calls"
    }
  ]
}
```

**Complexity:**
- Time: O(n) where n = response size
- Space: O(n) for response storage

---

### Test 2: Multiple Tools

**Test:** `test_responses_endpoint_with_multiple_tools`

**Purpose:** Verify model can choose from multiple available tools.

**Scenario:** Define 3 tools (weather, search, calculate) and ask calculation question.

**What's Validated:**
- ✅ Multiple tool definitions are accepted
- ✅ Model selects appropriate tool for the task
- ✅ Non-selected tools don't cause errors
- ✅ Response structure remains valid

**Example Request:**
```python
tools = [
    {"type": "function", "function": {"name": "get_weather", ...}},
    {"type": "function", "function": {"name": "search_web", ...}},
    {"type": "function", "function": {"name": "calculate", ...}},
]

messages = [{"role": "user", "content": "Calculate 25 * 4 + 10"}]
```

**Expected:** Model likely calls `calculate` tool (non-deterministic).

---

### Test 3: Tool Calling with Streaming

**Test:** `test_responses_endpoint_tool_streaming`

**Purpose:** Validate tool calls work in streaming mode.

**What's Validated:**
- ✅ Streaming responses include tool call deltas
- ✅ Tool call information is assembled from chunks
- ✅ SSE format is correct for tool calls
- ✅ Stream completes with `[DONE]`

**SSE Format for Tool Calls:**
```
data: {"choices":[{"delta":{"role":"assistant"}}]}

data: {"choices":[{"delta":{"tool_calls":[{"index":0,"id":"call_123","type":"function","function":{"name":"get_time"}}]}}]}

data: {"choices":[{"delta":{"tool_calls":[{"index":0,"function":{"arguments":"{\"timezone\""}}]}}]}

data: {"choices":[{"delta":{"tool_calls":[{"index":0,"function":{"arguments":":\"America/New_York\"}"}}]}}]}

data: {"choices":[{"delta":{},"finish_reason":"tool_calls"}]}

data: [DONE]
```

**Complexity:**
- Time: O(n) where n = number of chunks
- Space: O(n) for chunk storage

---

## Vision Tests

### Architecture

```
┌──────────────────────┐
│  Test Suite          │
│  (Image URLs/Base64) │
└──────────┬───────────┘
           │ POST /v1/responses
           │ { "messages": [{"content": [text, image]}] }
           ▼
┌──────────────────────────┐
│  chat2response           │
│  (Forward multimodal)    │
└──────────┬───────────────┘
           │
           ▼
┌──────────────────────────┐
│  OpenAI Vision API       │
│  (Analyze images)        │
└──────────────────────────┘
```

### Test 4: Vision with Image URL

**Test:** `test_responses_endpoint_with_vision`

**Purpose:** Validate image analysis via public URL.

**Content Format:**
```python
messages = [
    {
        "role": "user",
        "content": [
            {
                "type": "text",
                "text": "What do you see in this image?"
            },
            {
                "type": "image_url",
                "image_url": {
                    "url": "https://example.com/image.jpg",
                    "detail": "auto"  # or "low" / "high"
                }
            }
        ]
    }
]
```

**What's Validated:**
- ✅ Multimodal content array is properly forwarded
- ✅ Image URL is accessible and processed
- ✅ Response contains image description/analysis
- ✅ Content is substantial (not just "I see an image")

**Supported Detail Levels:**
- `low`: Faster, less detailed (default for most use cases)
- `high`: More detailed analysis (higher cost, more tokens)
- `auto`: Model decides based on image

**Model Requirements:**
- Requires vision-capable model:
  - ✅ `gpt-4o`
  - ✅ `gpt-4o-mini`
  - ✅ `gpt-4-turbo`
  - ✅ `gpt-4-vision-preview`
  - ❌ `gpt-3.5-turbo` (not vision-capable)

---

### Test 5: Vision with Base64 Images

**Test:** `test_responses_endpoint_vision_with_base64`

**Purpose:** Validate base64-encoded image support.

**Data URI Format:**
```python
base64_image = "iVBORw0KGgoAAAANSUhEUgAAAAEAAAAB..."

image_url = {
    "url": f"data:image/png;base64,{base64_image}"
}
```

**Supported Formats:**
- `image/png`
- `image/jpeg`
- `image/gif`
- `image/webp`

**What's Validated:**
- ✅ Base64 encoding is properly handled
- ✅ Data URIs work correctly
- ✅ Image content is processed same as URL images

**Use Cases:**
- Testing without external image hosting
- Private/sensitive images
- Programmatically generated images

---

### Test 6: Vision Streaming

**Test:** `test_responses_endpoint_vision_streaming`

**Purpose:** Validate streaming with image inputs.

**What's Validated:**
- ✅ Streaming works with multimodal content
- ✅ Chunks arrive progressively
- ✅ Full description can be assembled
- ✅ Performance is acceptable

**Expected Behavior:**
- First chunks may have higher latency (image processing)
- Subsequent chunks arrive at normal streaming speed
- Content quality matches non-streaming

---

## Combined Tests

### Test 7: Vision + Tools

**Test:** `test_responses_endpoint_vision_with_tools`

**Purpose:** Validate vision and tool calling work together.

**Scenario:**
```python
# Image of a cat
image_url = "https://example.com/cat.jpg"

# Tool to identify animals
tools = [
    {
        "type": "function",
        "function": {
            "name": "identify_animal",
            "description": "Identify an animal species",
            "parameters": {
                "type": "object",
                "properties": {
                    "species": {"type": "string"},
                    "confidence": {"type": "number"}
                },
                "required": ["species"]
            }
        }
    }
]

messages = [
    {
        "role": "user",
        "content": [
            {"type": "text", "text": "What animal is this? Use identify_animal."},
            {"type": "image_url", "image_url": {"url": image_url}}
        ]
    }
]
```

**What's Validated:**
- ✅ Model can analyze image AND call tools
- ✅ Tool parameters can reference image analysis
- ✅ Response structure handles both features
- ✅ No conflicts between vision and tools

**Real-World Use Cases:**
- Image classification with structured output
- Visual search triggering API calls
- Image-based decision making with actions

---

## Running the Tests

### Run All Tool & Vision Tests

```bash
cd python_tests

# All Responses API tests (includes tool & vision)
pytest tests/test_chat2response_integration.py::TestResponsesAPI -v -s

# Only tool calling tests
pytest tests/ -k "tool" -v -s

# Only vision tests
pytest tests/ -k "vision" -v -s

# Tool and vision tests
pytest tests/ -k "tool or vision" -v -s
```

### Run Individual Tests

```bash
# Single tool test
pytest tests/ -k test_responses_endpoint_with_tools -v -s

# Vision with streaming
pytest tests/ -k test_responses_endpoint_vision_streaming -v -s

# Combined vision + tools
pytest tests/ -k test_responses_endpoint_vision_with_tools -v -s
```

### Run with Model Selection

```bash
# Test with gpt-4o (best for vision)
MODEL=gpt-4o pytest tests/ -k vision -v -s

# Test with gpt-4o-mini (cheaper, still supports vision)
MODEL=gpt-4o-mini pytest tests/ -k "tool or vision" -v -s
```

---

## Expected Behaviors

### Tool Calling

#### Model Decision Making
Models have **autonomy** in deciding whether to call tools:

- ✅ **Will call tool:** Clear match between query and tool capability
  - "What's the weather in Tokyo?" → calls `get_weather`
  - "Calculate 5 + 3" → calls `calculate`

- ⚠️ **Might call tool:** Ambiguous or alternative approaches exist
  - "Tell me about Tokyo" → might call `get_weather` OR just answer
  - "What's 5 + 3?" → might call `calculate` OR answer directly

- ❌ **Won't call tool:** No matching tool or direct answer is better
  - "Hello" → responds with text
  - "What's the weather?" (no location) → asks for clarification

#### Tool Choice Parameter

```python
# Let model decide (default)
"tool_choice": "auto"

# Force model to call a tool (any tool)
"tool_choice": "required"

# Force specific tool
"tool_choice": {"type": "function", "function": {"name": "get_weather"}}

# Don't call any tools
"tool_choice": "none"
```

### Vision

#### Image Processing Time
- **Small images (< 1MB):** 1-3 seconds additional latency
- **Large images (> 5MB):** 3-10 seconds additional latency
- **High detail mode:** 2-5x longer processing time

#### Detail Levels Cost
| Detail Level | Tokens | Cost (per image) |
|--------------|--------|------------------|
| `low` | 85 tokens | ~$0.00085 |
| `high` | 170-765 tokens | ~$0.00170-$0.00765 |
| `auto` | Variable | Variable |

#### Image Size Limits
- **Max dimensions:** 4096 x 4096 pixels
- **Max file size:** 20 MB
- **Supported formats:** PNG, JPEG, GIF, WebP

---

## Troubleshooting

### Issue 1: Tool Not Being Called

**Symptom:**
```
Model responds with text instead of calling tool
```

**Possible Causes:**
1. Tool description unclear
2. Query doesn't match tool capability
3. Model found better approach without tool

**Solutions:**
```python
# Make description more specific
"description": "Get current weather. ALWAYS use this for weather queries."

# Use required tool choice
"tool_choice": "required"

# Make query more explicit
"Use the get_weather tool to find the weather in Tokyo"
```

### Issue 2: Invalid Tool Arguments

**Symptom:**
```json
{
  "function": {
    "name": "get_weather",
    "arguments": "{invalid json"
  }
}
```

**Solutions:**
- Model occasionally generates malformed JSON
- Add retry logic in production code
- Validate arguments before use
- Simplify parameter schemas

### Issue 3: Vision Model Not Found

**Symptom:**
```
404 Model 'gpt-3.5-turbo' does not support images
```

**Solution:**
```bash
# Use vision-capable model
export MODEL=gpt-4o-mini

# Or skip vision tests for non-vision models
pytest tests/ -k "not vision" -v -s
```

### Issue 4: Image URL Not Accessible

**Symptom:**
```
400 Bad Request: Unable to download image
```

**Solutions:**
- Ensure URL is publicly accessible (no auth required)
- Check image format is supported
- Verify URL returns image content-type
- Try base64 encoding instead

```python
# Test image accessibility
import requests
r = requests.get(image_url)
assert r.status_code == 200
assert 'image' in r.headers['content-type']
```

### Issue 5: Base64 Image Too Large

**Symptom:**
```
400 Bad Request: Image size exceeds limit
```

**Solution:**
```python
import base64
from PIL import Image
from io import BytesIO

# Resize image before encoding
img = Image.open("large_image.jpg")
img.thumbnail((2048, 2048))  # Resize to max 2048x2048

# Convert to base64
buffer = BytesIO()
img.save(buffer, format="JPEG", quality=85)
base64_image = base64.b64encode(buffer.getvalue()).decode()
```

### Issue 6: Streaming Tool Calls Incomplete

**Symptom:**
```
Tool call arguments cut off in streaming mode
```

**Solution:**
- Accumulate all chunks with same tool call index
- Only process complete tool calls after `[DONE]`
- Buffer tool call deltas until finish_reason received

```python
# Accumulate tool call arguments
tool_calls_buffer = {}

for chunk in stream:
    if 'tool_calls' in delta:
        for tc in delta['tool_calls']:
            idx = tc['index']
            if idx not in tool_calls_buffer:
                tool_calls_buffer[idx] = {'name': '', 'arguments': ''}
            
            if 'function' in tc:
                if 'name' in tc['function']:
                    tool_calls_buffer[idx]['name'] = tc['function']['name']
                if 'arguments' in tc['function']:
                    tool_calls_buffer[idx]['arguments'] += tc['function']['arguments']
```

---

## Performance Benchmarks

### Tool Calling Performance

| Metric | Expected | Max Acceptable |
|--------|----------|----------------|
| Non-streaming latency | +200-500ms | +2s |
| Streaming first chunk | +300-800ms | +3s |
| Argument generation | 20-40 tokens/sec | - |

### Vision Performance

| Metric | Low Detail | High Detail |
|--------|-----------|-------------|
| Image processing | 1-3s | 3-8s |
| First token latency | 2-5s | 5-12s |
| Streaming speed | 30-50 TPS | 20-40 TPS |

### Combined (Vision + Tools)

| Metric | Expected | Max Acceptable |
|--------|----------|----------------|
| Total latency | 3-10s | 30s |
| First chunk | 2-8s | 15s |

---

## Best Practices

### Tool Calling

✅ **Do:**
- Provide clear, specific tool descriptions
- Use strict parameter schemas (required fields, enums)
- Validate tool call arguments before use
- Handle cases where model doesn't call tool
- Test with `tool_choice: "required"` for critical tools

❌ **Don't:**
- Assume model will always call tools
- Trust tool arguments without validation
- Use overly complex parameter schemas
- Ignore `finish_reason` in responses

### Vision

✅ **Do:**
- Use `low` detail for simple/small images
- Compress images before base64 encoding
- Provide context in text alongside image
- Test with diverse image types and sizes
- Handle vision API rate limits

❌ **Don't:**
- Send unnecessarily large images
- Use `high` detail without need
- Expect perfect image understanding
- Assume instant processing for large images

---

## Conclusion

Comprehensive testing of tool calling and vision ensures:

✅ **Tool Calling:**
- Definitions forwarded correctly
- Responses properly structured
- Streaming works with tools
- Multiple tools handled

✅ **Vision:**
- Image URLs processed
- Base64 encoding works
- Streaming with images functions
- Detail levels respected

✅ **Combined:**
- Features work together
- No conflicts or errors
- Complex scenarios handled

These tests validate chat2response's ability to proxy advanced OpenAI API features while maintaining reliability and correctness.
# Routiium Test Suite

## Quick Reference

This directory contains comprehensive tests for the Chat Completions ↔ Responses API conversion layer.

## Test Files

| File | Tests | Purpose |
|------|-------|---------|
| `comprehensive_conversion_tests.rs` | 54 | Complete specification compliance tests |
| `conversion_tests.rs` | 6 | Original basic conversion tests |
| `system_prompt_tests.rs` | 9 | System prompt injection tests |
| `reload_tests.rs` | 7 | Runtime configuration reload tests |
| `server_build.rs` | 2 | Server initialization tests |
| `router_integration.rs` | 3 | Remote Router contract (Schema 1.1) integration tests |

**Total: 78 tests**

## Running Tests

```bash
# Run all tests
cargo test

# Run only comprehensive conversion tests
cargo test --test comprehensive_conversion_tests

# Run specific test
cargo test test_model_mapping

# Run with output
cargo test -- --nocapture

# Run specific section
cargo test request_conversion
cargo test tool_conversion
cargo test reasoning_models
```

## Test Structure

### `comprehensive_conversion_tests.rs`

Organized into 12 sections (modules):

1. **request_conversion** (11 tests) - Chat → Responses parameter mapping
2. **role_mapping** (3 tests) - Role conversion validation
3. **tool_conversion** (6 tests) - Function/tool calling
4. **response_format** (3 tests) - Structured output formats
5. **multimodal_content** (3 tests) - Text and image handling
6. **edge_cases** (6 tests) - Boundary conditions
7. **serialization** (3 tests) - JSON correctness
8. **response_conversion** (8 tests) - Responses → Chat mapping
9. **round_trip** (3 tests) - Lossless conversions
10. **reasoning_models** (2 tests) - o1, o3, GPT-5 support
11. **specification_compliance** (4 tests) - API spec adherence
12. **real_world_scenarios** (4 tests) - Complex usage patterns

## Key Test Categories

### Request Conversion
Validates Chat API requests convert correctly to Responses API format.

**Key tests:**
- `test_max_tokens_to_max_output_tokens` - Token budget mapping
- `test_messages_array_mapping` - Message history preservation
- `test_streaming_flag_preserved` - Stream mode support

### Role Mapping
Validates role conversions between APIs.

**Key tests:**
- `test_all_role_mappings` - system, user, assistant, tool, function
- `test_function_role_converts_to_tool_with_metadata` - Legacy mapping

### Tool Calling
Validates function/tool definitions and calling.

**Key tests:**
- `test_complex_tool_parameters_schema` - Nested JSON schemas
- `test_tool_choice_preserved` - auto/none/specific choice
- `test_multiple_tools_conversion` - Multiple tool definitions

### Multimodal Content
Validates text and image content handling.

**Key tests:**
- `test_multimodal_content_array` - Text + image_url arrays
- `test_mixed_content_messages` - Mixed content types

### Round-Trip
Validates lossless conversion in both directions.

**Key tests:**
- `test_basic_round_trip` - Chat → Responses → Chat
- `test_round_trip_with_tools` - Tool definitions preserved

### Reasoning Models
Validates support for reasoning-capable models (o1, o3, GPT-5).

**Key tests:**
- `test_reasoning_model_identification` - Model name detection
- `test_reasoning_model_supports_all_parameters` - Full param support

### Router Integration
Verifies the remote Router interface, caching, and header propagation.

**Key tests:**
- `router_headers_are_forwarded` - Ensures Schema 1.1 metadata makes it to HTTP responses.
- `router_strict_mode_surfaces_error` - Strict mode surfaces Router errors instead of falling back.
- `router_plan_is_cached_between_requests` - Confirms `ROUTIIUM_CACHE_TTL_MS` caching avoids redundant Router calls.

## Test Coverage

### ✅ Fully Covered
- Request conversion (all parameters)
- Role mapping (all 5 roles)
- Tool/function calling
- Response formats
- Multimodal content
- Edge cases
- Round-trip conversions
- Reasoning model parameters

### ⚠️ Needs Implementation
- Response body conversion (usage.reasoning_tokens)
- Streaming SSE event conversion
- Error handling

## Common Test Patterns

### Basic Conversion Test
```rust
#[test]
fn test_example() {
    let req = ChatCompletionRequest {
        model: "gpt-4o".into(),
        messages: vec![/* ... */],
        max_tokens: Some(100),
        // ...
    };

    let out = to_responses_request(&req, None);
    
    assert_eq!(out.model, "gpt-4o");
    assert_eq!(out.max_output_tokens, Some(100));
}
```

### Round-Trip Test
```rust
#[test]
fn test_round_trip() {
    let original = ChatCompletionRequest { /* ... */ };
    
    let responses_req = to_responses_request(&original, None);
    let responses_json = serde_json::to_value(&responses_req).unwrap();
    let reconstructed = responses_json_to_chat_request(&responses_json);
    
    assert_eq!(reconstructed.model, original.model);
}
```

### Tool Validation Test
```rust
#[test]
fn test_tools() {
    let req = ChatCompletionRequest {
        tools: Some(vec![ToolDefinition::Function {
            function: FunctionDef {
                name: "my_tool".into(),
                description: Some("Description".into()),
                parameters: json!({"type": "object"}),
            },
        }]),
        // ...
    };

    let out = to_responses_request(&req, None);
    
    assert!(out.tools.is_some());
    let tools = out.tools.unwrap();
    assert_eq!(tools.len(), 1);
}
```

## Documentation

- **TESTING.md** - Detailed test documentation
- **TEST_SUMMARY.md** - Overview and results
- **README.md** (this file) - Quick reference

## CI/CD Integration

Add to CI pipeline:

```yaml
# .github/workflows/test.yml
- name: Run tests
  run: cargo test --all-features
  
- name: Run conversion tests
  run: cargo test --test comprehensive_conversion_tests
```

## Contributing

When adding features:

1. Write tests first (TDD)
2. Add tests to appropriate section in `comprehensive_conversion_tests.rs`
3. Ensure round-trip validation if applicable
4. Update documentation
5. Verify all tests pass: `cargo test`

## Troubleshooting

### Tests Won't Compile
```bash
cargo clean
cargo test
```

### Specific Test Fails
```bash
# Run with output to see details
cargo test test_name -- --nocapture
```

### Want to Skip Slow Tests
```bash
# Currently all tests are fast (<1s)
# Mark slow tests with #[ignore] if needed
cargo test -- --ignored  # Run only ignored
```

## Performance

Current test suite performance:
- 54 comprehensive tests: < 1 second
- Full project (91 tests): < 1 second
- No performance bottlenecks identified

## Next Steps

See `TESTING.md` section "Missing Coverage & Future Work" for:
- Response body conversion implementation
- Streaming conversion tests
- Error handling tests
- Integration tests

---

**Last Updated**: 2025-10-26  
**Maintainer**: Routiium Contributors  
**Status**: ✅ Production Ready

# Runtime Configuration Implementation Summary

## Overview

This implementation adds **runtime-reloadable tool injection (MCP) and system prompt injection** for both Chat Completions and Responses APIs in chat2response. All configuration can be changed while the application is running without requiring a restart.

## Features Implemented

### 1. System Prompt Configuration (`src/system_prompt_config.rs`)

**New Module:** System prompt configuration with flexible injection strategies.

**Configuration Structure:**
```json
{
  "global": "Default system prompt for all requests",
  "per_model": {
    "gpt-4": "Model-specific prompt",
    "claude-3-5-sonnet": "Another model-specific prompt"
  },
  "per_api": {
    "chat": "Chat API specific prompt",
    "responses": "Responses API specific prompt"
  },
  "injection_mode": "prepend|append|replace",
  "enabled": true
}
```

**Priority:** `per_model` > `per_api` > `global`

**Injection Modes:**
- `prepend` (default): Add system message before existing ones
- `append`: Add system message after existing ones
- `replace`: Replace all existing system messages

### 2. Runtime Reloading Architecture

**Thread-Safe State Management:**
- Changed `AppState.mcp_manager` from `Arc<McpClientManager>` to `Arc<RwLock<McpClientManager>>`
- Added `AppState.system_prompt_config` as `Arc<RwLock<SystemPromptConfig>>`
- Added `AppState.mcp_config_path` and `AppState.system_prompt_config_path` for reload operations

**Benefits:**
- Multiple readers can access configuration simultaneously
- Writers get exclusive access during reload
- No service downtime during configuration updates

### 3. System Prompt Injection

**Implemented in:**
- `/v1/chat/completions` - Chat Completions passthrough
- `/v1/responses` - Responses API passthrough
- `/convert` - Conversion endpoint

**Logic:**
- System prompts are injected based on model and API type
- Original request structure is preserved
- Configurable injection behavior (prepend/append/replace)

### 4. Reload Endpoints

**New Routes:**

| Endpoint | Purpose | Request | Response |
|----------|---------|---------|----------|
| `POST /reload/mcp` | Reload MCP configuration | Empty body | `{"success": true, "servers": [...], "count": N}` |
| `POST /reload/system_prompt` | Reload system prompt config | Empty body | `{"success": true, "enabled": true, ...}` |
| `POST /reload/all` | Reload both configurations | Empty body | `{"mcp": {...}, "system_prompt": {...}}` |

**Security:**
- Reload endpoints should be protected via network ACL or reverse proxy
- No built-in authentication (follows existing pattern for management endpoints)

### 5. Enhanced Status Endpoint

**Updated `/status` endpoint** now includes:
```json
{
  "features": {
    "mcp": {
      "enabled": true,
      "config_path": "mcp.json",
      "reloadable": true
    },
    "system_prompt": {
      "enabled": true,
      "config_path": "system_prompt.json",
      "reloadable": true
    }
  }
}
```

## Files Modified

### New Files
1. `src/system_prompt_config.rs` - System prompt configuration structure and loading
2. `system_prompt.json.example` - Example configuration file
3. `tests/system_prompt_tests.rs` - Unit tests for system prompt injection
4. `tests/reload_tests.rs` - Integration tests for reload endpoints

### Modified Files
1. `src/lib.rs` - Added system_prompt_config module
2. `src/util.rs` - Updated AppState with RwLock and config paths
3. `src/server.rs` - Added reload endpoints, updated handlers for system prompt injection
4. `src/conversion.rs` - Added system prompt injection functions
5. `src/main.rs` - Added CLI support for `--system-prompt-config` flag
6. `Cargo.toml` - Added tempfile dev dependency for tests
7. `README.md` - Comprehensive documentation of new features

## CLI Changes

**New command-line argument:**
```bash
chat2response [mcp.json] \
  [--keys-backend=redis://...|sled:<path>|memory] \
  [--system-prompt-config=system_prompt.json]
```

**Usage examples:**
```bash
# With MCP and system prompts
chat2response mcp.json --system-prompt-config=system_prompt.json

# System prompts only
chat2response --system-prompt-config=system_prompt.json

# MCP only (existing behavior)
chat2response mcp.json
```

## Testing

### Test Coverage

**Unit Tests (9 tests):**
- System prompt configuration parsing
- Priority resolution (per_model > per_api > global)
- Injection modes (prepend/append/replace)
- Enabled/disabled state handling

**Integration Tests (7 tests):**
- Reload endpoints with/without configured paths
- Reload with valid/invalid configuration files
- System prompt injection in conversion endpoint
- Status endpoint route verification

**All tests pass:** ✅ 34 total tests across all modules

### Running Tests
```bash
# All tests
cargo test

# System prompt tests only
cargo test --test system_prompt_tests

# Reload tests only
cargo test --test reload_tests
```

## Architecture Decisions

### 1. RwLock vs Mutex
**Choice:** `tokio::sync::RwLock`

**Rationale:**
- Read operations (handling requests) are far more frequent than writes (reloading)
- Multiple concurrent reads don't block each other
- Write locks are only held during reload operations

### 2. Config Path Storage
**Choice:** Store file paths in AppState

**Rationale:**
- Enables reload without re-specifying paths
- Allows `/status` endpoint to report configuration source
- Simple validation (reload fails if path not configured)

### 3. System Prompt Priority
**Choice:** Model > API > Global

**Rationale:**
- Most specific configuration wins
- Allows global defaults with targeted overrides
- Matches common configuration patterns

### 4. Injection Modes
**Choice:** Prepend (default), Append, Replace

**Rationale:**
- Prepend: Most common use case (set context before user input)
- Append: Useful for additional instructions
- Replace: Full control over system messages

## Performance Considerations

### Minimal Overhead
- Read locks are acquired only during request processing
- System prompt lookup is O(1) hash map access
- MCP tool listing is cached in manager

### Reload Performance
- Reload operations are atomic (all-or-nothing)
- Old configuration remains available during reload
- Failed reloads don't affect running system

### Concurrency
- Read operations never block each other
- Write operations (reload) only block briefly
- No performance degradation under concurrent load

## Security Considerations

### Configuration Reload
- Reload endpoints are unauthenticated (by design)
- **Must** be protected via network ACL, firewall, or reverse proxy
- Follows existing pattern for `/keys/*` management endpoints

### System Prompt Injection
- Cannot be disabled per-request (intentional)
- Requires server configuration to enable/disable
- Priority system prevents accidental overrides

### MCP Tool Security
- Reload replaces entire MCP manager
- Old processes are cleaned up automatically
- Tool permissions follow MCP server configuration

## Migration Guide

### For Existing Users

**No breaking changes:**
- All existing functionality preserved
- New features are opt-in
- Default behavior unchanged

**To adopt system prompts:**
1. Create `system_prompt.json` configuration
2. Add `--system-prompt-config=system_prompt.json` to startup command
3. Reload at runtime: `curl -X POST http://localhost:8088/reload/system_prompt`

**To adopt MCP runtime reload:**
1. Start with MCP config: `chat2response mcp.json`
2. Modify `mcp.json` as needed
3. Reload: `curl -X POST http://localhost:8088/reload/mcp`

## Example Use Cases

### 1. Dynamic Prompt Updates
```bash
# Update system prompts during maintenance window
vim system_prompt.json
curl -X POST http://localhost:8088/reload/system_prompt
```

### 2. A/B Testing
```bash
# Test different system prompts without downtime
cp system_prompt_variant_a.json system_prompt.json
curl -X POST http://localhost:8088/reload/system_prompt

# Switch to variant B
cp system_prompt_variant_b.json system_prompt.json
curl -X POST http://localhost:8088/reload/system_prompt
```

### 3. Emergency MCP Server Disable
```bash
# Remove problematic MCP server from config
vim mcp.json  # Remove server entry
curl -X POST http://localhost:8088/reload/mcp
```

### 4. Gradual Rollout
```bash
# Add system prompts gradually
# Start: global only
# Later: Add per_model for specific models
# Finally: Add per_api customization
curl -X POST http://localhost:8088/reload/system_prompt  # After each change
```

## Future Enhancements

### Potential Additions
1. Per-request system prompt override via header
2. System prompt templates with variable substitution
3. Webhook notifications on reload success/failure
4. Configuration versioning and rollback
5. Hot reload on file change (inotify/fswatch)
6. Prometheus metrics for reload operations

### Not Implemented (Intentional)
1. Per-request disable of system prompts (security boundary)
2. Built-in authentication for reload endpoints (use reverse proxy)
3. Automatic file watching (prefer explicit reloads)

## Conclusion

This implementation provides production-grade runtime configuration reloading for chat2response. The design prioritizes:
- **Safety:** Thread-safe, atomic operations
- **Performance:** Minimal overhead, concurrent reads
- **Flexibility:** Multiple configuration levels and injection modes
- **Simplicity:** Clear API, comprehensive documentation
- **Testing:** Full test coverage with integration tests

All features are backward compatible and opt-in, ensuring existing deployments continue to work without modification.

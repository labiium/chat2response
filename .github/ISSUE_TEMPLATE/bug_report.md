---
name: Bug report
about: Create a report to help us improve Chat2Response
title: "[bug]: "
labels: bug
assignees: ""
---

<!-- Thank you for taking the time to report a bug! Please fill out as much as possible. -->

## Summary

A clear and concise description of the problem.

## Steps to Reproduce

1. What did you do?
2. What happened?
3. Minimal reproduction (CLI commands, code snippet, or repository link):

```bash
# Example
cargo clean
cargo build
cargo run
```

```rust
// Minimal code snippet, if applicable
```

## Expected Behavior

What did you expect to happen?

## Actual Behavior

What actually happened? Include exact error messages and stack traces (redact any secrets).

```
<logs/output here>
```

## Environment

- Chat2Response version (release tag or commit SHA): 
- Installation method (cargo, source, container, other): 
- OS/Arch (e.g., macOS 14.5, Ubuntu 22.04, Windows 11; x86_64/aarch64): 
- Rust toolchain (`rustc --version`): 
- Python version (for e2e tests, if relevant): 
- OpenAI client or tooling (if relevant): 

## Configuration

List relevant configuration and environment variables (redact secrets):

- BIND_ADDR=
- OPENAI_API_KEY=***redacted***
- OPENAI_BASE_URL=
- Other:

## Related Logs and Traces

Attach or paste relevant logs (trim to essential parts and redact secrets). If applicable, set `RUST_LOG=debug,tower_http=info` and retry to provide more context.

```
<logs here>
```

## Screenshots (optional)

If applicable, add screenshots to help explain the problem.

## Additional Context

Add any other context about the problem here (network setup, proxies, corporate TLS, etc.).

## Regression?

- Did this work in a previous version? If so, which version/commit?
- What changed (dependencies, OS, configuration)?

## Possible Solution (optional)

If you have ideas on how to fix it, share them here.

---

## Checklist

- [ ] I searched existing issues and discussions; this is not a duplicate.
- [ ] I can reproduce this with the latest `main` branch.
- [ ] I included minimal, reproducible steps and relevant logs (with secrets redacted).
- [ ] I confirmed the issue is not caused by local environment misconfiguration.
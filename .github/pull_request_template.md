# Pull Request Template

Thank you for contributing to Chat2Response! Please fill out the sections below to help us review your PR efficiently.

## Summary

- What does this PR change? Provide a concise summary.

## Related Issues

- Closes #<id> (if applicable)
- Related to #<id>

## Type of Change

- [ ] feat (new feature)
- [ ] fix (bug fix)
- [ ] refactor (no functional change)
- [ ] perf (performance improvement)
- [ ] docs (documentation only)
- [ ] test (add/update tests)
- [ ] chore (tooling, CI, deps)
- [ ] build (build system or external dependencies)

## Motivation and Context

- Why is this change needed? What problem does it solve?

## Design / Implementation Notes

- Outline design choices, trade-offs, and alternatives considered.
- Mention notable patterns, error handling, logging, or configuration decisions.

## API / Configuration Changes

- New/changed environment variables:
- New/changed endpoints, request/response shapes:
- Backward compatibility, deprecations, and migration guidance:

## How Has This Been Tested?

- Steps to reproduce and validate:
  - Build:
      cargo build --all-targets
  - Format check:
      cargo fmt --all -- --check
  - Lint (warnings as errors):
      cargo clippy --all-targets --all-features -- -D warnings
  - Unit/Integration tests:
      cargo test
  - End-to-end (optional, Python 3.9+):
      python -m pip install -r e2e/requirements.txt
      pytest -q e2e

- Include any logs or output snippets (redact secrets).

## Screenshots / Logs (optional)

- Add images or trimmed logs if they clarify the change.

## Breaking Changes

- [ ] Yes (describe impact and migration)
- [ ] No

If yes, describe exactly what breaks and how to migrate:

## Security / Privacy Considerations

- Any change to handling of secrets, tokens, or personally identifiable data?
- Confirm no secrets are logged and that `.env` files are not committed.

## Performance Impact

- Expected effect on latency, memory, allocations, or throughput.
- Include benchmarks or measurements if relevant.

## Documentation

- [ ] README updated
- [ ] Added/updated code comments
- [ ] Added/updated examples or usage notes
- [ ] Not applicable

## Release Notes

- Write a user-facing note for the next release (if applicable):

## Checklist

- [ ] Code compiles locally (debug and tests)
- [ ] `cargo fmt --all -- --check` passes
- [ ] `cargo clippy --all-targets --all-features -- -D warnings` passes
- [ ] `cargo test` passes
- [ ] End-to-end tests pass (if relevant): `pytest -q e2e`
- [ ] Tests added/updated to cover changes
- [ ] No secrets or sensitive data included
- [ ] `.env.example` updated if new env vars are introduced
- [ ] Dependencies justified and minimized (features scoped)
- [ ] License header/compatibility verified (Apache-2.0)

## Additional Context

- Anything else reviewers should know? Links to specs, API docs, prior art, or design docs.

---

By submitting this pull request, you agree that your contribution will be licensed under the Apache-2.0 license of this repository and that you will follow our Code of Conduct.
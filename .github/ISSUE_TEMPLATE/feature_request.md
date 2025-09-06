---
name: Feature request
about: Suggest an idea or improvement for Chat2Response
title: "[feat]: "
labels: enhancement
assignees: ""
---

<!-- Thank you for suggesting a feature! Please fill out as much as possible. -->

## Summary

A clear and concise description of the feature or improvement you’re proposing.

## Problem Statement

- What problem does this feature solve?
- Who is affected and how frequently?
- Why is the current behavior insufficient?

## Proposed Solution

Describe your proposed solution or approach in detail. Include configuration or API surface changes if relevant.

- High-level design:
- User experience / workflow:
- Example usage (CLI, HTTP call, or library):

```bash
# Example CLI or curl
curl -sS localhost:8088/convert -d '{ ... }'
```

```rust
// Example library usage
use chat2response::to_responses_request;
```

## Alternatives Considered

- Option A:
- Option B:
- Trade-offs and rationale:

## API / Config / CLI Changes

- New environment variables:
- New/changed endpoints or request/response shapes:
- Backward compatibility and migration plan:
- Deprecations (if any):

## Acceptance Criteria

List clear, testable criteria that define “done”:
- [ ] Criterion 1
- [ ] Criterion 2
- [ ] Criterion 3

## Impact and Risks

- Performance impact:
- Security/privacy considerations (ensure no secrets in logs, etc.):
- Operational concerns (observability, CORS, rate limiting, error handling):
- Documentation impact:

## Additional Context

Add any other context, links to related issues, or prior art.

- Related issues: #<id>, #<id>
- References / specs / API docs:

## Open Questions

- Question 1:
- Question 2:

---

## Checklist

- [ ] I searched existing issues and discussions; this is not a duplicate.
- [ ] I described the problem and why it matters.
- [ ] I provided a concrete proposal and acceptance criteria.
- [ ] I considered alternatives and trade-offs.
- [ ] I identified any API/config changes and their compatibility.
- [ ] I noted security, performance, and documentation impacts.
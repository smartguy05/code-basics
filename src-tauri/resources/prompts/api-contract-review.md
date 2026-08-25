---
title: API Contract & Compatibility Review
id: api-contract-review
---
Review the current diff for API contract and backward-compatibility breaks. Read-only — report, do not edit.

Look, in order of severity:
1. **Signature changes** — a public method or function whose parameters, return type, or nullability changed in a way that breaks existing callers.
2. **Shape changes** — a DTO, response body, or serialization key renamed, retyped, added as required, or removed from what clients already parse.
3. **HTTP surface changes** — a route path, verb, or status code altered so existing requests hit the wrong handler or get an unexpected result.
4. **Removed/renamed members** — a public type, member, or endpoint clients depend on deleted or renamed with no alias left behind.
5. **Missing version bump** — a breaking change that needs an API version bump or a compatibility shim to keep old clients working, and has neither.
6. **Rollback safety** — whether the change can be safely rolled back or feature-flagged, or whether it leaves persisted data or clients wedged if reverted.

For each finding: the file and line, one sentence on the break, and a concrete existing client call → the failure it now produces. Skip internal-only surfaces with no external consumer. If the diff introduces no contract or compatibility break, say so plainly rather than inventing findings.

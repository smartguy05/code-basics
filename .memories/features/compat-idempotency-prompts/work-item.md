# Feature: compat-idempotency-prompts

Add two agent-review prompt files to the enhancements/review prompt library. These are pure data files picked up by the enhancements prompt loader — no Rust change, no recompile, no id registry to update.

## Files added
1. `src-tauri/resources/prompts/api-contract-review.md` — title "API Contract & Compatibility Review", id `api-contract-review`. Reviews public signature changes, DTO/response/serialization shape changes, HTTP route/verb/status changes, removed/renamed public members, need for API version bump or compat shim, and rollback/feature-flag safety. Read-only.
2. `src-tauri/resources/prompts/idempotency-review.md` — title "Idempotency & Resilience Review", id `idempotency-review`. Reviews duplicate/redelivery safety (at-least-once), dedupe/idempotency key & outbox/inbox presence, partial-failure handling, retry safety and backoff, and non-idempotent side effects (double charge/send/insert). Read-only.

## Shape
Both mirror `src-tauri/resources/prompts/security-review.md` exactly: YAML front matter (`title`/`id`), one intro line ending "Read-only — report, do not edit.", a numbered severity-ordered checklist of bold lead-ins, and a closing output-format paragraph ending with the "say so plainly rather than inventing findings" abstain line.

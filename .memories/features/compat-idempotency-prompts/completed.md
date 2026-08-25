# Completed: compat-idempotency-prompts

## Files added (pure data, no recompile)
- `src-tauri/resources/prompts/api-contract-review.md` — API Contract & Compatibility Review prompt (id `api-contract-review`).
- `src-tauri/resources/prompts/idempotency-review.md` — Idempotency & Resilience Review prompt (id `idempotency-review`).

Both authored to match `security-review.md`'s shape exactly (front matter, one intro line with the "Read-only — report, do not edit." close, numbered severity-ordered bold-lead-in checklist, closing per-finding output-format paragraph ending on the abstain line).

No other files touched. No id registry exists to update; the enhancements/review prompt loader picks these up as data with no Rust change.

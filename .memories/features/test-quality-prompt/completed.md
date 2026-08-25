# Completed: Test Quality Review prompt

## Files touched
- `src-tauri/resources/prompts/test-quality-review.md` (new) — the review prompt.

## What was done
Read the existing `security-review.md` to match its shape exactly: YAML front matter
(title/id), a single "Read-only — report, do not edit." intro line, a numbered
severity-ordered checklist of **bold lead-in** — explanation items, and a closing
per-finding output paragraph ending "If the diff introduces no ... , say so plainly
rather than inventing findings."

Authored `test-quality-review.md` with id `test-quality-review`, title "Test Quality
Review". Checklist (severity order): (1) untested new/changed production logic,
(2) hollow assertions, (3) missing edge/boundary cases, (4) brittle coupling to
implementation detail, (5) tests weakened to pass.

## Notes
- No Rust/TS changed, so no typecheck/fmt gate applies. Prompt files are bundled via
  `bundle.resources` (see `sidecar/inspector` section of CLAUDE.md) and surface in the
  Enhancements → Run Agent menu via `cb_core::enhancements`.
- No other files edited per task constraint.

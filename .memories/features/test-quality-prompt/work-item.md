# Work Item: Test Quality Review prompt

## Goal
Add one agent-review prompt file that reviews the tests in the current diff, matching
the shape of `src-tauri/resources/prompts/security-review.md`.

## Details
- New file: `src-tauri/resources/prompts/test-quality-review.md`
- Front matter: title "Test Quality Review", id `test-quality-review`
- Read-only, report-only prompt (same shape as security-review.md).
- Numbered, severity-ordered checklist of bold lead-in — explanation items, then a
  per-finding output paragraph ending with the "say so plainly rather than inventing
  findings" line.

## Acceptance criteria
Checklist covers:
1. New/changed production logic with NO new/changed test exercising it.
2. Tests that assert nothing meaningful (only no-exception, or trivially-true assertion).
3. Missing edge/boundary cases (null/empty, zero, max/overflow, negative, concurrent access).
4. Tests coupled to incidental implementation detail rather than observable behavior.
5. A test weakened to pass (assertion loosened, case removed, or marked skip/ignore).

## Constraints
- Do not edit any other file except memory.

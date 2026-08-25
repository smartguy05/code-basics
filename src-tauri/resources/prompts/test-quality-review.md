---
title: Test Quality Review
id: test-quality-review
---
Review the tests in the current diff. Read-only — report, do not edit.

Look, in order of severity:
1. **Untested logic** — new or changed production code with no new or changed test exercising it.
2. **Hollow assertions** — a test that asserts nothing meaningful: only that no exception was thrown, or a trivially-true assertion that would pass regardless of the code under test.
3. **Missing boundaries** — an edge or boundary case left unexercised: null/empty, zero, max/overflow, negative, or concurrent access.
4. **Brittle coupling** — a test asserting on incidental implementation detail (internal call order, private shape, exact log text) rather than observable behavior.
5. **Weakened to pass** — a test loosened to go green: an assertion softened, a case removed, or a test marked skip/ignore.

For each finding: the file and line, one sentence on the gap, and the concrete input or change that would slip past the test as written. If the diff introduces no test-quality defect, say so plainly rather than inventing findings.

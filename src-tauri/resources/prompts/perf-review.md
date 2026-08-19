---
title: Performance Review
id: perf-review
---
Review the current diff for performance defects. Read-only — report, do not edit.

Look for:
1. **Needless allocation** — a clone, copy, or collection built only to be thrown away, or work repeated inside a loop that could be hoisted out.
2. **N+1 access** — a query or remote call issued once per item where one batched call would do.
3. **Quadratic loops** — a nested scan over the same data, or a linear lookup inside a loop that a set or map would make constant.
4. **Blocking in async** — synchronous I/O, a blocking wait, or a long CPU task on an async/executor thread.
5. **Missing pagination or index** — an unbounded fetch of a growing collection, or a lookup with no supporting index.

For each finding: the file and line, one sentence on the cost, and the concrete input scale at which it bites (e.g. "with N rows this runs N queries"). Prefer defects that grow with data over micro-optimizations. If the diff introduces no real performance regression, say so plainly rather than inventing findings.

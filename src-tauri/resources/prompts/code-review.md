---
title: Code Review
id: code-review
---
Review the current diff for correctness bugs and cleanup opportunities.

Focus, in order:
1. **Correctness** — logic errors, off-by-one, unhandled errors, race conditions, wrong edge-case behaviour. For each, give a concrete failing input → wrong output.
2. **Reuse & simplification** — duplicated logic, a helper that already exists, a simpler equivalent.
3. **Efficiency** — needless allocation, repeated work, an N+1 pattern.

Report the most important findings first. For each: the file and line, one sentence on the defect, and the concrete scenario that triggers it. Skip style nits. If nothing is wrong, say so plainly rather than inventing findings.

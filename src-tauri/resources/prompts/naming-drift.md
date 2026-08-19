---
title: Naming Drift
id: naming-drift
---
Review the current diff for names that no longer match what the code does. Read-only — report, do not edit.

Look for:
1. **Names that contradict behavior** — a function, variable, or field whose name promises one thing while the body now does another (a `get` that mutates, an `is_valid` that also saves, a `count` holding a sum).
2. **Terminology inconsistent with the surrounding code** — the diff introduces a new word for a concept the module already names something else, so two terms now mean one thing.
3. **Misleading identifiers introduced by the diff** — a unit, sign, or type embedded in a name that is now wrong (`_ms` holding seconds, `_bytes` holding a count), or a boolean whose name reads backwards from its meaning.

For each finding: the file and line, the name, one sentence on the mismatch between the name and the actual behavior, and the better-fitting name or the existing term it should align with. Judge against what the code does now, not what you would have called it. If the diff introduces no naming drift, say so plainly rather than inventing findings.

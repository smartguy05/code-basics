---
title: Verify Business Rules
id: verify-rules
---
Check the current diff against this codebase's recorded business rules. Read-only — report, do not edit.

1. Read every file under `.code-basics/rules/*.md`. Each states one business-rule invariant and where it is enforced. (The app may also prepend the loaded rules to this prompt as context — use those if present, but still read the directory to be sure you have them all.)
2. If the rules directory is empty or absent, say so plainly and suggest running **Extract Business Rules** first — then stop.
3. Otherwise, for each rule, examine the diff for any change that could violate the invariant: a removed or weakened guard, a new path that bypasses the check, an edit to the enforcing code that changes its meaning.

For each violation: cite the rule (its title/id), the file and line in the diff, and the concrete scenario in which the invariant now breaks (an input or state that would slip past). Do not flag a rule you cannot tie to a specific change in this diff. If no change in the diff violates any recorded rule, say so plainly rather than inventing findings.

---
title: Memory Files
id: memory
placement: after-first-heading
---
## CRITICAL: Memory Files

**ALWAYS update the per-work-item memory files when relevant.** Memory is scoped **per feature/bug** under `.memories/features/{feature-name}/` or `.memories/bugs/{bug-name}/`, not at the `.memories/` root. These files track work item state across sessions:

| File | Path | Purpose | When to Update |
|------|------|---------|----------------|
| `work-item.md` | `.memories/features/{feature-name}/work-item.md` or `.memories/bugs/{bug-name}/work-item.md` | The feature work item details, ACs, description | When loading or refreshing work item context |
| `plan.md` | `.memories/features/{feature-name}/plan.md` or `.memories/bugs/{bug-name}/plan.md` | Implementation plan for the feature or bug fix | When planning or revising the approach |
| `related-docs.md` | `.memories/features/{feature-name}/related-docs.md` or `.memories/bugs/{bug-name}/related-docs.md` | Pointers to relevant documentation | When discovering docs that inform the work |
| `notes.md` | `.memories/features/{feature-name}/notes.md` or `.memories/bugs/{bug-name}/notes.md` | Issues, gotchas, lessons learned **for this work item** | When debugging/solving something others might hit on this WI |
| `todos.md` | `.memories/features/{feature-name}/todos.md` or `.memories/bugs/{bug-name}/todos.md` | Remaining tasks and tech debt **for this work item** | When adding, completing, or deprioritizing tasks |
| `completed.md` | `.memories/features/{feature-name}/completed.md` or `.memories/bugs/{bug-name}/completed.md` | Completed work record (files touched, root cause, fix) | When finishing the work item (or a major phase of it) |

**Rules:**
1. Update these files **AT ALL TIMES** under the active work item folder — they are that work item's memory.
2. Update `completed.md` immediately after finishing a task (not at end of session).
3. Update `todos.md` to check off completed items and add new discovered tasks.
4. Update `notes.md` with any issue you debug/solve that others might hit.
5. Keep entries concise but descriptive — future you needs to understand.
6. Periodically prune `todos.md` to remove old completed items.
7. Periodically summarize and prune `completed.md` to keep the file size small.
8. **Cross-work-item patterns** (gotchas that recur across multiple work items) belong in `CLAUDE.md` (root or the relevant per-project `CLAUDE.md`), not in any single work item's `notes.md`.

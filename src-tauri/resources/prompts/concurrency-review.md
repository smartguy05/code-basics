---
title: Concurrency Review
id: concurrency-review
---
Review the current diff for concurrency defects. Read-only — report, do not edit.

Look for:
1. **Data races** — shared state read or written from more than one thread/task without a lock or atomic guarding it.
2. **Lock ordering / deadlock** — two locks acquired in different orders on different paths, or a re-entrant acquire of a non-re-entrant lock.
3. **Await holding a lock** — an `.await` (or any yield point) while a non-async guard is held, which can stall or deadlock the executor.
4. **Non-atomic check-then-act** — a check and the action it guards split so another thread can invalidate the check in between (TOCTOU).
5. **Cancellation / teardown** — a task, process, or handle that is not cancelled or joined on the drop/shutdown path, or a teardown that can hang.

For each finding: the file and line, one sentence on the hazard, and the concrete interleaving (which thread does what, in what order) that produces the wrong result or the hang. If the diff introduces no concurrency defect, say so plainly rather than inventing findings.

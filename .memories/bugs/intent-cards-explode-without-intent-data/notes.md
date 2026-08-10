# Notes / gotchas

- **The 133-card explosion was mostly a data problem, not a grouping bug.** With zero
  intent records, attribution abstains and pass 3 makes ~1 card per hunk. Check
  `.code-basics/intents/` exists and has edits.jsonl before blaming grouping.rs.
- **`edits.jsonl` missing while `labels.jsonl` grows** = the PostToolUse hook's edits are
  being dropped by `relative_to` (workspace mismatch — historically the `--workspace` pin),
  while `ingest_label` never checks paths. Orphaned labels in the wrong workspace are the
  telltale (ONEflight had 52).
- `import type { X }` / `pub use ...` contain DECLARING keywords (`type`, `pub`) — any
  import-shaped line must be rejected by word (NOT_A_SYMBOL) inside declaration_name, not
  just in the header fallback. Found via the ignored real-repo diagnostic AFTER the first
  fix landed; the diagnostic (`report_attribution_against_this_repository`) is the fastest
  way to see real-world label quality.
- Two parallel agents editing the same crate: the lib-test target won't compile while one
  is mid-signature-change — the grouping agent had to test in an isolated tree copy.
  Prefer sequencing agents that share a crate, or worktree isolation.
- One-off cb-core operations (like driving import_intent_history headlessly) work well as
  a temporary `#[ignore]` test in crates/core/tests/, run with `--ignored --nocapture`,
  then deleted.

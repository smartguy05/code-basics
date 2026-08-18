# Notes — behavioral-testing

## OPEN DESIGN QUESTION — building the baseline side (prepare.rs, next up)
`invocation::build(workspace, config, filter)` uses `workspace.root` + the full `Workspace`
(adapters read root + config paths). To build the HEAD side we need a `Workspace` rooted at
the **worktree path**, not the real root.
- `.code-basics/config.json` is COMMITTED, so the worktree checkout contains the same user
  configs → scanning the worktree reconstructs equivalent configs.
- Cleanest approach: scan the worktree as a fresh `Workspace` (workspace::scan or equiv),
  find the config by **id** in it. If the config isn't present in the baseline (e.g. an
  uncommitted working-tree-only config), **abstain** that comparison with a warning.
- Cost: scanning is filesystem-only (no build), acceptable. Verify the scan fn name/signature
  in `crates/core/src/workspace.rs` before writing prepare.rs.
- Alternative (rejected as fragile): clone the working Workspace and rebase root+project paths.

## Command layer can't be unit-tested (crate rule)
`behavioral_diff` takes `State<AppState>` → untestable. Keep it decision-free: extract any
decision (config lookup, side selection, abstain choices) into free functions in `prepare.rs`
/ `behavioral` and test THOSE. The `tee(rx) -> (channel, String)` helper beside `forward` in
commands/run.rs captures each side's stdout while streaming; that's plumbing, not a decision.

## Two runs need distinct supervisor ids
`Supervisor::run` REPLACES an entry when the id repeats. Use `"<config_id>:base"` /
`"<config_id>:work"` so the two sides don't clobber each other.

## HTTP phase must be sequential (port conflict)
Base app and work app bind the SAME port from the .http file. Run base up→ready→replay→down,
THEN work up→replay→down. Never concurrent. If config isn't serverful → abstain HTTP.

## git worktree gotchas (verified working)
- `git worktree add` creates the target dir itself; don't pre-create it (only its parent).
- A checkout kept as cache is a REAL registered worktree; a second create at the same oid
  adopts it (is_valid_worktree = `.git` gitlink file exists).
- Tests need `git` on PATH (Git Bash) — same constraint as the process:: tests.

## Confidence type
`crate::git::attribution::Confidence` (Low/Medium/High), derives Ord → `.min()` works for the
weakest-member rule. serde camelCase → "low"/"medium"/"high".

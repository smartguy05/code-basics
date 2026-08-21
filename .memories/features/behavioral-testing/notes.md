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

## "worktree add failed:" with EMPTY stderr == Windows STATUS_DLL_INIT_FAILED (2026-08-21)
Symptom in the field (ONEflight): Run before/after abstained with
`git worktree add for <oid> failed:` — a blank tail. Running the identical
`git worktree add --detach <dir> <oid>` by hand SUCCEEDS. Not a code bug.
Root cause: the spawned `git.exe` died *before initialising* and wrote nothing.
The sibling "Verify claims" run confirmed it — `claude -p ...` exited with
`-1073741502` == `0xC0000142` == STATUS_DLL_INIT_FAILED. Both children hit the
same OS condition: interactive-window-station desktop-heap exhaustion after long
uptime / many spawned processes (or an AV hook blocking CreateProcess).
Fix for the user: restart the app to free the desktop heap; reboot if it recurs.
Code fix: `worktree.rs::describe_process_failure` (pure, tested) now surfaces the
exit code when stderr is empty and names 0xC0000142 with the restart hint, so the
message is diagnosable instead of a bare "failed:". Other shell-outs (`git apply`
in git/repo.rs) still have the same swallow-empty-stderr gap if it ever recurs there.

## Run/report survives tab switch + live progress (2026-08-21)
Two field bugs on the before/after UI:
1. VISIBILITY — the run only showed one 11px faint line; the streamed stdout/stderr
   (`ProcessEvent {type:"output"}`) was ignored. Fix: capture output into a bounded
   `behavioralLog` (helper `appendRunLog` in behavioralPanelLogic.ts, tested) and render
   a `.behavioral-progress` area (prominent status + spinner + scrollable `.behavioral-log`,
   auto-scrolled). Only shown while `behavioralRunning || behavioralStatus`; else the hint.
2. LOST ON TAB SWITCH — ChangesView was `{tab==="changes" && <ChangesView/>}`, so leaving
   Changes UNMOUNTED it: the in-flight `behavioralDiff` promise resolved into a dead
   component (setState no-op) → report lost even after completion; returning showed nothing.
   Fix: App.tsx now keeps Changes mounted with `hidden={tab!=="changes"}` (like
   Run/Tests/Objects "own a running process, must survive a tab switch"). Added `active` prop;
   poll effect skips when `!active` (was scoped by unmount before), plus a re-read-on-activate
   effect to preserve the old "fresh on every visit" behaviour a remount gave for free.
Files: src/App.tsx, src/views/ChangesView.tsx, src/components/behavioralPanelLogic.ts(+test),
src/styles.css. NOTE: History + Architecture still remount on visit — only Changes changed.

## Before/after now runs in a floating window (2026-08-21, supersedes the sidebar-log note above)
User feedback: the sidebar run "showed more text then went away, no report." Root cause: the
report rendered as 11px muted text at the BOTTOM of IntentPanel (`.behavioral-overall`), below
10 cards — buried. Fix (their request: "run it in a window like verify claims"):
- NEW `src/components/BehavioralPanel.tsx` — floating window modelled on ReviewPanel, reuses
  `.review-panel`/`.review-header`/`.review-console`/`.review-pill` CSS + `reviewLayoutLogic`
  (drag/pos persistence) + `OutputConsole` (xterm, `handle(event)`). Runs `api.behavioralDiff`
  on mount, streams to the console, renders the full `BehavioralReport` below (`.behavioral-report`).
- Hosted at APP level (like `agentPanel`) → survives tab switches for free, so ChangesView
  reverted to conditional mount (removed the `active` prop / poll-gating / streamed-log state
  from the prior attempt; `appendRunLog` helper + its tests removed as dead).
- Both buttons now just call callbacks: `onRunBehavioral(configId)` / `onVerifyClaims(configId)`
  → App `openBehavioral(id, verify)`. When `verify`, the panel builds the evidence context and
  calls `onVerify` → existing `openVerifyClaims` opens the agent ReviewPanel. So verify = two
  windows (evidence + agent).
- App owns `behavioralReport`; panel reports it up via `onReport` → passed to ChangesView →
  IntentPanel for the per-card badges (unchanged).
Files: src/App.tsx, src/views/ChangesView.tsx, src/components/BehavioralPanel.tsx(new),
src/components/behavioralPanelLogic.ts(+test revert), src/styles.css.
NOTE: `behavioral_diff` has NO cancel command — closing the panel mid-run abandons the promise
(guarded by an `alive` ref); the Rust run finishes and its result is dropped. Fine for now.

## Value framing + moved out of prime sidebar space (2026-08-21)
User pushback: for a diff where tests pass on BOTH sides, before/after just says "old passes,
new passes" — redundant with running current tests. Honest value is only in DELTAS: test flips
attributed to your diff (separates your regression from pre-existing red), refactor invisibility
(identical HTTP/console = stronger than "tests pass"), HTTP replay. Least useful for a normal
green feature diff. Decision: keep the feature (mainly serves Verify claims + refactor/regression
cases) but demote the UI.
- Console noise fix: `console.rs` now masks durations (`mask_durations`, default true) — bracketed
  `[2 ms]`/`[< 1 ms]` and word `N Seconds/ms/...`. Was surfacing "console: 55 added/56 removed"
  as the sole delta on an identical-outcome run. Now → 0 deltas / "no observable difference".
  Tests in console_tests.rs. REBUILD NOTE: this is cb-core; the running dev app won't pick it up
  until a successful rebuild (dev watcher may hit the Access-denied exe lock while the app runs).
- Moved the two big always-visible buttons + hint into a compact "Before / after ▾" dropdown in
  the intent header next to "Review" (`.evidence-menu`, reuses `.dropdown-menu`/`.dropdown-item`,
  new `.dropdown-item.disabled`). Removed `.behavioral-run/.behavioral-actions/.behavioral-hint`
  CSS (now unused). Files: src/views/ChangesView.tsx, src/styles.css.

## Confidence type
`crate::git::attribution::Confidence` (Low/Medium/High), derives Ord → `.min()` works for the
weakest-member rule. serde camelCase → "low"/"medium"/"high".

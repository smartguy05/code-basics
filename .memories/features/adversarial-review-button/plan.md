# Plan — panel size persistence, capture-side intent attribution, rustfmt cleanup

> Approved plan (session 2026-08-19). Execute in a FRESH session via a workflow.
> All file:line anchors below are the map — do not re-scout. Branch:
> `claude/intent-scope-and-diff-fixes` (Phases 1–3 already committed:
> `8aeddf7`, `a956967`, `e280179`, `bc21cd0`).

## Context

Three follow-ups remain after the review-tooling roadmap:

1. **Persist the panel size.** The floating agent/review panel already persists
   its dragged *position* (`cb.agentPanel.layout`), but a native-grip resize is
   lost on reopen — flagged a "deferred nicety" in the code.
2. **Capture-side intent attribution.** Edits by a **subagent** (Task tool) get
   geometry but no reason: installed hooks are only `PostToolUse`+`Stop`,
   `HookEvent` rejects `SubagentStop`, and the transcript miner skips
   `isSidechain`. Do **both** halves: live-hook `SubagentStop` capture **and**
   retroactive sidechain mining.
3. **Fix the rustfmt drift.** `cargo fmt --check` flags ~29 files of pre-existing
   drift from earlier branch commits (behavioral/*, git durable-why, intents/*,
   erosion compact-rule style). A standalone `cargo fmt` cleanup makes it clean.

**Ordering:** rustfmt cleanup FIRST (standalone commit on the currently-clean
tree) so feature diffs are against a formatted base; then panel-size and
capture-side (independent — frontend vs Rust).

Governing rule: **a wrong result is worse than none** — abstain, never guess.

---

## 1. rustfmt cleanup (do first, standalone commit)

- `cargo fmt` (whole workspace) → `cargo fmt --check` clean. Reformats the
  pre-existing drift (behavioral/*, `git/*`, `intents/providers/*`, erosion
  compact `r(...)` rules) to canonical rustfmt — consistent with CLAUDE.md's
  "formatted wholesale, keep it that way".
- Sanity gate: `cargo build` + `cargo test -p cb-core` (fmt changes no behavior;
  the lone `process::cancel` flake passes isolated).
- Commit alone as `cargo fmt`. The one place we deliberately touch the
  pre-existing-drift files every prior task left alone.

## 2. Panel size persistence (frontend only)

Files: `src/components/reviewLayoutLogic.ts` (+ `.test.ts`),
`src/components/ReviewPanel.tsx`. (`src/styles.css` `.review-panel` already has
`resize: both` + min/max — no change; persisted size seeds the box the grip
then adjusts.)

- **`reviewLayoutLogic.ts`** — extend layout + add a size clamp (mirror
  `clampPanelPosition`/`PanelSize`):
  - `PanelLayout` gains optional `width?`/`height?` (drop the "size intentionally
    not persisted" comment). `PanelSize` already exists — reuse it.
  - `loadPanelLayout`/`savePanelLayout`: carry `width`/`height` through the same
    `typeof === "number"` guards.
  - New pure `clampPanelSize(size, viewport)` → clamp to `[MIN, viewport*maxFactor]`
    honoring the CSS floor (min-width 360 / min-height 280) and 96vw/92vh ceiling.
    Pure + tested.
- **`ReviewPanel.tsx`** — seed size from layout, apply inline; capture native-grip
  resizes via a `ResizeObserver` (grip fires NO pointer event, so `onUp` can't
  see it):
  - Seed a `size` state from `loadPanelLayout` (only when both width+height
    present, else CSS default); add `width`/`height` to the inline style object.
  - `ResizeObserver` on `panelRef` (mirror `DiffView.tsx:561-568`: `typeof
    ResizeObserver` guard → `observe` → `disconnect` on cleanup); read
    `offsetWidth/offsetHeight`, clamp via `clampPanelSize`, persist **debounced**
    (observer fires continuously during a drag-resize) by merging `{width,height}`
    into `cb.agentPanel.layout` alongside left/top. Guard so the initial default
    size isn't persisted before a real user resize.
- **Tests** (`reviewLayoutLogic.test.ts`, mirror its `fakeStorage` style): size
  round-trips; string width/height dropped; oversized clamps to viewport;
  below-floor clamps up to min.

## 3. Capture-side intent attribution (Rust)

### 3a. Live-hook `SubagentStop` — small, additive, abstain-safe

Files: `crates/core/src/intents/providers/hooks_json.rs`,
`crates/core/src/intents/hook.rs` (+ `hook_tests.rs`). No `recorder.rs` change
(flows through `parse_recorder_args`→`HookEvent::parse`→`ingest`).

- `hooks_json.rs`: add `"SubagentStop"` to `EVENTS` (line 42) — propagates to
  install/detect/uninstall (all iterate `EVENTS`); matcher `""` like `Stop`;
  `command_line` writes `--event SubagentStop`.
- `hook.rs`: add `HookEvent::SubagentStop`; `parse` accepts it; `ingest` routes
  it to **`ingest_label`** (like `Stop` — reads `last_assistant_message`, keys on
  `turn_id`). **Do NOT route through `ask_for_intent`** — keep the `event != Stop`
  guard so a subagent stop never blocks/exit-2 (Claude Code honoring a
  `SubagentStop` refusal is unverified; blocking a subagent could hang it).
- **Why no turn-id-match needed:** the label joins to the subagent's edits via
  the **cross-turn binder already shipped** (`git/coverage.rs`; see
  `.memories/bugs/workflow-intent-attribution`): a *declared, path-scoped*
  `Intent(paths):` label binds to matching-path geometry regardless of turn id.
  Worst case (no `last_assistant_message`) it's a silent no-op.
- Tests: move `SubagentStop` out of the reject list in
  `only_the_two_installed_events_are_recognised`; add a positive `parse` case +
  an `ingest`-routes-to-label test; update the "two events" doc wording.

### 3b. Retroactive sidechain mining — the substantial piece

File: `crates/core/src/intents/providers/claude_code.rs` (+ `claude_code_tests.rs`).

`read_transcript` (270–398) is a linear pass with one `block` counter and SKIPS
`isSidechain` (310–312). Parallel subagents **interleave** their sidechain lines
in one file, so contiguity can't separate them — group on **`parentUuid`
lineage**:

- Read `uuid`/`parentUuid` per line (not read today). First pass: build
  `uuid → parentUuid` (and `uuid → isSidechain`).
- For a sidechain entry, resolve its **subagent-root uuid** by walking
  `parentUuid` up to the topmost sidechain ancestor (child of the first
  non-sidechain entry — the Task tool_use). Cache per uuid. An unresolvable chain
  abstains (skipped, not mis-grouped).
- Keep per-subagent-root state (recent prose, `block`, `labelled_block`,
  `prompted_block`) in a `HashMap<rootUuid, …>`, mirroring the main-session
  logic; emit turn ids `claude-history-{session}-sub-{root}-{block}` so parallel
  subagents never share a turn. Main-session grouping unchanged.
- Labels stay `LabelSource::Inferred` (mined prose), keyed to the subagent turn —
  complementing 3a's declared labels.
- `TOOLING_WORDS` caveat (`hook.rs` includes "subagent"/"agent"): an inferred
  first sentence naming the subagent is refused by `looks_like_narration` —
  acceptable (declared labels are the target).
- Tests: **invert** `sidechain_entries_are_skipped` → a sidechain edit+prose is
  mined and joined; **two interleaved subagents** get separate turns (key case);
  an unresolvable-parent entry abstains; main-session path unchanged (regression).

## Critical files

- `src/components/reviewLayoutLogic.ts` (+ test), `src/components/ReviewPanel.tsx`
- `crates/core/src/intents/providers/hooks_json.rs`, `crates/core/src/intents/hook.rs` (+ `hook_tests.rs`)
- `crates/core/src/intents/providers/claude_code.rs` (+ `claude_code_tests.rs`)

## Reused, not rebuilt

- `PanelSize` + `clampPanelPosition` pattern (`reviewLayoutLogic.ts`);
  `ResizeObserver` house style at `DiffView.tsx:561-568`.
- `EVENTS`-drives-everything in `hooks_json.rs`; `ingest_label` + `turn_id` in
  `hook.rs`; the **cross-turn binder** (`git/coverage.rs`, shipped) for the join.
- The main-session prose→`block`→edits grouping in `read_transcript` as the
  template for per-subagent grouping.

## Verification

1. rustfmt: `cargo fmt --check` clean; `cargo build`; `cargo test -p cb-core`.
2. Panel size: `pnpm test` (new size tests) + `pnpm typecheck`; manual
   `pnpm tauri dev` — resize via the grip, close/reopen → returns at the resized
   size; oversized-then-reopen clamps on-screen.
3. Capture-side: `cargo test -p cb-core intent` + miner tests (interleaved
   subagents, invert sidechain-skip, unresolvable-parent abstain); full
   `cargo test -p cb-core`; `cargo clippy -p cb-core` clean for new code; rustfmt
   changed leaf files (`--config skip_children=true` for a mod-root).
   **Manual empirical (tests can't cover):** with capture enabled, run a Task
   subagent that edits and ends with `Intent(paths): …`, inspect
   `.code-basics/intents/` — confirm a `SubagentStop` label was recorded and the
   Changes→Intent view attributes the subagent's hunks to it. If the live payload
   lacks `last_assistant_message`, 3a is a no-op and 3b (mining) is the working
   path; report that finding rather than forcing it.
4. `pnpm docs:index` + `pnpm docs:check` if any public cb-core API changed (new
   `HookEvent` variant is internal; likely only INDEX regen).

## Execution via workflows (FRESH session)

Context is cleared before implementation — everything needed is in this file.

**Step 0 — rustfmt cleanup, done DIRECTLY (not a workflow; one command).** On the
clean tree: `cargo fmt` → `cargo fmt --check` clean → `cargo build` +
`cargo test -p cb-core` → commit alone as `cargo fmt`. Then feature diffs are
against a formatted base.

**Then one workflow — `subagent-capture-and-panel-size`:**

- **Implement (3 agents in parallel — file-disjoint):**
  1. `panel-size` — section 2 (frontend, tests-first vitest).
  2. `subagent-hook` — section 3a (`hooks_json.rs`, `hook.rs`, `hook_tests.rs`).
  3. `sidechain-miner` — section 3b (`claude_code.rs`, `claude_code_tests.rs`);
     interleaved-parallel-subagents is the key test.
- **Verify (1 agent):** full gate — `cargo test -p cb-core` (documented flake),
  `cargo clippy -p cb-core` (only pre-existing `rider.rs` warning allowed),
  targeted `rustfmt --check` on changed leaf files (`--config skip_children=true`
  for a mod-root), `pnpm test`, `pnpm typecheck`, `pnpm docs:index` + `docs:check`.
- **Review (2 agents in parallel):** (a) correctness/IPC — does the miner's
  `parentUuid` root-resolution separate interleaved subagents, never mis-group,
  abstain on an unresolvable chain? does panel-size clamp+debounce correctly?
  (b) faithfulness — is `SubagentStop` routed to `ingest_label` only (never
  `ask_for_intent`)? does the inverted sidechain test assert the new behavior?
  were any pre-existing tests deleted?

Every agent prompt: **tests-first**, shared `target/` (no private
`CARGO_TARGET_DIR`), **never** workspace-wide `cargo fmt` (only `rustfmt` the
leaf files you changed — beware rustfmt module-recursion churning children of a
mod-root; revert such collateral), match surrounding style, abstain on
uncertainty, edit only your slice's files.

**After the workflow:** verify on disk directly (don't trust agent reports),
apply review findings, keep the diff focused, commit in three focused commits
(fmt cleanup [Step 0]; panel size; subagent capture), then update this work-item
memory (`todos.md`/`completed.md`).

## Commit hygiene note (for PR)
The branch carries pre-existing rustfmt drift from earlier commits — Step 0's
`cargo fmt` cleanup addresses it. Nothing has been pushed; no PR opened.

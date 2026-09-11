# Completed — UI + browser bug batch

Reported together (2026-09-09). Plan: `~/.claude/plans/there-are-a-few-snuggly-shamir.md`.

## Bug 2 — Ctrl+N dead while a file editor is focused ✅
`search.all` (and `search.symbols`/`search.files`/`search.actions`) had no
`allowInText`, so `dispatchShortcut` skipped them when the caret was in
`.cm-editor`. Added `allowInText: true` to all four in `src/shortcutLogic.ts`.

## Bug 3 — Ctrl+F did nothing in a file editor ✅
`console.find` binds Ctrl+F globally and `dispatchShortcut` always
`preventDefault`s a matched chord, so CodeMirror's `searchKeymap` never saw it.
Fix (multi-handler pattern, like `run.run`):
- `FileEditor.tsx` registers a `console.find` handler that calls
  `openSearchPanel(view)` when the caret is inside *this* editor's frame
  (`wrapRef.contains(activeElement)`), else returns false. Imported
  `openSearchPanel` from `@codemirror/search`.
- `OutputConsole.tsx` `console.find` now returns false when
  `document.activeElement?.closest(".cm-editor")` — so exactly one acts.

## Bug 4 — Opened diffs showed no tab (no pin/close) ✅
The tab strip lived inside `<div className="content console-area" hidden={isDiff}>`
in `RunView.tsx`, so opening a diff hid all tabs. Moved the pinned/unpinned
`console-tabs` strip to the top of `.main` (above the `{isDiff ? DiffPane : …}`
branch), guarded by `openFiles.length > 0`. Pin/close already worked for diff
sources. `.console-tabs` is `flex:0 0 auto` and `.main` is a flex column, so no
CSS change was needed.

## Bug 1 — Codex made a background tab blink forever ✅
A backgrounded terminal's sticky bell-attention + `infinite` flash animation +
Codex's constant bells = perpetual blink. Turned the tab flash into a bounded,
self-settling pulse:
- `workspaceTabsLogic.ts`: added `ATTENTION_FLASH_MS` (4000), `AttentionPulses`,
  `pulseAttention` (idempotent while flashing/settled), `attentionActive`,
  `acknowledgeAttention`, `nextPulseExpiry`. Tests in `workspaceTabsLogic.test.ts`.
- `App.tsx`: `attentionPulses`/`pulseNow` state; effect arms pulses on the rising
  edge of `attentionByRoot` and clears on the falling edge; a single timer bumps
  `pulseNow` at the soonest expiry; tab render uses `attentionActive(...)` instead
  of raw `attentionByRoot[root]`; `activateWorkspace`/`closeWorkspace` acknowledge.
  Pill latch in `TerminalPanel` left unchanged (per user).

## Bug 5 — Start the language server on project/folder open ✅
`spawn_session` was already called on open but `session::start` only *resolved*
servers; the process started lazily on first `didOpen`/request. Now eager for
present languages:
- `crates/core/src/lsp/registry.rs`: `Language::from_ecosystem` (dotnet→CSharp,
  node→TypeScript, cargo→Rust; others None) + `languages_present(&[Project])`
  (dedup, `Language` order). Tests in `registry_tests.rs`.
- `session.rs`: `start`/`start_with_probe` gained a `warm: &[Language]` param;
  after `resolve_all` and before spawning `run`, `ensure_started` each warmed
  language. Tests in `tests/lsp_session.rs` (`warming_*`). Updated all callers
  (oracle, session tests, state_tests, commands/lsp.rs) — non-warming pass `&[]`.
- `commands/lsp.rs` `ensure_session_interleaved`: computes
  `languages_present(&workspace.projects)` and passes it.
- Python stays lazy (no scan ecosystem) — documented, no regression.

## Verified
- `pnpm typecheck` clean; `pnpm test workspaceTabsLogic` 38 pass.
- `cargo test -p cb-core registry::` pass; `cargo test -p cb-core --test lsp_session warming` 2 pass.
- `cargo check --workspace --all-targets` clean; `cargo fmt` applied.

## Bugs 6+7 — Browser per-workspace + stacking ✅ (via workflow, gate re-verified by hand)
Implemented by the `browser-per-workspace` workflow (Opus). Summary:
- `state.rs`: `browser` field → `Mutex<HashMap<PathBuf, Arc<Mutex<BrowserShared>>>>`;
  new `browser_shared`, `active_browser_shared` (the security choke point — only
  ever the active root's data, never another's), keyed `browser(app, root)` +
  `active_browser(app)`, per-root `browser_data`/`_mut`, `active_root_pathbuf`;
  `close()` drops the root's slot.
- `browser/mod.rs`: `HOST` → `HashMap<PathBuf, Host>`; `BrowserHandle` carries
  `root`; `on_main` closure is `(app, shared, map, root)`; new `visible_for`
  free fn gates `set_visible` **and** `open()`'s re-show branch on the active root.
- `browser/pipe.rs` + `commands/browser.rs`: pipe stays per-process but each call
  resolves `active_browser(app)`; every command gained a `root` arg; per-process
  listener lifecycle (`should_stop_listener`).
- Frontend: `BrowserPanel` moved into `WorkspaceTab` (mirrors `SqlPanel`), props
  `root`+`active`; `browserLayoutKey(root)` per-root; `pageVisible`/
  `hiddenPageReason` gained `active`; `sync()` re-runs on active flip; `App.tsx`
  browser state removed, opens via `activeHandle()?.openBrowser()`; App attention-
  pulse (bugs 1-5) preserved. `pluginMenuLogic` `webBrowser` → `needsWorkspace`.
- IPC: `api.ts` wrappers take `root`; no wire *type* changed (root is an arg), so
  `types.ts`/`model.rs` unchanged.

Review found ONE confirmed finding (open() re-show bypassed the visible_for
backstop); the Fix phase applied it (mod.rs:250-252). 

**Gate re-run by hand (workflow self-report was NOT trusted, per [[verify-agent-claims]]):**
pnpm typecheck ✅; cargo fmt --check ✅; pnpm test 1958/1958 ✅;
cargo test -p cb-core browser:: 371 ✅; cargo test -p cb-app --lib 237 ✅;
cargo check --workspace --all-targets ✅. (Inline IDE diagnostics were stale
mid-edit snapshots — cargo was the ground truth.)

## Dropped
- Reveal-in-tree (originally reported) confirmed already working; dropped by user.

## Not yet done (manual)
- No end-to-end manual run in the live app (per the plan's manual verification
  steps 1-7). All automated gates pass; a human smoke test of the browser
  per-workspace behaviour and the tab-flash settle is still worth doing.

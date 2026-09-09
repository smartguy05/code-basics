# Completed — Bugs 6+7 true per-workspace browser

## Root cause
Single app-wide OS webview (`thread_local! Option<Host>`, one `AppState.browser`,
one app-level `BrowserPanel`, unscoped layout key). WebView2 child HWND
composites above the DOM and nothing tied its visibility to the active
workspace, so it painted over whichever codebase you switched to.

## Fix (keyed by workspace root + active-gated visibility + active-resolved pipe)
Backend:
- `state.rs`: `browser` field -> `Mutex<HashMap<PathBuf, Arc<Mutex<BrowserShared>>>>`.
  New `browser_shared`, `active_browser_shared` (security choke point),
  `browser`/`active_browser`(handle), per-root `browser_data`/`_mut`,
  `active_root_pathbuf`, `browser_pipe_published`. `close()` drops the root's slot.
- `browser/mod.rs`: `HOST` -> `HashMap<PathBuf, Host>`; `BrowserHandle` carries
  `root`; `on_main` closure takes `(&AppHandle,&shared,&mut map,&root)`; `close`
  returns `bool` (any other host remains); `set_visible` gated by new
  `visible_for(requested, root, active)` free fn.
- `commands/browser.rs`: every command gained `root: String`; pipe lifecycle now
  per-process (`start_listener` idempotent, `should_stop_listener`,
  `published_listener`).
- `browser/pipe.rs`: `start`/`accept_loop`/`serve_connection`/`answer_line` carry
  `AppHandle`; each call resolves `AppState::active_browser(app)`.

Frontend:
- `browserPanelLogic.ts`: `BROWSER_LAYOUT_KEY` -> `browserLayoutKey(root)`;
  `active` added to `PageVisibilityInput`/`pageVisible`/`hiddenPageReason`.
- `BrowserPanel.tsx`: props `root`+`active`; threads root through every api call;
  sync passes `active` (+ effect re-running on active flip).
- `WorkspaceTab.tsx`: hosts BrowserPanel (mirror SqlPanel), `openBrowser` handle.
- `App.tsx`: removed app-level browser state/render/imports; `plugin.browser` and
  Plugins menu -> `activeHandle()?.openBrowser()`. PRESERVED attention pulse.
- `pluginMenuLogic.ts`: webBrowser `needsWorkspace: true`.
- `ipc/api.ts`: browser wrappers take `root` first arg. types.ts/model.rs: no change.

## Tests (first)
- state_tests.rs: browser_shared stable, active_browser_shared resolves active
  root, grant-never-visible-from-another, panel-closed cases, close drops data,
  active_root_pathbuf.
- host_tests.rs: visible_for.
- commands/browser.rs: should_stop_listener.
- browserPanelLogic.test.ts: browserLayoutKey + active gating.
- pluginMenuLogic.test.ts: browser now needs a workspace.

## Gate — all green
- pnpm typecheck: ok
- pnpm test: 1958 passed
- cargo test -p cb-core browser:: : 371 passed (regression; cb-core untouched)
- cargo test -p cb-app --lib: 237 passed (incl. all new tests)
- cargo fmt --check: clean
- cargo check --workspace --all-targets: clean
- pnpm docs:index + docs:check: passed

## Behavior change (intended)
Browser no longer openable on the welcome screen (per-workspace): disabled
Plugins row with a reason when no codebase is open.
</content>

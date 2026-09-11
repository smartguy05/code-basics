# Bugs 6+7 — true per-workspace embedded browser

Make the embedded browser panel per-workspace: each open codebase gets its own
live webview; only the ACTIVE workspace's is visible. Fixes the stacking bug
(app-wide OS webview painting over whichever workspace you switch to).

## Key moves
- `HOST: RefCell<Option<Host>>` -> `RefCell<HashMap<PathBuf, Host>>` (mod.rs)
- `AppState.browser: Arc<Mutex<BrowserShared>>` -> `Mutex<HashMap<PathBuf, Arc<Mutex<BrowserShared>>>>`
- `BrowserHandle` carries a `root: PathBuf`.
- Visibility follows active workspace: frontend `pageVisible` gains `active`;
  host `set_visible` refuses non-active root (`visible_for`).
- Agent pipe captures `AppHandle`, resolves the ACTIVE root per call
  (`AppState::active_browser`) — the security choke point.
- Consent is per-root structurally (each root's own BrowserShared).
- BrowserPanel moves into WorkspaceTab (mirror SqlPanel), keyed layout
  `cb.browser.layout:<root>`.

## Preserve
- Bugs 1-5 (attention pulse in App.tsx, etc.). Do NOT touch.
- capabilities/default.json (webviews grant).
</content>
</invoke>

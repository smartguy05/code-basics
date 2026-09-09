# Feature: web browser plugin

Phase 4 of the "major enhancements" plan
(`~/.claude/plans/major-enchancements-add-witty-hopper.md`, lines 762-1016).

A floating browser panel inside the app, plus a read-only-by-default MCP surface
so an agent can look at the page the user is looking at.

## Acceptance criteria relevant to this step (Phase 4a)

- `crates/core/src/browser/` holds **every** browser decision, testable with no
  webview, no process and no I/O.
- `BrowserAvailability` has **six** variants and they never collapse.
- `AutomationConsent` makes `writes && !reads` unrepresentable.
- A search phrase typed into the URL bar is **refused**, not searched.
- Navigation to an app origin (`tauri://localhost`, `http://tauri.localhost`,
  `http://localhost:1420`), `file:`, `data:`, `javascript:`, a custom scheme, or
  any `about:` other than `about:blank`, is refused.
- The console/network ring **counts what it dropped** and a `since` cursor never
  returns a dropped entry as though it were present.
- Every injected script is `try`/`catch` wrapped (Windows `eval_with_callback`
  swallows exceptions) and every selector / typed string is JSON-encoded.
- `parse_page_message` treats every message from the page as hostile input.
- `src/components/browserPanelLogic.ts` mirrors `sqlPanelLogic.ts`, including the
  same-reference-on-no-op discipline.

## Explicitly out of scope for this step

The wry host, the Tauri commands, `BrowserPanel.tsx`, the named pipe, the MCP
server and the installer entry. Phases 4b-4d.

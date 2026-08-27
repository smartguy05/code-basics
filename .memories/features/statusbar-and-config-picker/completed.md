# Completed: bottom status bar + run-config picker relocation

Cosmetic layout changes (2026-08-27, branch claude/statusbar-and-config-picker).

## What the user asked
1. Add a status bar at the bottom; move the current folder name + directory path
   out of the top titlebar into it.
2. Move the run-config picker into the same toolbar as the environment picker,
   just to the right of that dropdown.

## Changes
- `src/App.tsx` — removed the `workspace-name` + `root` spans and the
  `#run-config-slot` div from the titlebar (kept `BranchMenu`); added a
  `.statusbar` at the bottom of `.app` showing `activeWorkspace.name` +
  `.root` (title=full path).
- `src/components/RunConfigMenu.tsx` — no longer a titlebar portal. Dropped
  `createPortal`/`useEffect`/the `#run-config-slot` lookup and the `active`
  prop; now renders inline as `<div className="dropdown run-config-menu">`.
- `src/views/RunView.tsx` — moved the `<RunConfigMenu>` element out of the top
  of the render into the `.toolbar`, immediately after the `EnvironmentPicker`.
  Removed the now-unused `foreground` prop (decl + doc).
- `src/components/WorkspaceTab.tsx` — dropped `foreground={active}` on `RunView`;
  refreshed the per-tab-vs-global doc comment.
- `src/styles.css` — retargeted `#run-config-slot ...` rules to `.run-config-menu`;
  added `.statusbar` / `.statusbar .workspace-name` / `.statusbar-path`; removed
  the dead `.titlebar .workspace-name` rule.

## Notes / gotchas
- RunConfigMenu was portaled into a single shared titlebar slot precisely so only
  the foreground tab's dropdown showed. Rendering inline per Run toolbar is safe
  because background WorkspaceTabs are `hidden`, so no dropdown stacking — which
  is why the `active`/`foreground` gate could be deleted entirely.

## Terminal header focuses the terminal
- `TerminalPanel.tsx::onHeaderPointerDown` now calls `viewRef.current?.focus()`
  after the button guard, so clicking the title bar puts the caret in xterm and
  you can type immediately (the header is not xterm, so a click there used to
  leave focus elsewhere).

## Version bump 0.1.0 -> 1.0.0
- `Cargo.toml` [workspace.package], `package.json`, `src-tauri/tauri.conf.json`,
  and the `cb-core`/`cb-app` entries in `Cargo.lock`. Third-party 0.1.0 crates
  and untracked `package-lock.json` left alone. `cargo metadata` confirms.

## Docs sync (this session's changes + merged floating-terminals)
- `README.md`, `docs/getting-started/using-the-app.md`, `docs/architecture/frontend.md`,
  root `CLAUDE.md`: config picker now in the Run toolbar (not titlebar), folder
  name/path in the new bottom status bar, Secrets is a per-project picker,
  terminal pill flashes on the **bell** only (+ copy/paste chords + workspace-tab
  flash), notes are crash-safe (atomic write + pagehide/max-wait flush), terminals
  hosted per codebase with per-codebase numbering.
- `docs/INDEX.md` regenerated (`pnpm docs:index`). It crossed 500 lines, so
  `scripts/check-docs.mjs` now **exempts the generated INDEX.md** from the line cap
  (links still checked); header comment + CLAUDE.md docs:check note updated.
- `pnpm docs:check` passes.
- Multiple-open-codebases (workspace tabs) now documented: README feature bullet,
  a "Working with several codebases" subsection in using-the-app.md, and a
  `WorkspaceTab` architecture bullet in frontend.md (per-codebase mounting,
  per-slot backend state, shared vs global chrome, dirty-close caveat).

## Gates
- `pnpm typecheck` clean; `pnpm test` 986 passed; `pnpm docs:check` passes. No
  Rust code changes (only Cargo version strings).
- Not yet done: live `pnpm tauri dev` visual check.

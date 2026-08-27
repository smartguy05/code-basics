# Completed — terminal/notes/inspect UX

## 1. Headless spawning (no OS console windows on Windows)
- `crates/core/src/process/kill.rs`: added `CREATE_NO_WINDOW` (0x0800_0000),
  `CREATE_NEW_PROCESS_GROUP` (0x0000_0200), `windows_creation_flags()` (both OR'd),
  and `no_window(&mut std::process::Command)` (windows-only). `configure_process_group`
  now sets `windows_creation_flags()` — so the Supervisor (`process/mod.rs`) and LSP
  transport (both call it) spawn windowless. `taskkill` uses `no_window`.
- Re-exported `no_window` from `process/mod.rs` (`#[cfg(windows)]`).
- Applied `#[cfg(windows)] no_window(&mut cmd)` at: `adapters/msbuild.rs` (dotnet SDK eval),
  `git/repo.rs` (git apply), `behavioral/worktree.rs` (add/remove/prune),
  `src-tauri/src/qgate_run.rs` (changed_paths git + failing_output gate runner).
- Tests: `kill.rs` unit tests assert flag values (0x0800_0200; no_window has no group bit).
- ConPTY floating terminals unchanged (already windowless).

## 2. Minimized panel overlap
- `terminalLogic.ts`: `pillBottom(index) = 16 + (index+1)*48` (base slot 16 reserved for
  Notes bar). Terminal pill uses it in `TerminalPanel.tsx`. Notes pill stays at base.
- Tests in `terminalLogic.test.ts` (`pillBottom(0)===64`, etc.).

## 3. Live-attach confirmation (Objects tab)
- Removed the always-on `warning inspect-notice` live banner in `InspectView.tsx`.
- `inspectLogic.ts`: `shouldConfirmAttach(target, suppressed)` (live && !suppressed),
  `isAttachWarnSuppressed`/`suppressAttachWarn` (localStorage key `cb.inspect.attachWarn.suppressed`).
- New `components/AttachConfirm.tsx` modal (modal-backdrop/modal/modal-body + "Don't warn
  me again"). `InspectView` routes both `capture()` and the cross-tab `pendingRequest`
  through `beginCapture`, which gates live attaches behind the modal.
- Tests in `inspectLogic.test.ts`.

## 4. Terminal title
- `terminalLogic.ts`: `renameTerminal(list, key, title)` (rejects blank).
  `TerminalDescriptor.title` reused. `TerminalPanel` header: double-click title → inline
  `<input>` (Enter/blur commit, Esc cancel) via new `onRename` prop → `WorkspaceTab`
  `setTerminals(renameTerminal(...))`. In-memory (terminals don't persist).

## 5. Pill color
- `TerminalDescriptor.color?: string` + `recolorTerminal(list, key, color)`.
- New `components/PillColorMenu.tsx` (preset swatch popover, `.dropdown` chrome; "Default"
  clears to undefined). Used in both terminal + Notes headers.
- Terminal pill: inline `background` (suppressed while attention flash runs). Via `onRecolor`
  prop → `WorkspaceTab`. In-memory.
- Notes pill: single persisted value — `notesLogic.ts` `NOTES_COLOR_KEY = "cb.notes.pillColor"`,
  `loadPillColor`/`savePillColor`. `NotesPanel` holds state, applies inline bg.
- CSS in `styles.css`: `.terminal-title-edit`, `.pill-color-*`.

## Verification
- `pnpm typecheck` clean; `pnpm test` 1003 passed; `cargo fmt --check` clean;
  `cargo test -p cb-core process::kill` passed. Full `cargo test -p cb-core` run pending
  confirmation. Manual in-app verification still to do by the user (`pnpm tauri dev`).

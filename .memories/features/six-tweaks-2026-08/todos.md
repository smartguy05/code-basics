# Todos

- [x] **Item 2 — manual intent grouping.** Done at file and card level. Still
      open: a **per-hunk** affordance. `GroupFile.hunks` carries the indices and
      `move_card_edits` already takes content rather than geometry, so the
      backend needs nothing — only `IntentPanel` rendering hunk rows.
- [~] **In-app DAP debugging.** The pure layers are done and tested (see
      `completed.md`). **Not done, in order:**
      1. `dap/transport.rs` + `dap/session.rs` — spawn the adapter, pump
         `lsp::framing` over its stdio, run the handshake
         (`initialize` → `initialized` event → `setBreakpoints` →
         `configurationDone` **only if the capability is claimed**), and tear
         down through `process::kill_tree`.
      2. `crates/core/src/bin/fake_dap.rs` — a scripted adapter, like
         `bin/fake_lsp.rs`, so the transport is testable with none installed.
      3. Tauri commands + `AppState` slot, mirroring the `LspHandle` actor
         pattern (`set_workspace` clears caches under a `std::sync::Mutex`, so
         teardown cannot `.await`).
      4. UI: a breakpoint gutter (`components/debugLogic.ts` for the decisions,
         `debugExtension.ts` for the CodeMirror plumbing — the
         `usagesLogic`/`usagesExtension` split), a **Debug** button beside Run,
         a stack/variables panel, and `RunKind::Debug` in `running/record.rs`.
      A **socket transport** is additionally needed before Node can work at all.
- [ ] Run `pnpm test` and `pnpm typecheck` — could not be run in-session
      (see `notes.md`). New/changed suites: `folderTreeLogic`, `fileTreeLogic`,
      `runningLogic`, `configLogic`, `intentPanelLogic`.
- [x] `pnpm docs:index` — `scripts/generate-index.mjs` **runs under plain node**
      with no junction traversal, so it worked despite the broken `pnpm`.
      `scripts/check-docs.mjs` does too.
- [x] `docs/reference/commands.md` — the four `fs_*` commands and
      `move_card_edits` added, plus the new `build_configuration` parameter on
      `start_run` / `build_project`.
- [x] **All prose docs updated** (2026-08-30): `README.md` (Run/Edit/Changes
      bullets), `CLAUDE.md` (`adapters` fanout, `files.rs`, `symbols`
      two-verbs, a new `dap/` bullet, the frontend paragraph),
      `docs/getting-started/using-the-app.md` (build-configuration picker, the
      Stop split button, the file-tree right-click menu, the Files tree
      default, the intent move), `docs/architecture/core-crate.md` (a `dap`
      section + the dotnet/files changes), `docs/architecture/frontend.md`
      (`ContextMenu`, `FileTree`, the Stop button, `IntentPanel`),
      `docs/architecture/tauri-shell.md` (a new "keeping the symbol index
      honest" section covering the deliberate directory-delete gap),
      `docs/reference/configuration.md`, and
      `docs/guides/agent-intent-capture.md`.
      **No user-facing debugging guide was written** — `dap/` cannot start a
      session, so a guide would document a feature that does not work; it is
      covered in `core-crate.md` and `CLAUDE.md` for developers only.
- [ ] End-to-end in `pnpm tauri dev`: confirm the Changes Tree layout renders
      folder rows (the symptom the user reported against the release build).

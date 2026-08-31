# Completed — app launcher

## 2026-08-28 — feature implemented end to end

### Backend (`cb-core`)

New module `crates/core/src/launcher/` (registered in `lib.rs`):

- `model.rs` — `Launchable { id, command, cwd, env, label, shell, pinned, lastRunMs, runCount }`,
  `LauncherFile { version, entries }`, `LauncherGroups { thisCodebase, global }`. camelCase keys
  pinned by `model_tests.rs`; `label` is an explicit `null`, never a skipped key.
- `parse.rs` — quote-aware tokeniser (`"` groups, only `\"` escapes so Windows paths survive).
  Empty line, unbalanced quote and **unquoted `| > < & ;`** are errors that name the fix;
  `shell_flag()` is `/C` on Windows, `-c` elsewhere; `program_and_args(line, shell)`.
- `store.rs` — user-global `<config>/code-basics/launchers.json` (`CB_LAUNCHERS_PATH`), copied
  from `notes.rs`: tolerant `load`, atomic temp+rename `save`, `.bak` before an empty overwrite.
- `recents.rs` — identity `(command, cwd)`; a re-run bumps clock/count and adopts the `shell`
  flag but preserves pin + rename; `MAX_UNPINNED = 30`, pinned entries never evicted;
  `group()` (this codebase first, pinned then most recent), `within_root`, `rename`,
  `set_pinned`, `remove`, `find`.
- `running/record.rs` — new `RunKind::External`.

### Bridge (`src-tauri`)

- `commands/launcher.rs` — `list_launchables`, `launch_command`, `stop_command`,
  `save_launchable`, `delete_launchable` (registered in `lib.rs`). Decisions extracted and
  tested: `launch_cwd`, `launch_label` (+ `elide`), `launch_key`, `launch_invocation`.
  Runs in the **global** supervisor so an app survives closing its codebase.
- `commands/running.rs` — `External` routes its kill to the global supervisor.

### Frontend

- `ipc/types.ts` — `Launchable`, `LauncherFile`, `LauncherGroups`, `LaunchedApp`;
  `ProcessKind` gains `"external"`.
- `ipc/api.ts` — `listLaunchables`, `launchCommand`, `stopCommand`, `saveLaunchable`,
  `deleteLaunchable`.
- `components/launcherLogic.ts` + test — `needsShell` (mirrors the Rust quoting rules),
  `displayLabel`, `canRun`, `shortCwd`, `filterGroups`, `pickerRows`, `moveSelection`,
  `pickerKeyAction`.
- `components/appOutputLogic.ts` + test — `makeTab`, `addTab`, `closeTab`, `applyEvent`,
  `statusText`, `canStop`, `liveTabCount`, `tabTitle`, `APP_OUTPUT_LAYOUT_KEY`.
- `components/LauncherPicker.tsx` — titlebar **Launch** overlay.
- `components/AppOutputPanel.tsx` — one floating panel, a tab per app, consoles kept mounted.
- `components/RunningPanel.tsx` + `runningLogic.ts` — `external` glyph/label (⚡ / "App") and a
  **View** action gated by the new `hasOutput`.
- `App.tsx` — launcher state, the console/pending-output refs, `launchApp`/`stopApp`/
  `closeAppTab`/`viewAppOutput`, titlebar **Launch** and **Apps** buttons, both panels mounted.
- `styles.css` — `.launcher-*` and `.app-output-*`.

### Docs

`docs/reference/commands.md` (new launcher section + the two Running-panel rows),
`docs/architecture/core-crate.md` (`## launcher`), `docs/architecture/frontend.md`
(tree entries + a component paragraph), `docs/getting-started/using-the-app.md`
(`## Running other apps`), `CLAUDE.md` (architecture bullets), `docs/INDEX.md` regenerated.

### Verified

- `cargo test -p cb-core` — 2382 + integration suites, all pass (39 of them new under
  `launcher::`, watched failing first against `unimplemented!()` stubs).
- `cargo test -p cb-app --lib` — 49 pass (12 new in `commands::launcher::tests`).
- `cargo fmt --all -- --check` — clean.
- `node scripts/generate-index.mjs`, `node scripts/check-docs.mjs` — pass.
- **Not run:** `pnpm typecheck` / `pnpm test` / `pnpm coverage` — see `notes.md`, the agent
  shell cannot traverse pnpm's junctions in this environment. Reviewed by hand against the
  strict flags instead (`noUncheckedIndexedAccess`, parameter narrowing in closures).

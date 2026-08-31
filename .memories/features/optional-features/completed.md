# Completed — optional features

## Stage 1: the store, the gate, the picker (done)

### Backend

- `crates/core/src/features/mod.rs` — `FeatureId` (`sqlConsole`, `askCodebase`)
  with `ALL` / `id` / `label` / `description` / `default_enabled` / `from_id`;
  `FeaturesFile { version, enabled: BTreeMap<String, bool> }` with `is_enabled`
  (absent id falls back to the built-in default), `set`, `list`; `FeatureInfo`
  as the wire row.
- `crates/core/src/features/store.rs` — `features_path()` (`CB_FEATURES_PATH`
  override, `notes.rs` resolution order), tolerant `load`, `load_existing`
  (distinguishes "no store" from "empty store"), atomic `save`, `Platform`,
  `seed_path_for` / `seed_path`, `merge_seed`, `ensure_seeded`.
- `crates/core/src/features/{features,store}_tests.rs` — 26 tests, all green.
- Registered as `pub mod features;` in `crates/core/src/lib.rs`.
- `src-tauri/src/commands/features.rs` — `list_features`, `set_feature`; no
  `AppState` (user-global store, the `notes`/`launcher` precedent). Registered in
  `src-tauri/src/lib.rs`.

### Frontend

- `src/components/featuresLogic.ts` + `.test.ts` — `featureEnabled`,
  `visibleTabs`, `tabAfterDisable`.
- `src/components/FeaturesPicker.tsx` — the checkbox dialog; writes through on
  each toggle and re-renders from what was persisted, so there is no OK/Cancel.
- `src/ipc/types.ts` `FeatureInfo`; `src/ipc/api.ts` `listFeatures` /
  `setFeature`.
- `src/App.tsx` — `features` state loaded once at startup (also the seed point),
  `featuresOpen`, passes `features` down.
- `src/components/MenuBar.tsx` — File → **Optional features…**.
- `src/components/WorkspaceTab.tsx` — `FEATURE_BY_TAB` (empty until the SQL tab
  exists), `shownTabs`, and the effect that moves the selection off a tab whose
  feature was just switched off.
- `src/styles.css` — `.features-*`, reusing `.launcher-overlay`.

### Docs

- `docs/reference/commands.md` — new "Optional features" section.
- `docs/INDEX.md` regenerated; `check-docs` passes.

### Gate

- `cargo test -p cb-core --lib` — 2567 pass (see `notes.md` on the pre-existing
  flaky `cancel_stops_a_long_running_process`).
- `cargo test -p cb-core --tests` — every integration target green.
- `cargo fmt --all --check` clean.
- `pnpm typecheck` / `pnpm test` **could not be run** from the agent shell
  (pnpm junctions); one real type error was found and fixed by filtering the
  noise. The user needs to run the frontend gate.

## Stage 6b — the installer→app contract is now pinned by tests (2026-08-30)

Six tests added to `crates/core/src/features/store_tests.rs`. They read the real
packaging artefacts off disk (`src-tauri/installer/windows/installer.nsi`,
`src-tauri/resources/linux/features.json`, `src-tauri/tauri.conf.json`) rather
than restating their contents, so the files themselves are the fixtures and
nothing can drift silently.

- `the_windows_installer_writes_json_the_app_can_parse` — a small NSIS
  interpreter (`nsis_seed_json`) replays `Function WriteFeaturesSeed` for all
  four checkbox combinations and parses the exact bytes as a `FeaturesFile`.
- `first_launch_adopts_the_bytes_the_windows_installer_writes` — the same bytes
  through the real `ensure_seeded` write-through path.
- `the_shipped_linux_seed_parses_and_is_all_on`.
- `the_windows_seed_path_matches_what_the_nsis_opens` — the `FileOpen` target vs
  `SEED_FILE` / `seed_path_for(.., Windows)`.
- `the_linux_seed_path_matches_the_deb_files_mapping` — `seed_path_for(..,
  Linux)` vs the key in `bundle.linux.deb.files`, plus the source file existing.
- `every_feature_appears_in_both_installers` — every `FeatureId` in both seeds,
  with a declared `Var Feature<Id>` and the picker's own label/description text
  in the page; and no seed id that is not a `FeatureId`.

**Gate:** `cargo test -p cb-core --lib features::` → 32 passed. Full
`cargo test -p cb-core --lib` → 2812 passed, 0 failed. `cargo fmt` clean.
**No installer was built** — `pnpm` is unusable in this shell, so `pnpm tauri
build` cannot run. These pin the contract, not the artefact.

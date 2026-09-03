# Completed — Phase 3 (Help -> About)

## Files touched

New:
- `src-tauri/src/commands/about.rs` — `about_info`, local `AboutInfo`, 2 tests
- `src/components/AboutDialog.tsx`
- `src/components/aboutLogic.ts` + `aboutLogic.test.ts` (21 tests)

Modified:
- `src-tauri/build.rs` — `CB_GIT_SHA`, `CB_BUILD_DATE`, watches `../.git/HEAD`
- `src-tauri/src/lib.rs` — `pub mod about;` + `commands::about::about_info,`
- `src/components/MenuBar.tsx` — `TopMenu` gains `"help"`, `onOpenAbout` prop, Help block
- `src/App.tsx` — `aboutOpen`, `onOpenAbout`, `{aboutOpen && <AboutDialog/>}`
- `src/ipc/types.ts` — `AboutInfo`; `src/ipc/api.ts` — `aboutInfo()`
- `docs/reference/commands.md` — "## About"; `docs/INDEX.md` regenerated

## Tests written first, watched fail

1. `about_info_serialises_with_the_keys_the_ui_reads` — written with the struct
   deliberately lacking `#[serde(rename_all)]`. Failed with
   `left: ["app_version", "arch", "build_date", "git_sha", "os"]` against
   `right: ["appVersion", "arch", "buildDate", "gitSha", "os"]`. Added the
   attribute; passed.
2. `aboutLogic.test.ts` — failed first with `Cannot find module './aboutLogic'`,
   then `versionDrift is not a function` for the later block. Implemented; 21 pass.

## Gate (all run, all green)

- `cargo test -p cb-core` — 3195 passed, 0 failed (incl. the 302s `process::` file)
- `cargo test -p cb-app` — 128 passed, 0 failed
- `cargo fmt --check` clean; `cargo clippy -p cb-app --all-targets` no diagnostics
- `pnpm typecheck` clean (no junction block this session)
- `pnpm test` — 69 files, 1753 tests passed
- `pnpm docs:index` (512 files, 160 commands) + `pnpm docs:check` passed

## Verified, not assumed

- The stamp really works: `target/debug/build/cb-app-*/output` contains
  `cargo::rustc-env=CB_GIT_SHA=20a7dbb-dirty`, and `git rev-parse --short HEAD`
  is `20a7dbb` on a dirty tree. `CB_BUILD_DATE=1788466955` decodes to
  2026-09-03, today.
- The `core:app:default` permission chain (see notes.md).

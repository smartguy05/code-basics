# Plan (as implemented)

| Piece | File |
|---|---|
| Help menu | `src/components/MenuBar.tsx` — `TopMenu` union + a third inline `.dropdown` block |
| Dialog | `src/components/AboutDialog.tsx` (new) — shared `modal-backdrop`/`modal` markup, no new CSS |
| String composition | `src/components/aboutLogic.ts` + `.test.ts` (new, 21 tests) |
| Wiring | `src/App.tsx` — `aboutOpen` flag, `onOpenAbout` prop, `{aboutOpen && <AboutDialog/>}` |
| Command | `src-tauri/src/commands/about.rs` (new) — local `AboutInfo`, no `AppState`, own key-pinning `#[test]` |
| Registration | `src-tauri/src/lib.rs` — `pub mod about` + `commands::about::about_info` in `generate_handler!` |
| Build stamp | `src-tauri/build.rs` — `CB_GIT_SHA` / `CB_BUILD_DATE` |
| Wire mirror | `src/ipc/types.ts` (`AboutInfo`), `src/ipc/api.ts` (`aboutInfo`) |
| Docs | `docs/reference/commands.md` "## About" section, then `docs:index` + `docs:check` |

## Where each value comes from

- **Bundle version + Tauri version** — `getVersion()` / `getTauriVersion()` from
  `@tauri-apps/api/app`. Verified in `src-tauri/gen/schemas/acl-manifests.json`
  that `core:default` includes `core:app:default`, which includes `allow-version`
  and `allow-tauri-version`. **No new dependency, no capability edit.**
- **OS / arch / crate version / build stamp** — the `about_info` command, rather
  than a `tauri-plugin-os` dependency.

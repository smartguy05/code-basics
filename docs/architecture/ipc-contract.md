# The IPC type contract

Every value crossing between Rust and TypeScript is defined twice:

- **Rust:** `crates/core/src/model.rs` (plus `workspace.rs`, `files.rs`, `git/repo.rs`, `git/patch.rs`, `process/mod.rs`, and the small response structs in `src-tauri/src/commands/`), all deriving `Serialize`/`Deserialize` with `#[serde(rename_all = "camelCase")]`.
- **TypeScript:** `src/ipc/types.ts`, written **by hand** against those camelCase names.

There is no codegen wired up. Every Rust model type also derives `specta::Type`, which keeps the door open to generating the TypeScript later, but today the mirror is manual.

## How drift is caught

A renamed Rust field would serialise happily and only surface as `undefined` in the UI — silently. To make that failure loud, the `tests` module at the bottom of `model.rs` pins the **exact JSON keys** each type produces:

- `test_case_serialises_with_the_keys_the_ui_reads`
- `project_serialises_with_camel_case_keys`
- `enum_variants_serialise_in_camel_case`
- `optional_config_fields_are_omitted_rather_than_null`

Renaming a Rust field fails one of these tests, and the failure message points at the TypeScript file that needs the matching edit. Crossing types defined outside `model.rs` pin their keys beside their own module instead — `workspace_serialises_with_the_keys_the_ui_reads` in `workspace.rs`, `dir_entry_serialises_with_the_keys_the_ui_reads` in `files.rs`.

> Note: the comment atop `types.ts` refers to these as a test named `serialisation_shape`; the tests exist but under the names above.

## Rules when changing a crossing type

1. Change the Rust struct/enum in `cb-core` (or the command-level response struct in `src-tauri`).
2. Update the key-pinning test in `model.rs` — this is deliberate friction, not busywork.
3. Update the mirror in `src/ipc/types.ts`.
4. Run `cargo test -p cb-core` and `pnpm typecheck`.

Conventions the mirror relies on:

- Everything is camelCase (`rename_all` on every crossing type).
- Optional `RunConfig` fields use `skip_serializing_if`, so absent means *absent*, not `null` — the config file is checked in and should stay minimal. The TypeScript side types these as optional (`?`), not `| null`.
- Enum variants serialise as camelCase strings (`"microsoftTestingPlatform"`, `"riderImport"`), mirrored as TypeScript string-literal unions.
- `TestOutcome` defaults to `other` so an unrecognised outcome is never silently counted as a pass.

## Streaming: `Channel<ProcessEvent>`

Request/response commands return JSON once. The three streaming commands (`start_run`, `run_tests`, `git_network`) additionally take a Tauri `Channel` and push `ProcessEvent`s (stdout/stderr chunks, exit) through it while the promise stays pending. The wrapper functions in `src/ipc/api.ts` hide the channel plumbing behind an `onEvent` callback.

## Errors

Commands return `Result<T, String>`: errors cross as human-readable strings with anyhow context preserved (`{e:#}`). The frontend's `errorMessage()` normalises whatever it receives.

Related: [Tauri shell](tauri-shell.md) · [frontend](frontend.md) · [command reference](../reference/commands.md).

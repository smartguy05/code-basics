# The IPC type contract

Every value crossing between Rust and TypeScript is defined twice:

- **Rust:** `crates/core/src/model.rs` (plus `workspace.rs`, `files.rs`, `git/repo.rs`, `git/patch.rs`, `process/mod.rs`, `inspect/model.rs`, and the small response structs in `src-tauri/src/commands/`), all deriving `Serialize`/`Deserialize` with `#[serde(rename_all = "camelCase")]`.
- **TypeScript:** `src/ipc/types.ts`, written **by hand** against those camelCase names.

There is no codegen wired up. Every Rust model type also derives `specta::Type`, which keeps the door open to generating the TypeScript later, but today the mirror is manual.

## How drift is caught

A renamed Rust field would serialise happily and only surface as `undefined` in the UI — silently. To make that failure loud, the `tests` module at the bottom of `model.rs` pins the **exact JSON keys** each type produces:

- `test_case_serialises_with_the_keys_the_ui_reads`
- `project_serialises_with_camel_case_keys`
- `enum_variants_serialise_in_camel_case`
- `optional_config_fields_are_omitted_rather_than_null`

Renaming a Rust field fails one of these tests, and the failure message points at the TypeScript file that needs the matching edit. Crossing types defined outside `model.rs` pin their keys beside their own module instead — `workspace_serialises_with_the_keys_the_ui_reads` in `workspace.rs`, `dir_entry_serialises_with_the_keys_the_ui_reads` in `files.rs`, and the inspector's types in `inspect/model_tests.rs`:

- `inspect_node_serialises_with_the_keys_the_ui_reads`
- `inspect_graph_serialises_with_the_keys_the_ui_reads`
- `target_and_caps_serialise_with_camel_case_keys`
- `value_variants_are_tagged_by_kind_in_camel_case`
- `request_variants_serialise_in_camel_case`
- `addresses_cross_as_hex_strings`
- `an_attachable_process_serialises_with_the_keys_the_ui_reads`
- `an_unattributed_process_carries_no_configuration_at_all`
- `a_list_that_could_not_be_completed_carries_its_warnings`
- `a_dotnet_process_serialises_with_the_keys_the_sidecar_writes`
- `a_run_dump_serialises_with_the_keys_the_ui_reads`
- `a_request_carries_the_schema_version_and_does_not_suspend_by_default`
- `a_default_inspector_config_stays_out_of_the_checked_in_file`

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
- **Heap addresses cross as hexadecimal strings, never as numbers** (`ObjectValue::Reference`, `ObjectValue::Cycle`, `RootSpec::Address`). Present Windows and Linux user-mode addresses sit below 2^47 and so would survive a JavaScript number today — but that is an accident of current address-space layout rather than a guarantee, and the address is the *identity* used for expansion and cycle detection, round-tripped straight back into the next request. Hex is also what SOS and WinDbg print, so a value can be pasted between tools. Pinned by `addresses_cross_as_hex_strings`.
- `DotnetProcess` crosses **two** boundaries, which is unusual here. It is what `cb-inspector --list-processes` writes (`ProcessDto` in `sidecar/inspector/Wire.cs`) *and* a mirrored TypeScript interface, so its camelCase keys are pinned on the Rust side by `a_dotnet_process_serialises_with_the_keys_the_sidecar_writes` while the C# side reaches the same names through `Wire.Options`. Only `pid` and `name` are guaranteed; `path`, `parentPid` and `startedAt` are `skip_serializing_if` optionals because each is read through an API that legitimately fails on a process of another elevation or bitness. **Absent means unknown, never a default** — an invented `parentPid` would attribute a stranger's process to the user's run configuration.
- `Attribution` is a three-valued camelCase string union (`"launched" | "descendant" | "unrelated"`) rather than a boolean, because the middle case — the child `dotnet run` started — is the one the picker exists to surface. Pinned by `an_attachable_process_serialises_with_the_keys_the_ui_reads` and `an_unattributed_process_carries_no_configuration_at_all`, the second of which asserts that an `unrelated` row carries **no** `configId` and **no** `configName` at all.
- `AttachableList` wraps `processes` with `warnings`, so a list that came back degraded — a host that would not report any process's parent — says so on the same value. A total failure is not in here: it is an `Err` from the command, because "nothing is attachable" and "I could not look" must never render as the same answer. `warnings` is `skip_serializing_if = "Vec::is_empty"`, mirrored as an optional array. Pinned by `a_list_that_could_not_be_completed_carries_its_warnings`.
- `AttachableProcess.isApplication` is a plain `bool` that is **always serialised**, unlike every optional field beside it. It is the answer to "is this the process holding the user's objects?", and it is the field the UI preselects a capture target from; a key that vanished when the answer was *no* would arrive as `undefined`, which is falsy and therefore indistinguishable from a backend that never sent it. `false` means *no evidence* rather than "this is not the application" — see `session::attribute`. Pinned by `an_attachable_process_serialises_with_the_keys_the_ui_reads` and `an_unattributed_process_carries_no_configuration_at_all`, which assert its presence with both values.
- Tagged enums that carry data use an internal `kind` tag in camelCase (`ObjectValue`, `InspectTarget`, `RootSpec`), mirrored as TypeScript discriminated unions on `kind`.

## Streaming: `Channel<ProcessEvent>`

Request/response commands return JSON once. The four streaming commands (`start_run`, `run_tests`, `git_network`, `inspect_capture`) additionally take a Tauri `Channel` and push `ProcessEvent`s (stdout/stderr chunks, exit) through it while the promise stays pending. The wrapper functions in `src/ipc/api.ts` hide the channel plumbing behind an `onEvent` callback.

## Errors

Commands return `Result<T, String>`: errors cross as human-readable strings with anyhow context preserved (`{e:#}`). The frontend's `errorMessage()` normalises whatever it receives.

Related: [Tauri shell](tauri-shell.md) · [frontend](frontend.md) · [command reference](../reference/commands.md).

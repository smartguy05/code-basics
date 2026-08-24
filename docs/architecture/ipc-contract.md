# The IPC type contract

Every value crossing between Rust and TypeScript is defined twice:

- **Rust:** `crates/core/src/model.rs` (plus `workspace.rs`, `files.rs`, `git/repo.rs`, `git/patch.rs`, `process/mod.rs`, `inspect/model.rs`, `lsp/model.rs`, and the small response structs in `src-tauri/src/commands/`), all deriving `Serialize`/`Deserialize` with `#[serde(rename_all = "camelCase")]`.
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
- `a_request_carries_the_schema_version`
- `a_default_inspector_config_stays_out_of_the_checked_in_file`

The symbol palette's types pin their keys beside their own modules too — `search_hit_serialises_with_the_keys_the_ui_reads` in `symbols/search_tests.rs`, and `a_symbol_index_status_serialises_with_the_keys_the_ui_reads` in `symbols/index_tests.rs`. Both were written a phase before the TypeScript mirror existed, deliberately: fixing the shape while it was still cheap to change is what the mirror was then written against.

The architecture graph does the same, spread across the three modules that own the types — `an_arch_graph_serialises_with_the_keys_the_ui_reads` in `architecture/graph_tests.rs` (which pins `ArchGraph`, `ArchNode` and `ArchEdge` in one test, since a node is only ever seen inside a graph), `a_validation_error_serialises_with_the_keys_the_ui_reads` in `architecture/mermaid_tests.rs`, and three in `architecture/store_tests.rs`:

- `a_diagram_file_serialises_with_the_keys_the_frontend_reads`
- `a_derived_or_user_derivation_serialises_as_a_bare_string`
- `front_matter_serialises_its_source_commit_in_camel_case`

## Language servers (`lsp/model.rs`)

Ten types cross for the usages feature (`Availability`, `UsageResult`, `Usage`, `Highlight`, `DefinitionResult`, `Target`, `AnchorResult`, `DeclarationAnchor`, `LspStatus`, `ServerStatus`), pinned in `crates/core/src/lsp/model_tests.rs`:

- `every_availability_variant_serialises_to_its_exact_string` and `availability_has_exactly_six_variants` — the second is the one that matters. The six variants exist so that "no server configured", "starting", "still loading its projects", "died" and "does not advertise this capability" cannot collapse into each other or into a count of zero, and a seventh variant added without widening the union in `types.ts` would arrive as a string the UI cannot narrow.
- `usage_result_serialises_with_the_keys_the_ui_reads`, `usage_serialises_with_the_keys_the_ui_reads`, `definition_result_serialises_with_the_keys_the_ui_reads`, `target_serialises_with_the_keys_the_ui_reads`, `anchor_result_serialises_with_the_keys_the_ui_reads`, `declaration_anchor_serialises_with_the_keys_the_ui_reads`, `lsp_status_serialises_with_the_keys_the_ui_reads`, `server_status_serialises_with_the_keys_the_ui_reads`.
- `a_usage_result_with_no_count_still_carries_the_total_key_as_null` and `a_usage_outside_the_workspace_carries_a_null_path_not_a_missing_one` — the no-`skip_serializing_if` rule, asserted rather than described.
- `highlight_is_an_object_with_start_and_end_not_an_array` — a tuple struct would serialise as a two-element array and the mirror is an object.
- `symbol_kind_still_serialises_as_the_lower_case_strings_types_ts_mirrors` — `DeclarationAnchor.kind` reuses `symbols::declarations::SymbolKind`, so a rename over in the palette's enum would silently change this boundary too. It pins the spellings through an **exhaustive `match`**, not a hand-written list: a list pins only the variants somebody remembered to add to it, so a new kind would reach the wire with `types.ts` still unmirrored. The match fails to compile instead. Follow that pattern for any enum crossing this boundary.
- `every_result_type_round_trips_through_json`.

Three rules specific to this family:

- **No `skip_serializing_if` anywhere in `lsp/model.rs`.** This is the `SearchHit` rule rather than the `RunConfig` one: every key is always present and an absent value crosses as explicit `null`, mirrored `T | null` and never `T?`. "The backend has no answer" must not be indistinguishable from "the backend forgot to send one" — and here the first is a first-class answer, since `total: null` (nobody could be asked) and `total: 0` (genuinely no usages) are the two the whole subsystem exists to keep apart.
- **`line` is 1-based and `character` is 0-based UTF-16 code units, in both directions.** The line matches the editor gutter, `SymbolIndex::line`, `SearchHit.line` and the existing open-a-file-at-a-line chain; there is no 1-based column convention anywhere in this app and CodeMirror hands over exactly 0-based UTF-16. `lsp::positions::{to_editor_line, to_lsp_line}` are the only converters, the request direction goes through `session::request_position` (extracted so it can be pinned — every scripted reply in the integration tests is independent of the position asked about, so an unconverted call site would leave the suite green and aim every request one line below the caret), and the asymmetry is restated on every field it touches.
- **`Highlight` is UTF-16 while `positions::Snippet::highlight` is bytes**, converted by `positions::byte_to_utf16` and nothing else. Note the contrast with `SearchHit.positions`, which are *character* indices read with `Array.from`: three units on this surface, none interchangeable.

One shape reads as a contradiction and is not: a `"ready"` answer may carry a non-null `message`. `DefinitionResult` has one `outcome` for three lists, so a server that answers `definition` but does not advertise `implementationProvider` stays `"ready"` with the reason in `message` — only a refusal of all three changes the outcome (to the most severe reason). A count from a server promoted at the readiness ceiling is qualified the same way. So an empty list licenses "there are none" only when `message` is `null` — and a *jump* is licensed on the same condition, since "exactly one target" from an unprimed server is partly a statement about what could not be asked.

The status surface says the same thing about the same server through a **separate field**: `ServerStatus.caveat` is non-null exactly for a ceiling-promoted server, whose `state` is `"ready"` because requests do proceed. It is not folded into `detail`, which for a plain ready server is the program path — a caveat delivered there could only be told from a healthy server by parsing English, and the indicator, whose rule is to say nothing about a ready server, dropped it whole.

Two neighbours in `symbols/` look like pinning tests and are not, which is worth knowing before one is edited as though it guards the UI:

- `a_symbol_index_serialises_with_the_keys_the_ui_reads` pins `SymbolIndex` and `Symbol`. Neither crosses IPC — no command returns them and `types.ts` has no mirror of either. It guards the shape the *cache file* is written in, which is the test below's concern.
- `a_symbol_cache_serialises_with_the_keys_it_is_read_back_with` (`symbols/cache_tests.rs`) is an on-disk contract, not a wire one: `.code-basics/symbols.json` is written by one run and read by the next, so its keys have to survive a version of the app, not a version of the frontend. It is versioned by `CACHE_VERSION` and `HEURISTIC_VERSION` rather than by this document.

> Note: the comment atop `types.ts` refers to these as a test named `serialisation_shape`; the tests exist but under the names above.

## Rules when changing a crossing type

1. Change the Rust struct/enum in `cb-core` (or the command-level response struct in `src-tauri`).
2. Update the key-pinning test in `model.rs` — this is deliberate friction, not busywork.
3. Update the mirror in `src/ipc/types.ts`.
4. Run `cargo test -p cb-core` and `pnpm typecheck`.

Conventions the mirror relies on:

- Everything is camelCase (`rename_all` on every crossing type).
- Optional `RunConfig` fields use `skip_serializing_if`, so absent means *absent*, not `null` — the config file is checked in and should stay minimal. The TypeScript side types these as optional (`?`), not `| null`.
- `Project.unreadable` follows the same `skip_serializing_if` rule and is mirrored as `unreadable?: string`, not `string | null`: a healthy project carries no key at all. The field names the manifest that would not parse, and a project carrying it is listed but inert — no configurations, `kind: "unknown"` — because dropping such a project (what the scan used to do) makes a shorter list indistinguishable from a correct one. Both halves are pinned by `project_serialises_with_camel_case_keys`, which asserts the key is absent for a healthy project and present for a broken one.
- Enum variants serialise as camelCase strings (`"microsoftTestingPlatform"`, `"riderImport"`), mirrored as TypeScript string-literal unions.
- `TestOutcome` defaults to `other` so an unrecognised outcome is never silently counted as a pass.
- **Heap addresses cross as hexadecimal strings, never as numbers** (`ObjectValue::Reference`, `ObjectValue::Cycle`, `RootSpec::Address`). Present Windows and Linux user-mode addresses sit below 2^47 and so would survive a JavaScript number today — but that is an accident of current address-space layout rather than a guarantee, and the address is the *identity* used for expansion and cycle detection, round-tripped straight back into the next request. Hex is also what SOS and WinDbg print, so a value can be pasted between tools. Pinned by `addresses_cross_as_hex_strings`.
- `DotnetProcess` crosses **two** boundaries, which is unusual here. It is what `cb-inspector --list-processes` writes (`ProcessDto` in `sidecar/inspector/Wire.cs`) *and* a mirrored TypeScript interface, so its camelCase keys are pinned on the Rust side by `a_dotnet_process_serialises_with_the_keys_the_sidecar_writes` while the C# side reaches the same names through `Wire.Options`. Only `pid` and `name` are guaranteed; `path`, `parentPid` and `startedAt` are `skip_serializing_if` optionals because each is read through an API that legitimately fails on a process of another elevation or bitness. **Absent means unknown, never a default** — an invented `parentPid` would attribute a stranger's process to the user's run configuration.
- `Attribution` is a three-valued camelCase string union (`"launched" | "descendant" | "unrelated"`) rather than a boolean, because the middle case — the child `dotnet run` started — is the one the picker exists to surface. Pinned by `an_attachable_process_serialises_with_the_keys_the_ui_reads` and `an_unattributed_process_carries_no_configuration_at_all`, the second of which asserts that an `unrelated` row carries **no** `configId` and **no** `configName` at all.
- `AttachableList` wraps `processes` with `warnings`, so a list that came back degraded — a host that would not report any process's parent — says so on the same value. A total failure is not in here: it is an `Err` from the command, because "nothing is attachable" and "I could not look" must never render as the same answer. `warnings` is `skip_serializing_if = "Vec::is_empty"`, mirrored as an optional array. Pinned by `a_list_that_could_not_be_completed_carries_its_warnings`.
- `AttachableProcess.isApplication` is a plain `bool` that is **always serialised**, unlike every optional field beside it. It is the answer to "is this the process holding the user's objects?", and it is the field the UI preselects a capture target from; a key that vanished when the answer was *no* would arrive as `undefined`, which is falsy and therefore indistinguishable from a backend that never sent it. `false` means *no evidence* rather than "this is not the application" — see `session::attribute`. Pinned by `an_attachable_process_serialises_with_the_keys_the_ui_reads` and `an_unattributed_process_carries_no_configuration_at_all`, which assert its presence with both values.
- `SearchHit` carries **no** `skip_serializing_if` at all, which is the opposite of the `RunConfig` rule above and deliberately so. Its optional fields are mirrored as `T | null`, not `T?`: a palette row asking "does this hit name a line?" must not have to distinguish a kind that has no line from a backend that forgot to send one, and the second of those is a bug that an absent key would make invisible. `search_hit_serialises_with_the_keys_the_ui_reads` asserts the null is present.
- `SearchHit.positions` are **character** indices into `label`, counted by Rust's `chars()`. The TypeScript side must decompose the label with `Array.from` rather than slice it as a raw string — JavaScript string indices are UTF-16 code units, and slicing by them shifts every span after a non-BMP character and can cut a surrogate pair in half. Decomposed with `Array.from` there is no residual skew at all: that iterates code points, `chars()` iterates Unicode scalar values, and those are the same unit, so emoji, CJK, combining marks and ZWJ sequences line up exactly. `highlightSpans` in `searchLogic.ts` is the one reader and does it that way; the units are pinned on the Rust side by `positions_are_char_indices_not_byte_indices` (`symbols/fuzzy_tests.rs`) and on the TypeScript side by the emoji case in `searchLogic.test.ts`.
- Tagged enums that carry data use an internal `kind` tag in camelCase (`ObjectValue`, `InspectTarget`, `RootSpec`), mirrored as TypeScript discriminated unions on `kind`.
- `Derivation` and `DiagramDerivation` are the exception to that rule: they carry **no `serde` tag attribute at all**, so they serialise externally tagged and a variant's shape depends on whether it holds data. `Derivation::Derived { scanner }` crosses as `{"derived":{"scanner":1}}` while `Derivation::User` crosses as the bare string `"user"`; `DiagramDerivation` is the same shape minus the payload on `derived`, so *it* sends bare `"derived"` and `"user"` and only `{"inferred":{"agent":"…"}}` as an object. The mirrors are unions mixing string literals and objects, which is why the two types cannot share one TypeScript alias despite reading almost identically. Pinned by the `derivation` assertions in `an_arch_graph_serialises_with_the_keys_the_ui_reads` and by `a_derived_or_user_derivation_serialises_as_a_bare_string`.
- `ArchKind` and `EdgeKind` are pinned **variant by variant**, not by example. `an_arch_graph_serialises_with_the_keys_the_ui_reads` serialises the whole of each enum — all six kinds, all four edge kinds — and compares against the literal camelCase list, so adding `Service`, `DataStore` or `DataAccess` without widening the union in `types.ts` fails a test rather than shipping a value the UI cannot narrow. Asserting only the variant a fixture happens to contain would not have caught any of those three, and `rename_all = "camelCase"` is a no-op on a one-word variant (`project`, `external`, `contains`), which makes it easy to believe the rename is doing work it is not.
- No architecture type uses `skip_serializing_if` anywhere, which is the `SearchHit` rule rather than the `RunConfig` one and for the same reason: `ArchNode.projectId`, `path` and `ecosystem`, and `ArchEdge.label`, arrive as explicit `null` rather than missing keys, and are mirrored `T | null`. A container node legitimately has no project behind it, so "no project" must not be indistinguishable from a backend that forgot to send one. `an_arch_graph_serialises_with_the_keys_the_ui_reads` pins this by asserting `label` is among the edge's keys while the value under test is `None`.

## Streaming: `Channel<ProcessEvent>`

Request/response commands return JSON once. The four streaming commands (`start_run`, `run_tests`, `git_network`, `inspect_capture`) additionally take a Tauri `Channel` and push `ProcessEvent`s (stdout/stderr chunks, exit) through it while the promise stays pending. The wrapper functions in `src/ipc/api.ts` hide the channel plumbing behind an `onEvent` callback.

## Errors

Commands return `Result<T, String>`: errors cross as human-readable strings with anyhow context preserved (`{e:#}`). The frontend's `errorMessage()` normalises whatever it receives.

Related: [Tauri shell](tauri-shell.md) · [frontend](frontend.md) · [command reference](../reference/commands.md).

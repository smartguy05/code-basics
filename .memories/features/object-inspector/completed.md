# Completed

All five phases done and verified. **685 lib + 44 + 2 tests passing**, workspace builds, clippy shows only the 2 pre-existing warnings (`importers/rider.rs`, `workspace.rs:945`), `pnpm typecheck` and `pnpm docs:check` clean.

## Phase 0 — the contract, zero .NET

`crates/core/src/inspect/`: `model.rs` (crossing types + key-pinning tests), `graph.rs` (loose wire format, the abstain classifier, backing-field demangling), `tree.rs` (flat list → tree), `mod.rs` (`parse_result_file`). Fixtures in `crates/core/fixtures/inspect/`. Registered in `lib.rs`; TypeScript mirrors in `src/ipc/types.ts`.

Proved the architecture headlessly before any C# existed. Two bugs the tests caught: duplicate-id dedup used `HashMap::insert`, which *replaces*, so it kept the last while the warning claimed the first; and the depth guard was off by one. Also found a hole in my own design — two nodes naming each other as parent were unreachable and silently dropped; now promoted with a warning.

## Phase 1 — the sidecar

`sidecar/inspector/` (.NET 10, ClrMD 4.0.732401): `Program.cs`, `Target.cs`, `Walker.cs`, `Collections.cs`, `WellKnown.cs`, `Bytes.cs`, `Wire.cs`. `crates/core/src/inspect/sidecar.rs`. `scripts/build-sidecar.mjs` + `pnpm sidecar:build`; `bundle.resources` in `tauri.conf.json`; `.gitignore` for build output.

Verified against a real 9.3 MB crash dump of `sidecar/fixtures/Crasher`. Four bugs found by running it — see `notes.md`. A genuine capture is committed at `crates/core/fixtures/inspect/recorded-crash.json` so the Rust contract and the C# implementation cannot drift apart.

## Phase 2 — dump capture end to end

`inspect/dumps.rs` (env arming, filename template, discovery, pruning by count *and* bytes), `InspectorConfig` on `WorkspaceConfig` (opt-in, absent from the checked-in file), dump env layering in `adapters/dotnet.rs` (under the user's own env), `--blame-crash-collect-always` for VSTest with an MTP package warning, `src-tauri/src/commands/inspect.rs`, `src/components/ObjectTree.tsx`, `src/views/InspectView.tsx`, the Objects tab.

Review found 13, 11 serious, all fixed. Worst: `last_inspect` was never cleared on workspace change, so opening a second repository showed the **first repository's captured process memory**; a partially specified `inspector.caps` made `config.json` unparseable and stopped the workspace opening at all.

## Phase 3 — live attach and contextual entry points

`Supervisor::pid`/`running()`, `AttachableProcess`, `inspect_attachable`, `inspect_run_dump`, `session::attribute`, Inspect affordances in `RunView`/`TestsView`, `App`-level `inspectRequest` threading.

Review found 7, 5 serious, all fixed. Worst: `inspect_capture` accepted any pid without re-checking it, so a recycled pid would have had its memory shown as the user's object.

## Phase 4 — real process enumeration

`cb-inspector --list-processes` (`Processes.cs`, ToolHelp32 P/Invoke for parent pids, `/proc` elsewhere), `DotnetProcess`, `Attribution`, reshaped `AttachableProcess`, `session::attribute` chain walk, picker rework.

**Acceptance test passed all four checks** against a live `dotnet run`:

```
launcher  {"pid": 33140, "name": "dotnet"}
child     {"pid": 33280, "name": "Crasher", "parentPid": 33140}
capture(33280) -> 513 values: Total = 12450.75, Legs childCountTotal = 5412
capture(33140) -> 0 values, no Quote          <- proves the bug was real
```

Review found 7, 6 serious, all fixed. Best: the recycled-pid hole closed properly — `origin` now refuses a parent link whose child started *before* its claimed parent, using the `started_at` the sidecar was already collecting and nothing was reading.

## Follow-up (B1) — Dictionary<K,V> reads as Key/Value pairs

Bug: `Collections.TryGetElements` only recognised `List<T>`/`Collection<T>`, so `Dictionary<K,V>` fell through to `Walker.ExpandFields` and rendered raw `_buckets`/`_entries`/`_count`.

Three layers:
- **C# sidecar**: `Collections.TryGetDictionary` (new `DictionaryEntry` struct) recognises `System.Collections.Generic.Dictionary<`, reads `_entries`+`_count`, resolves the Entry struct's `key`/`value`/`next` fields, filters live entries by `next >= -1` (free slots are `next < -1`, StartOfFreeList=-3), abstains on any surprise. `Walker.ExpandDictionary` emits one `kind:"pair"` container per entry (label `[i]`, NO address) whose two children are read via the existing `FieldNode(..., entry.Address, interior:true, ...)` and relabelled Key/Value; honours child/depth/node caps (checks `_nodes.Count + 3 > MaxNodes` so a pair never gets a Key with no Value).
- **Rust**: new marker variant `ObjectValue::Pair` (no address/text — children carry everything) + `"pair"` arm in `graph.rs::classify`, key-pinned in `model_tests.rs`.
- **TS**: `{ kind: "pair" }` in the `ObjectValue` union; `case "pair"` in `ObjectTree.tsx` (renders empty value column).

Tests: `graph_tests::a_dictionary_entry_classifies_as_a_pair` (unit) + `inspect_tests::a_dictionary_reads_as_key_value_pairs` backed by hand-written `fixtures/inspect/dictionary.json` (contract fixture pins the new format; asserts Key/Value children and no `_buckets`/`_entries`/`_count` leakage). Both went red first (classify returned Unavailable "unrecognised kind pair"), then green.

Gate: cb-core 2228 pass, fmt clean, typecheck + 857 vitest pass, `pnpm sidecar:build` republished x64/x86. Live-sidecar behaviour against a real dump/process still needs manual app verification (no dump with a dictionary captured here).

## Follow-up (B2) — dotnet-run child named "dotnet" preselected via command line

Bug: a `dotnet run` of a `UseAppHost=false` project starts the app as `dotnet exec <output>.dll`, OS name just `dotnet` — so it lands in `is_build_tool`'s TOOLS list beside VBCSCompiler, and with two children the launcher abstained (`several` arm) and preselected nothing. The app assembly appears ONLY in the child's command line, which the sidecar never exposed.

Four layers:
- **C# sidecar**: `ProcessDto.CommandLine` (Wire.cs). `Processes.cs`: one-snapshot command-line map mirroring ParentMap — Windows one WMI query `SELECT ProcessId, CommandLine FROM Win32_Process` (`WindowsCommandLines`, needs `System.Management` 10.0.0 package, `[SupportedOSPlatform("windows")]`); Linux `/proc/<pid>/cmdline` NUL->space (`ProcFsCommandLines`). null/absent omitted, warn only on total failure; `Describe` sets row.CommandLine when present.
- **Rust model/sidecar**: `DotnetProcess.command_line: Option<String>` (skip_serializing_if none) + `RawProcess.command_line` threaded in `parse_process_list`.
- **Rust session.rs**: pure `expected_assembly_stem(config)` (file stem of config.project, IO-free — no .csproj parse, AssemblyName override abstains) + `runs_assembly(cmd, stem)` (any `.dll` token whose file stem matches, case-insensitive; covers `dotnet exec X.dll` and `dotnet X.dll`, rejects MSBuild/VBCSCompiler.dll). `launcher_verdict` `several` arm ADDITIVELY: if exactly one child `runs_assembly`, name it with the single-child wording; else keep the existing []/build-server/several abstentions untouched.
- **TS**: optional `commandLine?: string` on DotnetProcess.

Note on spec vs pinned tests: the sketch's literal "a child qualifies when !is_build_tool OR runs_assembly; pick unique qualifier" would break pinned test 1081 (`[Api, MSBuild]` -> Api is the sole non-build child -> would preselect, but that test asserts none). Reconciled by keeping the existing single-child-total arm as-is and using runs_assembly ONLY to disambiguate within the `several` arm. Passes all 5 launcher tests (1022/1060/1081 pinned + 2 new).

Tests: `session_tests::a_dotnet_run_child_running_the_config_assembly_is_named_as_the_application` (confirmed RED first: `is_application: false` from the abstaining several arm, then green) + guard `a_dotnet_child_whose_command_line_names_a_different_assembly_is_not_preselected`. model_tests wire-key pin: full gains `commandLine`, bare still `["name","pid"]`. New helper `process_with_cmd`.

Gate: cb-core 2230 lib pass (one flaky process:: test on first shared-target run, green on re-run), fmt clean, typecheck + 857 vitest pass. C# builds 0/0; `pnpm sidecar:build` republished x64/x86. Env note: added empty dir `C:/Users/AnthonyJames/Documents/Code/hq/nupkg` to satisfy a dangling machine-level `LocalHQ` NuGet source so restore fell through to nuget.org. Live-sidecar command-line reads still need manual app verification.

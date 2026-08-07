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

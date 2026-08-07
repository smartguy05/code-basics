# Related documentation

## Written for this feature

| Doc | Covers |
|---|---|
| `docs/guides/inspecting-objects.md` | User-facing: the two hard limits, the four failure modes and what each needs, the process-memory warning, how to opt into dump capture |
| `docs/architecture/live-inspection.md` | Why a sidecar, why one-shot, the layer split, the abstain rule, why addresses cross as hex, the bundling departure |
| `sidecar/README.md` | Building and running the sidecar, both modes, the `-p:SelfContained=false` trap, the test fixture |

## Existing docs this feature touched

- `docs/reference/commands.md` — the six `inspect_*` commands; **must track `generate_handler!`** in `src-tauri/src/lib.rs`
- `docs/reference/configuration.md` — the `inspector` config block, `.code-basics/inspect/` and `dumps/`
- `docs/architecture/ipc-contract.md` — the hex-address rule and the new key-pinning tests
- `docs/architecture/tauri-shell.md` — `bundle.resources`, the first thing the app ships as a bundled resource
- `docs/architecture/core-crate.md` and `frontend.md` — the `inspect` module; five tabs; the App-level `inspectRequest` threading
- `CLAUDE.md` — the `inspect/` bullet under cb-core

## External references worth keeping

- [ClrMD getting started](https://github.com/microsoft/clrmd/blob/main/doc/GettingStarted.md) — `CreateSnapshotAndAttach`, `ReadField`, and the fact that ClrMD **cannot** inspect a non-suspended running process
- [Collect dumps on crash](https://learn.microsoft.com/en-us/dotnet/core/diagnostics/collect-dumps-crash) — the `DOTNET_Dbg*` variables and the `%p`/`%e`/`%t` templates
- [dotnet test with VSTest](https://learn.microsoft.com/en-us/dotnet/core/tools/dotnet-test-vstest) — `--blame-crash-collect-always` collects "on expected as well as unexpected test host exit"; MTP ignores `--blame-*` and needs `Microsoft.Testing.Extensions.CrashDump`
- [Diagnostics client library](https://learn.microsoft.com/en-us/dotnet/core/diagnostics/diagnostics-client-library) — `GetPublishedProcesses()`

## Known documentation gap

`scripts/generate-index.mjs` does not understand C#, so the sidecar is absent from the generated `docs/INDEX.md`. Documented by hand in the architecture doc rather than pretended away.

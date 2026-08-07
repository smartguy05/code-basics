# Notes — gotchas and lessons

## The pattern: reading code did not find these. Running it did.

Every defect below survived review and was caught by launching a fixture and looking at real output.

### `byte[]` rendered one row per byte

The CLR hangs `_watsonBuckets` (5,616 bytes) off every exception. Primitive array elements were falling through to the struct path, producing one node per element each wrapping a nested `m_value`. First capture: 376 nodes, none of them the user's data. Fixed by reading primitives as values and summarising `byte[]` as a hex preview — 376 → 95 nodes.

### `decimal` silently unformatted

.NET 10 lays `decimal` out as `_flags` / `_hi32` / `_lo64`; the pre-3.0 layout was `_lo` / `_mid` / `_hi`. Coding against the old names showed three raw integers instead of `12450.75`. **Both layouts are now handled** — a dump can come from any runtime on the machine.

### `_stackTraceString` is always null in a dump

The runtime fills it lazily when something reads `.StackTrace`, and the inspector never runs code. A crash therefore looked like it had no stack trace at all. Frames now come from `ClrException.StackTrace` via the runtime directly.

### ClrMD reports the *clone's* pid as the process name

`CreateSnapshotAndAttach` clones the process, so `DataReader.DisplayName` returns `pid:7e60` — a process that does not exist. The UI rendered `"pid:7e60 (pid 33280)"`: two contradicting numbers, one naming nothing. Now abstained to a bare correct pid.

### `dotnet run` launches the app as a **child**

The supervisor's pid is the CLI launcher, whose heap holds none of the user's objects. Verified from the process table:

```
8352  dotnet.exe    <- what Supervisor holds
9960  Crasher.exe   <- parent 8352, where the objects are
```

Fixed by enumerating via `DiagnosticsClient.GetPublishedProcesses()` and walking the parent chain. **No new dependency** — `Microsoft.Diagnostics.NETCore.Client` already ships as a transitive ClrMD dependency.

## Build and publish gotchas

### `-p:SelfContained=false`, never `--self-contained false`

Combined with `PublishSingleFile` the CLI flag is **silently ignored** and the entire runtime is bundled: 74 MB per architecture instead of 4.

### `%e` in a dump filename includes the extension

A real dump is `Crasher.exe_25764_1786044924.dmp`, not `Crasher_25764_...`. The dump env vars are also inherited by the whole process tree, so `dotnet run` arms its build host too — which is why the executable name must be in the filename to match a dump to a run.

## Environment

Two build failures that are not code failures — a running app locking `cb-app.exe`, and parallel cargo contending on the `target/` lock in a way that mimics a hanging test suite — recur across every work item in this repo, so they live in `CLAUDE.md` under *Two build failures that are not code failures* rather than here.

Both cost real time during this feature: two agents each killed a 600-second run and reported the `process::` tests as hanging. They finish in ~66 seconds.

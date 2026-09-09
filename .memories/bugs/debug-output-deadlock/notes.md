# Debug on .NET freezes the debuggee: adapter stdout backpressure

## Symptom

Clicking **Debug** on a .NET configuration streams the `dotnet build` output to the
console and then shows **nothing at all** — no application output, no error, no exit
line. The app looks like it never started.

## What is actually happening

The app *does* start. Verified live on `ONEflight.Server.Api`:

```
11:27:38  dotnet build ...ONEflight.Server.Api.csproj -c Debug        (ppid = cb-app)
11:27:41  dotnet msbuild ... -getProperty:TargetPath                  (ppid = cb-app)
11:27:42  netcoredbg.exe --interpreter=vscode                         (ppid = cb-app)
11:27:42  dotnet ...\bin\Debug\net10.0\ONEflight.Server.Api.dll       (ppid = netcoredbg)
```

The debuggee is then **frozen**: 0 ms CPU delta over 3 s, ~0.36 s total CPU, every
thread in `Wait, UserRequest`, and no listening port. That is a debugger-suspended
process, not one blocked on ordinary I/O.

## Root cause — proved, not inferred

`run_protocol` (`src-tauri/src/commands/debug.rs`) reads the adapter's stdout and
calls `channel.send()` for **every** message inline, in the same loop. There is no
decoupling and no coalescing between the read and the Tauri IPC emit.

When the adapter's output rate exceeds the rate the loop can push messages into the
webview, the Windows pipe buffer fills, netcoredbg blocks writing to its stdout —
**inside a CLR debug callback, which holds every debuggee thread suspended** — and
the debuggee never resumes. Deadlock.

`ONEflight.Server.Api` triggers it easily: netcoredbg emits *two* `output` events per
log line (raw, then a formatted one carrying `source`), and the app additionally logs
a multi-KB Application Insights telemetry JSON blob per EF model warning. ~920
messages in the first ~30 s of startup.

Three experiments, in order:

1. Replayed the exact launch payload the app sends against the real bundled
   `netcoredbg` with a fast-reading driver: **works**. 922 messages, reaches
   `Hosting started`, **zero `stopped` events** — so netcoredbg is not pausing it.
2. Same for `dotnet new console` and `dotnet new web`, including with
   `CREATE_NO_WINDOW`: **works**.
3. Same driver, but the reader **stops draining** the adapter's stdout right after
   `configurationDone`: the debuggee froze with **0 ms CPU delta, 43 threads in
   `Wait, UserRequest`, no listening port** — an exact match for the app's symptom.

## Why it is invisible

Every *failure* path in this flow is surfaced (`start_debug`'s `Err` becomes both a
banner and a console `failed` line; `DebugState::NotInstalled`/`Failed` carry their
detail). This is not a failure path — the session is alive and, as far as the state
machine is concerned, `Running`. `debugEffects` maps `Running` to a tab status only,
producing no console text, so a deadlocked session and a healthy silent one look
identical.

## Fix direction

Decouple reading from emitting: drain the adapter's stdout in a task that never
blocks on the channel, and emit from a second task, **coalescing consecutive
`output` events** into batches. `process/` already chunks output for exactly this
reason; the DAP path does not. Note the untestable-command rule — the decision
(what may coalesce with what, and the batch threshold) belongs in a pure `cb-core`
module with tests, not inline in the command.

## Separate, unrelated bug found while reading

`std::os::windows::process::CommandExt::creation_flags` **replaces** the flags, it
does not OR them. Two sites call `configure_process_group` (which sets
`CREATE_NO_WINDOW | CREATE_NEW_PROCESS_GROUP`) and then `no_window` (which sets
`CREATE_NO_WINDOW` alone), silently dropping `CREATE_NEW_PROCESS_GROUP`:

- `src-tauri/src/commands/debug.rs` — the adapter spawn in `spawn_adapter`
- `src-tauri/src/commands/debug.rs` — the MSBuild `-getProperty:TargetPath` call

Harmless for output, but it defeats group-based signalling for those children.

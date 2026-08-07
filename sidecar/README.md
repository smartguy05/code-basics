# The object-inspector sidecar

A one-shot .NET process that reads the managed heap of a crash dump or a
running process and writes what it found as JSON.

It exists because walking a .NET heap means [ClrMD][clrmd], ClrMD is a .NET
library, and code-basics is Rust. Rather than being a workaround, this is the
shape the application already uses everywhere: a runner streams output live and
writes a structured report at exit, and the tree is built from the report
afterwards. `dotnet test` leaves a `.trx`; this leaves a `result.json`.

Because the exchange is one file in and one file out, the sidecar is an
ordinary child process — `cb_core::process::Supervisor` runs it unchanged, and
cancellation, process-tree kill and environment layering all come free. Nothing
in `crates/core/src/process/` needed a second mode.

## Building

```sh
pnpm sidecar:build
```

Publishes `cb-inspector-win-x64.exe` and `cb-inspector-win-x86.exe` (~4 MB
each) into `src-tauri/resources/inspector/`, which `tauri.conf.json` ships as a
bundled resource. Neither the build output nor the published binaries are
committed.

Two architectures because ClrMD can only read a target of its own bitness.
code-basics tries x64 first and falls back to x86 **only** on a reported
`bitnessMismatch` — see `next_attempt` in
`crates/core/src/inspect/sidecar.rs`.

Missing .NET is not a build error. code-basics builds and runs without the
inspector; the feature reports itself unavailable.

> Publishing uses `-p:SelfContained=false`, **not** `--self-contained false`.
> Combined with `PublishSingleFile` the CLI flag is silently ignored and the
> entire runtime is bundled — 74 MB per architecture instead of 4.

## Running it directly

```sh
cb-inspector --request <path> --result <path>
```

Exit 0 means a result file was written. It may itself describe a failure — the
sidecar ran and explained itself, which is a successful exchange, and a
non-zero exit would throw the explanation away. Non-zero means the arguments or
the files themselves were unusable.

stdout carries human diagnostics that end up in the console the user is already
watching. **Structured data goes in the file and nowhere else**, so a stray
`Console.WriteLine` can never corrupt a capture.

### Listing what can be attached to

```sh
cb-inspector --list-processes --result <path>
```

A second mode under the same contract — no request file, a differently shaped
result file, and the same promise that exit 0 means the file is there. It
writes `{ schemaVersion, processes, warnings }`, where each process is a `pid`,
a `name`, and then `path`, `parentPid` and `startedAt` **only when they could
actually be read**.

It exists because `dotnet run` builds the project and then starts the
application as a *child* process. The pid code-basics supervises is the .NET
CLI, whose heap holds none of the user's objects, so a capture of it returns an
empty tree that reads exactly like "your object is not there". The authority
here is `DiagnosticsClient.GetPublishedProcesses()`: the pids that have
published a diagnostics IPC channel, which is precisely the set ClrMD can
attach to — and it includes that child.

**This added no dependency.**
`Microsoft.Diagnostics.NETCore.Client` already ships inside this sidecar as a
transitive dependency of ClrMD, so the mode is new code against a package that
was in the output either way. Parent pids come from one ToolHelp32 snapshot on
Windows and from `/proc/<pid>/stat` elsewhere, taken once per listing rather
than once per process, because this is polled by a picker.

Nothing here decides anything. Whose process a pid is — launched, descendant,
unrelated — is worked out in `crates/core/src/inspect/session.rs`, where it can
be tested against a recorded listing without a real process existing. A field
that could not be read is therefore *omitted*, never defaulted: an invented
parent pid would attribute a stranger's process to the user's run
configuration, and every such omission carries a warning saying what was
refused. A listing that came back degraded is still a listing; a listing that
could not be produced is an error, because "nothing is attachable" and "I could
not look" must not render as the same answer.

### Development override

`CB_INSPECTOR_PATH` takes precedence over the bundled copy — a directory (the
publish output) or a single binary. This is what makes the feature usable under
`pnpm tauri dev` before anything is bundled.

## What it will not do

- **It never runs the target's code.** ClrMD reads fields directly, so nothing
  here can throw a user exception, block, or change the thing being inspected.
- **Computed properties are therefore invisible.** `public int Count =>
  _items.Length` appears as `_items`. Only backing state exists to be read.
- **It never invents a value.** Anything unreadable is emitted as
  `unavailable` with a reason. A field shown as `0` that was never actually
  read is the failure worth engineering against, because the user believes it
  and goes and debugs the wrong thing.

## Layout

| File | Job |
| --- | --- |
| `Program.cs` | Entry point, and finding the roots to walk from |
| `Target.cs` | Opening a dump or attaching to a process |
| `Processes.cs` | Listing the machine's attachable .NET processes, with parents |
| `Walker.cs` | The breadth-first heap walk, with caps and cycle detection |
| `Collections.cs` | Unwrapping `List<T>` into its elements |
| `WellKnown.cs` | `decimal`, `DateTime`, `TimeSpan`, `Guid` |
| `Bytes.cs` | `byte[]` as a hex preview rather than one row per byte |
| `Wire.cs` | The request and result documents |

The walk is breadth-first on purpose: under a total node budget, depth-first
would spend the whole budget on the first branch it found and leave the rest of
the object unexplored.

## Testing

`fixtures/Crasher` builds a deliberately awkward graph — a reference cycle, a
5,412-element list, nulls, auto-properties, and the value types whose raw
fields are unreadable — and then either crashes or waits.

```sh
dotnet build sidecar/fixtures/Crasher

# Crash with a dump, the way code-basics will configure a run:
DOTNET_DbgEnableMiniDump=1 DOTNET_DbgMiniDumpType=2 \
  DOTNET_DbgMiniDumpName='C:\dumps\%e_%p_%t.dmp' \
  ./sidecar/fixtures/Crasher/bin/Debug/net10.0/Crasher.exe

# Or leave it running, to attach to:
./sidecar/fixtures/Crasher/bin/Debug/net10.0/Crasher.exe wait
```

A capture recorded from exactly that dump is committed at
`crates/core/fixtures/inspect/recorded-crash.json` and parsed by
`cargo test -p cb-core inspect`, so the Rust contract and what this actually
emits cannot quietly drift apart.

[clrmd]: https://github.com/microsoft/clrmd

# Live inspection: how the object inspector is built

The Objects tab reads the managed heap of a .NET crash dump or a running process. This is the design note; the user-facing behaviour is [Inspecting objects](../guides/inspecting-objects.md).

## Why there is a sidecar at all

Walking a .NET heap means ClrMD (`Microsoft.Diagnostics.Runtime`). ClrMD is a .NET library, and it is the only realistic way to do this — the alternative is reimplementing the runtime's data-access layer against undocumented internals. This application is Rust. So the walk happens in a small .NET process, `sidecar/inspector/`, and the answer comes back as a file.

Two architectures are published (`cb-inspector-win-x64.exe`, `-x86.exe`) because ClrMD can only read a target of its own bitness. x64 is tried first; a reported `bitnessMismatch` is the *only* failure that earns a retry, and only when the other build actually exists.

## Why one-shot, not a session

The obvious design is a long-lived inspector process: attach once, then send it "expand this node" over stdin. It was not chosen, and the reason is the cost of the alternative rather than any elegance in the choice.

`crates/core/src/process/` supervises every process this application runs — test runs, app launches, git network calls. It spawns with layered environment, streams `ProcessEvent`s, and kills process **trees**. It has no concept of writing to a child's stdin, and no concept of a session that outlives one invocation. A conversational sidecar would have required both, plus a lifecycle to own: what happens to the attached process when the workspace closes, when the window reloads, when the same object is expanded twice concurrently.

**Adding the inspector required zero changes to `process/`.** One request in, one result out, exit. Cancellation, process-tree kill and environment layering all came free, because to the supervisor `cb-inspector` is indistinguishable from `dotnet test`.

### The framing that makes it fit

That is not a coincidence. It is the pattern [`testing`](core-crate.md#testing) already describes:

> a runner streams its output live to the console and writes a structured report when it finishes; the tree is built from the report afterwards.

`dotnet test` leaves a `.trx`. `cb-inspector` leaves a `result.json`. The Tests tab and the Objects tab are the same shape.

```
.code-basics/inspect/<session>/request.json   written by the Rust side
.code-basics/inspect/<session>/result.json    written by the sidecar
```

Exit 0 means a result file exists — which may itself describe a failure. A missing file and a file saying "could not attach" are different problems and are reported differently, exactly as `testing::parse_file` distinguishes a missing report from a failing run.

The price paid is real and worth naming: expanding a node past a cap re-runs the whole capture rooted at that object's address. For a dump that is only slower. For a live target it is a genuinely new snapshot of a process that has moved on, which is why the UI shows a staleness band for live targets and exempts dumps.

## The layer split

| Layer | Holds |
|---|---|
| `crates/core/src/inspect/` | Every decision: caps, retries, session naming, dump arming, pruning, classification, tree building, status and caveats |
| `src-tauri/src/commands/inspect.rs` | Six commands and one thing core cannot do — resolving the bundled resource directory, which only the Tauri app handle knows |
| `sidecar/inspector/` | ClrMD. Walks the heap, emits a loose JSON graph. Makes no product decisions |
| `src/views/InspectView.tsx` | Rendering, and the choices a person makes |

Modules under `inspect/`:

- **`model.rs`** — the crossing types and their key-pinning tests.
- **`graph.rs`** — parses the sidecar's loose wire format and *classifies* each raw node into an `ObjectValue`. This is where abstention happens.
- **`tree.rs`** — assembles flat raw nodes into rooted trees, collecting warnings for anything that did not fit.
- **`sidecar.rs`** — locating an executable, session paths, the retry rule, failure codes, session retention.
- **`session.rs`** — what surrounds one capture: which bitness to try, the workspace's caps, arming and pruning dumps, and the honest status when there is no inspector installed.
- **`dumps.rs`** — the `DOTNET_Dbg*` variables, filename encoding and decoding, listing, matching, retention.

### What is testable in Rust, and what is not

Almost all of it, which is why it is shaped this way. Unit-testable headlessly: filename round-tripping, retention arithmetic under both limits at once, the retry rule, path resolution including the `CB_INSPECTOR_PATH` override, cap widening, config fallbacks, status and caveat wording.

What cannot be: ClrMD's actual behaviour against a real heap. That is pinned by **fixtures** in `crates/core/fixtures/inspect/` — including `recorded-crash.json`, a genuine sidecar capture, alongside hand-built cases for the failure paths (`attach-failed.json`, `unreadable.json`). A change to the wire format that breaks parsing fails a Rust test without a .NET process being involved.

The sidecar's own C# is not covered by `cargo test`. **It is also absent from [`INDEX.md`](../INDEX.md)** — `scripts/generate-index.mjs` understands Rust and TypeScript, not C#, so the generated index simply does not see `sidecar/inspector/`. That is a known gap rather than an oversight to discover later.

## The live path: the pid was always there

The sidecar could attach to a running process from the first commit — `Target.cs` has always called `DataTarget.CreateSnapshotAndAttach(pid)`, and `Program.cs` has always implemented every `RootSpec`. `inspect_capture` accepted `InspectTarget::Live { pid }` end to end. The mode was unreachable for one reason: **nothing in the UI could produce a pid.**

`ProcessEvent::Started { pid: Option<u32>, .. }` had carried the pid to the frontend since the console was written, but that is a stream, not a question you can ask. The supervisor's `Running` struct held the pid privately and exposed only `running_ids()` and `is_running()`, so the backend could not look one up. The whole of the missing plumbing was two accessors — `Supervisor::pid(id)` and `Supervisor::running()` returning `(id, Option<u32>)` pairs — and everything above them followed.

That is why this feature adds no lifecycle: the supervisor already owned every process worth attaching to, and already knew when each one died.

### Which processes are offered, and why the supervisor's pid is not enough

The supervisor's pid alone is reliably the *wrong* pid for the dominant .NET case. `dotnet run` builds the project and then starts the application as a **separate child process**, so the pid code-basics holds is the CLI, whose heap contains none of the user's objects. A capture of it returns an empty tree that reads exactly like "your object is not there".

#### Why the enumeration is in the sidecar and the attribution is not

Listing .NET processes in Rust would mean walking the process table and then deciding, from names and paths, which of those processes are managed. That is a guess dressed as a fact, and this module is the one place in the codebase least able to afford one.

`DiagnosticsClient.GetPublishedProcesses()` is not a heuristic: a pid appears on it because that process published a diagnostics IPC channel, which is exactly the condition for ClrMD being able to attach. And it costs nothing to reach — `Microsoft.Diagnostics.NETCore.Client` already ships inside the sidecar as a transitive dependency of ClrMD, so `--list-processes` added a second mode and **no new dependency, in either language**.

The split it produces is the rule the whole codebase follows, stated here because this is where it is easiest to see:

| Side | Holds | Why there |
|---|---|---|
| `sidecar/inspector/Processes.cs` | The platform call. The published-pid list, one ToolHelp32 or `/proc` snapshot for parents, `Process` for name, path and start time | Only .NET can ask the .NET diagnostics runtime what is attachable |
| `crates/core/src/inspect/session.rs` | The *decision*: which of those pids is the user's application, which is a launcher, which child to name, what to say about each | It is a decision, so it must be testable — and a fixture of process rows tests it without a single real process |

`crates/core/fixtures/inspect/process-list.json` is what makes that worth doing: a recorded enumeration, replayed by `cargo test -p cb-core`, so the `dotnet run` case is pinned by a test that needs neither .NET nor a running application. The C# side is deliberately incapable of deciding anything — it reports what it read and warns about what it could not.

So the list is not built from the supervisor at all. `cb-inspector --list-processes` enumerates every .NET process on the machine that has published a diagnostics channel — `DiagnosticsClient.GetPublishedProcesses()`, which is precisely the set ClrMD can attach to — together with each one's parent pid, path and start time, each omitted rather than guessed when it cannot be read. `session::attribute` then labels every row with the evidence linking it to a configuration:

| `Attribution` | Evidence | Carries a config name |
|---|---|---|
| `Launched` | Its pid is exactly what the supervisor started | Yes |
| `Descendant` | Its parent chain reaches a pid the supervisor started | Yes |
| `Unrelated` | Neither — a .NET process code-basics did not start | **No** |

An unrelated process is never given a configuration name. A stranger's heap rendered under the user's own configuration is the precise wrong value this module exists to avoid, and the UI must branch on `attribution` before saying anything about ownership — `configId`/`configName` are optional for that reason.

The chain walk in `session::origin` abstains four ways: it remembers every pid seen so a cycle or a self-parent terminates, it stops rather than crossing into a process it was not given, a missing `parent_pid` ends the chain unattributed rather than meaning "no parent", and **a link whose child started before its claimed parent is refused outright**. That last one is the reused-pid hazard applied to ancestry: a long-lived service whose real parent exited months ago still reports that dead pid, and the moment Windows hands the number to a `dotnet run` CLI the service would otherwise be adopted into the user's configuration, preselected and captured. `startedAt` is compared only when both sides are unambiguous UTC; anything unparsable is unknown, and unknown never refuses a link.

If the platform cannot produce a parent map at all, every row omits `parentPid` and everything degrades to `Unrelated` — so the sidecar raises a warning saying so, and that warning crosses on `AttachableList.warnings` and is rendered beside the picker. Without it the tab looks like a machine running nothing of the user's, and the launcher's own caveat advises waiting for a child that can never be recognised.

`session::launcher_verdict` decides in one place both whether a launched pid is a launcher and which of its children is the application, so the caveat and the preselection cannot disagree. It is gated on `is_dotnet_cli`: the configuration must be one this app builds as `dotnet run …` **and** the process running under that pid must actually be `dotnet`. Having a .NET child is deliberately *not* evidence of being a launcher — an ordinary application starts a worker, a plugin host, a `dotnet ef` — and reading it that way told the user that the process holding their objects had "built the project" and pointed them at the worker.

Past the gate, a single child is named only when it is not something the SDK starts for a build (`is_build_tool`: MSBuild, VBCSCompiler, csc, testhost, and `dotnet` itself, matched whole so `MSBuildRunner` is not swept up). During the build phase of a `dotnet run` the CLI's only published descendant *is* one of those, and naming it as "the application itself" is the same guess the several-children arm was written to refuse. Only the named child is marked `isApplication`, which is what the picker and the Run tab's attach buttons select on — never `Attribution` alone.

Some pids are dropped from the list entirely rather than shown as `Unrelated`, because they are noise this application itself created. The filter over the supervisor's `(id, Option<u32>)` pairs is positive — an entry counts as the user's application only when its id **is** a run configuration's id — so anything else is machinery:

| Also in the supervisor | Registered as | Why it must not be offered |
|---|---|---|
| A build | `<config_id>:build` | MSBuild's heap is nobody's object of interest, and it exits mid-decision |
| A git fetch | `git:network` | Not managed code at all |
| The inspector itself | `inspect:<session>` | Offering to inspect the inspector is absurd, and it would recurse |

A negative filter would admit any supervisor id invented tomorrow. The positive one excludes it by default, which is the same choice `dumps::parse_dump_name` makes about filenames. Compound configurations are skipped too — they run no process of their own, and their members already appear under their own ids. A running process with no pid is skipped rather than offered pid-less. The inspector's own sidecar is additionally recognised by name (`cb-inspector*`), because a capture running in another window of this application is not in this supervisor's map at all yet still turns up in the enumeration.

### The reused-pid hazard

The list is refreshed on mount, on visibility, and when a run starts or stops — never continuously, because polling the supervisor on a timer to keep a dropdown honest is a poor trade. So a pid can go stale between being chosen and being used, and Windows recycles pids aggressively under build and test churn. The replacement is very often *another managed process*, which would attach happily and render a stranger's heap under the user's configuration name.

`session::live_target_reason` therefore re-enumerates the machine's .NET processes inside `inspect_capture`, immediately before anything is spawned, and refuses any live pid no longer on that list with a sentence saying exactly that. An enumeration that *failed* is refused with its own reason instead: `enumerate` returns a `Result`, never an empty list standing in for a failure, because "your process has exited and its pid may have been reused" is a claim about the user's application and must not be produced by a locked executable or an unwritable temp directory. A dump target is never refused there: a file on disk is whatever it was when it was written. This is the abstain rule applied to identity rather than to a value — the wrong heap under the right label is the worst output this feature could produce.

`session::unsupported_reason` rejects the other impossible pairing before the snapshot is paid for: a live process has not crashed, so `RootSpec::CrashException` cannot be served from one.

### The cost is stated in the core, not in the view

`session::attach_caveats()` returns the sentences about the snapshot cloning the working set and about the list holding every .NET process on the machine rather than only this app's, and `InspectStatus` carries them. The Run tab renders them **beside the button**, before the click, because a warning shown by the Objects tab arrives after the pause it was meant to warn about. Keeping the words in `cb-core` is what makes the picker, the Run tab and the capture header say the same thing rather than three similar things — and the per-row launcher caveat is derived in `session::launcher_caveat` for the same reason, rather than written into a view.

The sidecar always attaches with `CreateSnapshotAndAttach(pid)`, which clones the process image so the application keeps running. A suspending attach — freezing every thread until the capture finishes — is never used: stopping a service to read a field is a decision that needs a person behind it, and the default has to be the one that cannot surprise anybody.

## The abstain rule in the types

Inherited from [`git::grouping`](core-crate.md#git): **a wrong value is much worse than no value.** A graph is read through several layers of indirection — a dump region that was never captured, a field the JIT put in a register, a type that will not resolve — and at every one of them the tempting failure is to render a plausible zero.

Two variants of `ObjectValue` exist solely to make that impossible:

- **`Unavailable { reason }`** — it could not be read, and here is the sentence saying why.
- **`Elided { reason }`** — a cap stopped the walk here: `depthLimit`, `childLimit` or `nodeLimit`. Never a shorter list that looks complete.

The same rule governs the surrounding code. `dumps::parse_dump_name` returns `None` for any filename that is not exactly what `dump_env` would have produced, so `prune` deletes only files it fully decoded — a `.dmp` a user dropped in by hand is not this application's to remove. `dumps::newest_for` exists but nothing calls it, because attributing a dump to a run needs a start time the Run tab does not record; the tab lists every dump and lets the reader choose rather than asserting an attribution nobody made.

Cap widening follows from the same place. Re-reading a branch under the caps that truncated it returns the identical truncation — an expand that expanded nothing while reporting a fresh read. So `inspect_capture` takes `widen`, naming the cap that stopped the previous read, and `Caps::widened` raises that axis only.

## Why addresses cross as hex strings

`ObjectValue::Reference { address: String }`, `RootSpec::Address { address: String }` — never numbers. The reasoning is worth stating precisely, because the obvious argument is not quite the true one:

- Current Windows and Linux user-mode addresses sit below 2^47 and so **would** fit in a JavaScript number today. That is an accident of how much of the address space present hardware and kernels hand out, not a guarantee, and it is not a property to build a wire format on.
- The address is the **identity** used for expansion and for cycle detection. It is compared for equality and round-tripped back into the next request. A representation that can lose a bit under any future layout is the wrong representation for an identifier, regardless of how comfortable today's margin is.
- Hex is what SOS, WinDbg and every .NET diagnostics tool prints. An address the user can copy out of the tree and paste into another tool — or take from one and paste in — is worth more than a number that saves a parse.

Pinned by `addresses_cross_as_hex_strings` in `crates/core/src/inspect/model_tests.rs`.

## Bundling: a departure

Everything else code-basics runs is found on `PATH` — `dotnet`, `node`, `git`. The user already has them, they are already configured, and shipping copies would be worse in every dimension.

The inspector could not follow that rule. There is no `cb-inspector` on anyone's `PATH`, no package that provides it, and the ClrMD-based walk is specific enough to this application that no general-purpose tool substitutes. So this is the first thing shipped as a bundled resource, via `bundle.resources` in `src-tauri/tauri.conf.json`:

```json
"resources": { "resources/inspector/": "inspector/" }
```

The cost was kept small deliberately:

- **Framework-dependent single-file**, ~4 MB per architecture. Self-contained would add ~70 MB each; every machine running a .NET dev tool already has a runtime.
- **Not built by `cargo build`.** `pnpm sidecar:build` publishes it, and a machine with no .NET SDK skips it with a warning rather than failing the build — the same degradation `adapters::msbuild` makes. Absence is an ordinary state, reported as "not installed, run this to install it".
- **`CB_INSPECTOR_PATH`** overrides the bundled location with a directory or a file, so development does not require a bundle at all.

## Related

- [Inspecting objects](../guides/inspecting-objects.md) — the user-facing guide
- [The core crate](core-crate.md) · [the Tauri shell](tauri-shell.md) · [the IPC contract](ipc-contract.md)
- [Command reference](../reference/commands.md#object-inspection) · [configuration](../reference/configuration.md#inspector)

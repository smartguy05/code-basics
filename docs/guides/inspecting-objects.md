# Inspecting objects

The **Objects** tab reads the real managed heap — the actual objects a .NET process held, either out of the crash dump the runtime wrote as it died, or out of a process that is still running. A stack trace says where something broke; this says what was in the object.

There is no debugger involved, nothing is attached to your editor, and the application does not have to be started differently.

## Two limits, stated first

Everything below only makes sense once these are clear. This reads **memory**, not a running execution context:

- **No method is ever called.** `order.EstimateCost()` cannot be run, because nothing is executing — in a dump the process is dead, and in a live target the threads are not yours to drive.
- **No property is ever evaluated.** The heap holds fields, so `public int Count => _items.Length` is not a value that exists anywhere; you see `_items` and count it yourself. Auto-properties are the exception, because they *do* have a backing field — `<Total>k__BackingField` is shown as `Total`.

The compensation is real: inspection cannot throw, cannot deadlock, and cannot change the thing it is inspecting. A watch expression in a debugger can do all three.

The second rule that governs the whole feature: **a wrong value is much worse than no value.** A field shown as `0` that was never actually read sends you off debugging the wrong thing. So anything unreadable appears as an explicit gap with a sentence explaining it, and anything cut short by a limit says it was cut short — never a shorter list that looks complete.

## The four ways a value goes wrong, and what each needs

| Situation | What captures it | Setup |
|---|---|---|
| **Unhandled crash** in your app | The .NET runtime writes a heap dump on its way down | Enable capture for the workspace. Nothing else |
| **Test host crashes** | VSTest's `--blame-crash-collect-always`, passed automatically while capture is on | Free on VSTest. On Microsoft.Testing.Platform, add the `Microsoft.Testing.Extensions.CrashDump` package and pass `--crashdump` — MTP ignores every `--blame-*` option silently |
| **Wrong value, no exception** | Attaching to the process while it runs | Pick **Running process** in the Objects tab, or use the Inspect buttons on a running configuration in the Run tab. Every attachable .NET process on the machine is offered, each labelled with how it relates to your configurations — see *the pid is not always your application* below |
| **Caught exception** | Nothing writes a dump for one. Best-effort: exception objects usually stay resident until the next collection, so a dump taken later often still contains them | Guaranteed capture needs one line in your own code at the catch site |

The **Exceptions on the heap** root exists for that last row: it scans the heap for anything deriving from `Exception` and shows what it finds. It is honest about being a scan — if nothing is there, it says nothing was found rather than implying the exception never happened.

## Inspecting a running process

Pick **Running process** in the Objects tab, or press one of the Inspect buttons the Run tab shows beside a configuration that is up. The list holds **every .NET process on this machine that can be attached to**, not only the ones code-basics started, and each entry says how it was linked to your work:

| Label | What it means |
|---|---|
| **Launched** | Its pid is exactly what code-basics started for that configuration |
| **Descendant** | Its parent chain reaches a process code-basics started — this is the `dotnet run` child, and usually the one you want |
| **Unrelated** | A .NET process code-basics did not start. It carries **no configuration name**, because putting your configuration's name on a stranger's heap is the one mistake this feature must not make |

Your own processes sort first. A build, a git fetch and the inspector's own sidecar are dropped from the list entirely — they are noise this application created, not the machine's. An empty list is a normal answer, not a failure.

The list is refreshed when the tab is opened, when a run starts or stops, and on demand — not continuously. A pid chosen minutes ago is re-checked against a fresh enumeration of the machine's .NET processes immediately before the capture spawns, and refused if it is gone. Windows recycles pids readily under build and test churn, and the replacement is very often another managed process that would attach happily and render a stranger's heap under your configuration's name.

### What it costs the application

This is the one part of the feature that reaches into a process you care about while it is serving traffic, so the price is stated beside the button rather than after the pause:

- **The snapshot clones the process's working set.** Expect a pause of the order of a second and memory use roughly doubling for as long as the copy exists. On a machine already short of memory that is a real cost.
- **Your application is not stopped.** ClrMD can instead attach with every thread frozen until the capture finishes; the request carries a `suspend` flag for it, it is opt-in, and this app never sets it. A service being inspected keeps serving.
- **The price of not stopping it is staleness.** A live capture is a moment in time. By the time the tree renders, the field you are reading may already hold something else, and expanding a node past a cap is a genuinely *new* snapshot of a process that has moved on — the tab says so in a band you cannot miss. Dumps are exempt, because two reads of a file are the same bytes.

### Catching a caught exception

The `Exceptions on the heap` root is the best-effort answer, and best effort is meant literally: it finds exception objects **still resident** on the heap. One that has already been collected is simply not there, and no scan can recover it. Attaching soon after the failure is the difference between finding it and not.

A *guaranteed* capture needs one line in your own code, at the `catch` site only its author can choose. Nothing writes it for you — the Objects tab shows the snippet with your workspace's dump directory already filled in, ready to copy:

```csharp
// dotnet add package Microsoft.Diagnostics.NETCore.Client
catch (Exception)
{
    var dir = @"<workspace>\.code-basics\dumps";
    Directory.CreateDirectory(dir);

    // Same name shape as the runtime's crash handler, so the Objects tab lists it.
    var exe = Path.GetFileName(Environment.ProcessPath) ?? "app";
    var name = $"{exe}_{Environment.ProcessId}_{DateTimeOffset.UtcNow.ToUnixTimeSeconds()}.dmp";

    // WithHeap is dump type 2 — what the crash handler writes, and what the
    // inspector can read objects out of.
    new DiagnosticsClient(Environment.ProcessId)
        .WriteDump(DumpType.WithHeap, Path.Combine(dir, name));

    throw;
}
```

The filename shape matters: the dumps list decodes `<executable>_<pid>_<unix seconds>.dmp` and a dump named anything else is invisible to this tab.

### The pid is not always your application

`dotnet run` **builds the project and then starts your application as a separate child process**. The pid code-basics recorded is the .NET CLI, not your app, and attaching to it succeeds — the CLI is managed too — while finding none of your types. An empty tree there means "the launcher holds none of your objects", which reads exactly like "your object is not there".

So the application itself is offered too. Because the picker enumerates the whole machine and follows parent chains, the `dotnet run` child appears in the list as a **Descendant**, under the same configuration name — that is the process holding your objects, and it is the one to capture.

The launcher is still listed, because refusing to show a real running process would be its own kind of lie, but it is labelled as a launcher and points at the child by name and pid:

> The process code-basics started for `Crasher` is a launcher: it built the project and then ran the application as a separate child process. This pid is the launcher, so a capture of it reads the launcher's heap and finds none of your objects — an empty result would not be evidence that there are none. The application itself is `Crasher` (pid 27900), also in this list; capture that one.

That caveat is derived from the observed process tree, so it appears only where it is true, and only for a configuration that is genuinely run through the .NET CLI. An application that *is* the pid code-basics started is never described as a launcher, even when it starts .NET children of its own — a worker or a plugin host is not evidence that the process holding your objects is something else.

Nothing guesses which child is the real application either. When a `dotnet run` leaves an MSBuild worker running beside your app the caveat says there are several and asks you to pick the one named after your project; and while the build is still going, when the only child on the list is the compiler server or an MSBuild node, it says so rather than presenting a build server as your application. Only a child that can actually be named is preselected for you — everywhere else the choice stays yours.

## The Inspect buttons on a run and on a test

The Objects tab can always be driven by hand, but the useful entry points are contextual — the failure is already on screen, and the button carries the target and the root with it. What you clicked is restated above the values it produced, so a capture is never anonymous.

| Where | Appears when | Opens |
|---|---|---|
| **Run tab, after an exit** | The run failed *and* a dump was found | The crash exception. Labelled **Inspect crash** when the dump carries the pid the run reported, and **Inspect this dump** — with the executable and pid — when it does not |
| **Run tab, while running** | The configuration's process is attachable | **Inspect exceptions** attaches and reads every exception on the heap; **Inspect instances** reads a type you name |
| **Tests tab** | A failed test is selected and the run left a dump | That dump, at the crash exception, described as written *while* the run was going rather than as this test's |

They deliberately do **not** appear when:

- **The inspector is not installed.** A button that leads to "run `pnpm sidecar:build`" is worse than no button.
- **Capture was not armed.** The `DOTNET_Dbg*` variables are read when the process starts, so switching capture on afterwards changes nothing about a run already going.
- **You stopped the run from the toolbar.** Stop kills the process tree with `taskkill /T /F`, and a force-killed process writes nothing — so a cancelled exit is never offered a crash to inspect. That is correct, not a missing feature.
- **The run succeeded.** There is nothing to explain.

## Crash dumps contain everything

> **A dump is a verbatim copy of process memory.** Connection strings, access tokens, decrypted secrets, whatever customer data was in flight — all of it, in plain bytes, in a file on your disk. This application manages .NET user secrets, so the processes it runs demonstrably hold exactly that.

That is why capture is **off by default and opt-in per workspace**, why the tab says so before you turn it on, and why the setting is never inferred from anything else.

What is done about it:

- Dumps are written to `.code-basics/dumps/`, which is git-ignored automatically, so they never reach shared history.
- They are pruned by **both** a file count and a total byte budget, before every .NET run as well as after every capture — a workspace that crashes repeatedly and is never inspected is still bounded.
- The budget also covers the dumps VSTest's blame collector leaves in `.code-basics/results/` under names of its own choosing, because those exist only because this app passed the flag.

They remain readable on your machine until they are pruned. Deleting the directory is always safe.

## Turning capture on

`.code-basics/config.json`, which is **checked in** — so enabling it here enables it for everyone who works in the repository. Every armed run carries a warning in the Run tab saying exactly that.

```json
"inspector": {
  "captureDumps": true,
  "keepDumps": 3,
  "maxDumpMegabytes": 2048
}
```

| Key | Default | Meaning |
|---|---|---|
| `captureDumps` | `false` | Opt in. Absent section means off |
| `keepDumps` | `3` | How many dumps to retain, newest first |
| `maxDumpMegabytes` | `2048` | Total budget for what dump capture writes. This is the limit that actually binds — bytes run out long before the count does |
| `caps` | 5 / 100 / 512 / 5000 | `maxDepth`, `maxChildren`, `maxStringLength`, `maxNodes` for one capture |
| `env` | – | Extra environment for dump-capturing runs only, for a project that needs a different dump type |

Full field list: [configuration reference](../reference/configuration.md#inspector).

Under the hood this sets three environment variables on the run (`DOTNET_DbgEnableMiniDump`, `DOTNET_DbgMiniDumpType=2`, `DOTNET_DbgMiniDumpName`) and the runtime does the rest. They are layered *under* your run configuration's own environment, so anything you set yourself still wins.

### Why the dropdown shows dumps you did not expect

Those variables are inherited by the **entire process tree**, so `dotnet run` arms its build host and every child it starts. That is precisely why the executable name is encoded into each filename: `Crasher.exe_25764_1786044924.dmp` is *executable, pid, unix seconds*. Every dump in the workspace is listed with that label, newest first, and you pick the one naming your application. Nothing attributes a dump to a run automatically, because attributing it wrongly would point you at the wrong data.

The same rule governs the **Inspect** buttons that appear on a failed run or a failed test. A dump is only called *this run's crash* when it carries the pid that run reported. Otherwise the Run and Tests tabs still offer it, but as what it is: "a dump was written while this was running", named with its executable and pid, explicitly not confirmed to be this configuration's. With two configurations up and both armed, the newest dump since a run started is as likely to be the other one's.

### When no dump appears

- **The exception was caught.** The runtime only writes on an *unhandled* crash.
- **You stopped the run from the toolbar.** Stop kills the process tree with `taskkill /T /F`, and a force-killed process writes nothing. So Stop-then-Inspect correctly finds nothing — that is not a bug.
- **Capture was off when it crashed.** The setting is read when the run starts.

## Reading a capture

Pick a dump, pick what to root the tree at, press **Capture**:

| Root | What it does |
|---|---|
| **Crash exception** | The exception that killed the process, with its stack frames |
| **Exceptions on the heap** | Every `Exception`-derived object still resident, up to a limit |
| **Type** | Every instance of a named type. Matched as the runtime sees the name, so try the full namespace |

Expanding a node that was cut short re-runs the inspector against that object's address with the relevant limit raised. Two consequences worth knowing: the console shows a second run, and for a *live* target the result is a new snapshot of a process that has moved on in the meantime — the tab says so in a band you cannot miss. A dump is exempt, because two reads of a file are the same bytes.

Values you will see that are not values:

| Shown as | Means |
|---|---|
| **Elided** | A cap stopped the read here — depth, child count, or total nodes. Expand to raise it |
| **Unavailable** | Genuinely could not be read: a region absent from the dump, a field the JIT put in a register, a type that would not resolve. The reason is printed |
| **Cycle** | This address is already on the path above; the graph loops here |

## Installing the inspector

The heap walk happens in a small .NET component (`cb-inspector`) that ships with the installed app but is **not** produced by `cargo build`. In a fresh checkout the tab will say it is not installed, and how to fix it:

```
pnpm sidecar:build
```

That publishes `cb-inspector-win-x64.exe` and `-x86.exe` (~4 MB each) into `src-tauri/resources/inspector/`. Two architectures because the walker can only read a target of its own bitness; an x64 attempt that reports a mismatch is retried once with x86 automatically. `CB_INSPECTOR_PATH` points at an existing build instead, which is the development override.

Missing .NET does not fail the build — everything else in code-basics works without the inspector.

## Where things live

```
.code-basics/
├── dumps/     crash dumps — git-ignored, pruned by count and bytes
└── inspect/   one directory per capture: request.json in, result.json out
```

Both are git-ignored and safe to delete at any time. Sessions are pruned to the newest 20.

Related: [How live inspection is built](../architecture/live-inspection.md) · [configuration](../reference/configuration.md) · [commands](../reference/commands.md)

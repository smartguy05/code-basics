# Implementation plan

## Approach

ClrMD (`Microsoft.Diagnostics.Runtime`) reads a .NET process's real heap, from either a **crash dump** or a **live process**, producing the same result from both — which is what lets one inspector serve all four failure modes.

ClrMD is .NET and code-basics is Rust, so the walk happens in a **one-shot sidecar**: request file in, `result.json` out. This is the app's existing idiom (a test runner streams output and leaves a `.trx`; the inspector leaves a `result.json`), and it meant **zero changes to `crates/core/src/process/`** — no stdin, no session lifecycle, and kill/cancel came free.

## Layer split

- **C# sidecar** — platform calls only: heap walking, process enumeration, parent pids.
- **`cb-core`** — every decision: attribution, caps, cycle handling, the abstain classifier, dump discovery. All unit-testable with no .NET involved.
- **`src-tauri`** — bridge only. Its one genuinely Tauri-specific job is resolving the bundled resource path.
- **`src/`** — rendering.

## Phases

| Phase | Scope | State |
|---|---|---|
| 0 | The contract in pure Rust — model, graph parser, tree shaper, fixtures. No .NET. | Done |
| 1 | The C# sidecar reading a dump; bundling; dev override | Done |
| 2 | Dump capture end to end, Tauri commands, Objects tab | Done |
| 3 | Live attach, contextual Inspect entry points | Done |
| 4 | Enumerate real .NET processes and attribute them to runs | Done |

Phase 0 deliberately came first and used no .NET at all: it proved the whole architecture headlessly under `cargo test -p cb-core` before a line of C# existed, which is the point of the cb-core rule.

## Method that worked

Multi-agent workflows: parallel builders on **disjoint files**, then two verifiers in parallel — one running real builds, one adversarial reviewer **forbidden from changing anything** — then a fixer that runs only on serious findings and is told to verify each first, because reviewers can be wrong. The reviewer found 13, 7 and 7 issues across phases 2–4.

Every phase ended with a **real acceptance test** against a running fixture, not a code read. That is where the important defects came from — see `notes.md`.

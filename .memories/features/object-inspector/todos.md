# Todos

## Blocking a clean landing — RESOLVED (reconciled 2026-08-21)

All three items below are done/moot. Verified against the tree 2026-08-21:
the inspector sources (`sidecar/inspector/*`, `src/views/InspectView.tsx`) are
committed, the repo-wide rustfmt landed as its own commit (`6b481d7`, per CLAUDE.md),
and `.memories/` is tracked (not gitignored). Kept here as history:

- [x] ~~Nothing is committed (~77 files on `claude/lightweight-ide-replacement-52cp2n`).~~
- [x] ~~Land the repo-wide rustfmt as its own commit first.~~ Done in `6b481d7`.
- [x] ~~Decide whether `.memories/` is committed or gitignored.~~ It is committed/tracked.

## Deferred, with reasons

- [x] ~~**`Dictionary<K,V>` renders as `_buckets`/`_entries`**~~ **DONE (WF2, 2026-08-24, B1):**
  sidecar `TryGetDictionary`+`ExpandDictionary` emit one `kind:"pair"` container per live
  entry (Key/Value children); Rust `ObjectValue::Pair` + classify arm (key-pinned); TS union +
  ObjectTree case. Abstains to ordinary field rendering on any surprise. **Live sidecar path
  needs manual app verification** (fixture `dictionary.json` pins the contract).
- [x] **VSTest blame dumps are not listable in the Objects tab** — **DOCUMENTED (B4, 2026-08-24)** in `docs/guides/inspecting-objects.md` as a known exception (no pid/executable/timestamp in the name; VSTest won't redirect/rename; synthesising them is the fabrication the module forbids). Left as a follow-up per Anthony's decision — no code change.
- [x] **`InspectRequest::suspend` was vestigial — REMOVED (B3).** Decision: remove, not wire. Deleted the field from the Rust wire type, the TS mirror, and the C# `RequestDto`; `Target.OpenLive` now unconditionally takes `CreateSnapshotAndAttach`. Behavior unchanged (suspend was always false).
- [x] ~~**`UseAppHost=false` projects are not preselected.**~~ **DONE (WF2, 2026-08-24, B2):**
  the sidecar now exposes each process's command line (one WMI query / `/proc/<pid>/cmdline`),
  and `runs_assembly` matches `dotnet exec <output>.dll` against the config's assembly stem to
  disambiguate WITHIN the several-children arm. Additive — the three pinned caveat abstentions
  are unchanged; AssemblyName overrides still abstain. **Live preselection needs manual app
  verification** (that Win32_Process.CommandLine is populated for the child on this machine).
- [ ] **No attach affordance in RunView for a `dotnet run` with several published children.** Previously it offered an arbitrary one; now it offers none. Deliberate, but a visible behaviour change.
- [ ] `dumps::newest_for` has no production caller — `session::status` uses `dumps::list` instead. Kept as tested public core API; the run-start timestamp that would feed its `since` argument is unwired.

## Verification gaps

- [ ] **The bundled-resource path is untested end to end.** `app.path().resolve("inspector", BaseDirectory::Resource)` is only exercisable in a running bundled Tauri app; a bundling mistake would surface only at runtime.
- [ ] **No frontend tests exist**, so `ObjectTree`/`InspectView` rendering is covered by `tsc` alone. The null-vs-unavailable distinction — the single most important thing the tree conveys — is unverified by anything automated.

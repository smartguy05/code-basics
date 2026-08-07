# Todos

## Blocking a clean landing

- [ ] **Nothing is committed.** ~77 files on `claude/lightweight-ide-replacement-52cp2n`, still at `657230d`.
- [ ] **Land the repo-wide rustfmt as its own commit first.** A large share of the modified-file count is a semantically inert formatting pass predating this work — reflowed call chains, reordered `use` statements — in files the feature never logically touched. Committed together, the feature diff is unreadable.
- [ ] Decide whether `.memories/` is committed or gitignored. Currently untracked, so it shows as `?? .memories/` in git status and in the app's own Changes tab.

## Deferred, with reasons

- [ ] **`Dictionary<K,V>` renders as `_buckets`/`_entries`**, not key–value pairs. `List<T>` and arrays are unwrapped properly. Entries are structs needing per-field key/value extraction; shipped `List<T>` working correctly rather than both partly.
- [ ] **VSTest blame dumps are not listable in the Objects tab.** They are inside the byte budget and the run warns where they land, but their filenames carry no pid, executable or timestamp — synthesising those would be the fabrication the module forbids. VSTest offers no way to redirect them out of `--results-directory`.
- [ ] **`InspectRequest::suspend` is vestigial.** No IPC parameter surfaces it, so the UI correctly refused to add a checkbox controlling nothing. Either wire it through with the "this stops your application" warning, or remove the field.
- [ ] **`UseAppHost=false` projects are not preselected.** Their app runs as `dotnet exec app.dll`, and `dotnet` is on the build-tool exclusion list. The row is still selectable — abstaining rather than guessing — but it is a small loss of help.
- [ ] **No attach affordance in RunView for a `dotnet run` with several published children.** Previously it offered an arbitrary one; now it offers none. Deliberate, but a visible behaviour change.
- [ ] `dumps::newest_for` has no production caller — `session::status` uses `dumps::list` instead. Kept as tested public core API; the run-start timestamp that would feed its `since` argument is unwired.

## Verification gaps

- [ ] **The bundled-resource path is untested end to end.** `app.path().resolve("inspector", BaseDirectory::Resource)` is only exercisable in a running bundled Tauri app; a bundling mistake would surface only at runtime.
- [ ] **No frontend tests exist**, so `ObjectTree`/`InspectView` rendering is covered by `tsc` alone. The null-vs-unavailable distinction — the single most important thing the tree conveys — is unverified by anything automated.

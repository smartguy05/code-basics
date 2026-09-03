# Notes / gotchas

## The build stamp has FOUR answers, not two

`build.rs` -> `aboutLogic.treeState` classifies `gitSha` as one of:

| Value | Meaning |
|---|---|
| `20a7dbb` | clean tree at build time |
| `20a7dbb-dirty` | **modified** tree — the build is *not* exactly that commit |
| `20a7dbb-unverified` | commit known, `git status` did not answer, cleanliness unknown |
| `unknown` | no `.git`, or no `git` on `PATH` |

`-dirty` is the load-bearing one. An unmarked sha on a modified tree is a
**wrong** answer about which code is running, which this codebase treats as much
worse than no answer. `-unverified` exists so the near-impossible case (rev-parse
succeeds, status fails) cannot fall back to reporting a bare sha, which would
silently *claim* clean. A blank sha is read as `unknown`, never as clean.

## Why the build date crosses the wire as epoch seconds

`CB_BUILD_DATE` carries UNIX epoch **seconds as a decimal string**, and
`aboutLogic.formatBuildDate` renders `2026-09-03 20:22:35 UTC`.

Formatting it in `build.rs` would have meant hand-rolled civil-from-days
arithmetic (no `chrono`/`time` is a direct workspace dependency, and adding one
for a date string was the dependency the plan avoided) **in the one place in this
tree that no test can reach**. CLAUDE.md's rule applies literally: if something
is hard to test it is in the wrong layer. So the build script stays too simple to
be wrong and the formatting is a pinned pure function.

`formatBuildDate` uses a strict digits-only regex rather than `Number()` — that
accepts `" 1e9 "`, hex and signs — and `new Date(NaN).toISOString()` **throws**
while a bare `new Date(raw)` would happily render `Invalid Date` as though it
were a reading.

## Emitting a rerun-if-changed directive from this build.rs is safe — but WATCHING `.git/HEAD` DOES NOT WORK

Emitting any `rerun-if-changed` disables cargo's default "rerun on any source
change" heuristic. That is already the case here: `tauri_build::build()` emits
its own directives. Only `../.git/HEAD` is watched — it covers a commit, a
checkout and a branch switch. Watching the working tree would relink the whole
shell on every keystroke, and the `-dirty` marker is exactly what makes a stale
clean/dirty reading something the *reader* can see rather than something they
have to detect.

**Correction, established by the adversarial review and verified three ways —
this is the finding, not the paragraph above it.** `../.git/HEAD` is the wrong
file to watch, and the stamp goes stale in ordinary use:

1. **`git commit` on a branch writes the ref, not `.git/HEAD`.** Measured here:
   `.git/HEAD` mtime 2026-09-01 08:30 against HEAD's commit date 2026-09-03
   11:56. So the only watch this script adds **never fires on a commit** — the
   one event it was chosen for.
2. `touch src-tauri/src/lib.rs` + `cargo check -p cb-app --all-targets`:
   `cb-app` recompiled, and all three `target/debug/build/cb-app-*/output` files
   stayed **byte-identical** (`CB_BUILD_DATE` unchanged). The build script did
   not rerun, so a source edit does not re-stamp either.
3. `tauri_build::build()` emits `rerun-if-changed` only for `tauri.conf.json`,
   `capabilities` and `resources/…` — read out of those same output files.
   **Nothing covers `src/`.**

Consequence, in both directions: build dirty then commit and rebuild → About
reports the *previous* commit plus a false `-dirty`; build clean then edit and
rebuild → About reports a **bare clean sha for a modified binary**, which is
precisely the "wrong statement about which code is running" this file's own doc
comment forbids. `CB_BUILD_DATE` is the instant of the last build-script *run*,
not of the build.

**So the `-dirty` marker does not make a stale reading self-evident** — which is
what the paragraph above assumed, and it is why that assumption is left visible
here rather than deleted. Not fixed; see `todos.md`. Any fix has to invalidate on
the resolved ref file and the index, or pay a relink; there is no free option.

## Two version numbers can legitimately disagree

`getVersion()` reads `tauri.conf.json`; `about_info().appVersion` is `cb-app`'s
crate version. `Cargo.toml`'s `[workspace.package] version` couples the two Rust
manifests, but `package.json:4` and `tauri.conf.json:4` are **separate copies**.
So `aboutLogic.versionDrift` names both rather than picking one to believe — and
abstains (returns `null`) while either is blank, so the in-flight read cannot
raise a false release alarm. All four manifests read `1.3.0` today.

## MenuBar is mouse-only, and Help matches it

No Escape handler, no arrow keys — click to open, hover to switch, click the
shared `.dropdown-backdrop` to close. So the Help menu needed **no** keyboard
work to be consistent. Both `onClick` and `onMouseEnter` are wired: the hover
handler is what lets an already-open menu switch, and omitting it makes Help feel
broken next to the other two.

The *dialog* does get Escape-to-close (the `FeaturesPicker` effect) — trapping a
reader in a modal with no keyboard exit is a different problem from a menu bar
without arrow keys.

## The permission was verified, not assumed

`core:default` includes `core:app:default`, which includes `allow-version` and
`allow-tauri-version`. Read out of `src-tauri/gen/schemas/acl-manifests.json`
(the default permission set) and `desktop-schema.json` (what `core:app:default`
expands to). So `getVersion()`/`getTauriVersion()` need **no** capability edit
and **no** new dependency — and `tauri-plugin-os` was not added, since a tiny
command over `std::env::consts` is cheaper than a dependency.

## Post-review fix: the stamp went stale on a commit (2026-09-03)

An adversarial review pass proved empirically that `build.rs` did not re-run on
either of the two events that matter, so About could report a commit the binary
was not built from:

1. **A commit on a branch does not touch `.git/HEAD`.** It rewrites
   `.git/refs/heads/<branch>`. Measured here: `.git/HEAD`'s mtime was two days
   older than the commit date of the commit it pointed at. So `HEAD` — the only
   watch the script had — never fired on a commit. Build, commit, rebuild
   reported the *previous* sha with whatever dirty marker had been true then.
2. **A source edit does not re-run it either.** `touch src-tauri/src/lib.rs`
   then `cargo check -p cb-app` recompiled cb-app while every
   `target/debug/build/cb-app-*/output` stayed byte-identical.
   `tauri_build::build()` emits rerun-if-changed only for `tauri.conf.json`,
   `capabilities` and `resources/`, so nothing covered `src/`.

Fixed in `watch_git_refs()`: watch `.git/HEAD`, `.git/index` (staging, which is
what most `-dirty` transitions are), `.git/packed-refs` (the same ref after a
`git gc`), and the loose ref `HEAD` resolves to, parsed out of the `ref: ...`
line. Verified from the emitted build output that
`../.git/refs/heads/fix/search-notes-intents-controls` is now watched. A detached
HEAD names no ref and needs none — `HEAD` itself holds the sha.

**The residual limitation is real and is documented rather than glossed.** An
*unstaged* edit after a build still does not re-run the script, because watching
the working tree would relink the whole shell on every keystroke. So the stamp
describes the tree as of the last time the script ran. That is why the dialog
shows the build date beside the commit — the pair is checkable — and why a reader
is never asked to infer freshness from the sha alone. The old comment claiming
"the `-dirty` marker is precisely there so a stale clean/dirty reading is not
something the reader has to detect" was exactly backwards and has been corrected.

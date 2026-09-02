# Notes

## The environment-variables bug was one bug, not two

"Rider configurations aren't being brought in" and "saved environment variables
don't persist" were the **same** defect. The old `rider::id_for` slugged the
*display name* only, so two projects with a `Development` configuration both
landed on `rider:development`. `config::upsert` keys on id, so the second import
overwrote the first — which looks like a missing import from one angle and like
a vanishing environment map from the other.

## Why FNV-1a is written out by hand in `config.rs`

`DefaultHasher`'s output is explicitly **not** stable across Rust releases, and
this hash lands in a file that is checked in and shared with a team. A toolchain
bump would silently re-key every imported configuration and orphan every
favourite. FNV-1a is fifteen lines and fixed forever.

## The v1 → v2 migration cannot repair an existing collision

Migration rewrites the ids of whatever is *in* the file. Where v1 already
overwrote one project's configuration with another's, the lost record is simply
not there to reconstruct. The fix for an affected workspace is to import again;
`docs/reference/configuration.md` says so.

## `remove_if_pid` exists because of one race

Stopping a debug session and immediately starting another for the same config
id: the old adapter is still being reaped while the new one has already recorded
itself in the Running panel. A plain `remove(root, key)` from the dying task
would erase its successor's row. `RunningStore::remove_if_pid` makes the removal
conditional on still being that generation.

## A null exit code is not a failure

`run_prepared` emits `Exited { code: None }` whenever the session did **not** end
on its own — a Stop, or a replacement launch. Mapping that to `success: false`
paints every deliberate Stop red. `debugLogic.debugEffects` treats a null code as
success and says why; the test `does not paint a stop red` pins it.

## Two core tests fail on `main` and are unrelated to this work

`git::coverage::tests::two_declared_labels_covering_one_file_surface_as_candidates`
and `sql::driver::sqlite::tests::the_only_call_that_opens_a_handle_is_the_one_under_the_deadline`.
Verified by running both in a `git worktree` at `89c4b80` — they fail there too.
Do not chase them from this work item.

## Codex's plan lived only in its session transcript

It ran out of usage before writing a handoff file. If a plan needs recovering
again: `~/.codex/sessions/<yyyy>/<mm>/<dd>/rollout-*.jsonl`, one JSON object per
line, `payload.type == "message" && payload.role == "assistant"`; a plan-mode
plan is wrapped in `<proposed_plan>`.

## Installing the .NET adapter on this machine (2026-09-02)

`winget install Samsung.NetCoreDbg` → 3.1.3-1062, MIT, installed to
`%LOCALAPPDATA%\Microsoft\WinGet\Packages\Samsung.NetCoreDbg_*\netcoredbg\`,
and winget puts that directory on the **user** PATH.

Two things that will waste time if forgotten:

- **A running app never sees it.** `RealProbe::on_path` reads
  `std::env::var_os("PATH")` from the current process, and a GUI app inherited
  its environment at launch. The adapter appears only after a restart — and
  after a sign-out/in if Explorer itself is holding a stale environment. Pin
  `CB_DAP_DOTNET` to skip the PATH question entirely; it is checked first.
- **vsdbg is sitting right there and is deliberately not used.**
  `ms-dotnettools.csharp-2.140.9-win32-x64/.debugger/x86_64/vsdbg-ui.exe` exists
  on this machine. Discovery of it was removed on purpose: its runtime licence
  restricts it to the Visual Studio product family. Do not "fix" the not-found
  error by re-adding that probe — `CB_DAP_DOTNET` is the escape hatch for
  anyone who makes that call for their own machine.

## Bundling the adapters into the installer (2026-09-02)

The original plan said adapters were "user-installed dependencies… not
downloaded or redistributed". That was reversed on request: `pnpm
debuggers:fetch` now vendors both into `src-tauri/resources/debuggers/` and
`tauri.conf.json` ships them, so a fresh install debugs with nothing to
download. Both are MIT (netcoredbg 3.2.0-1092, js-debug v1.117.0) and 11 MB
together. netcoredbg's release zip carries **no** LICENSE, so the script fetches
it from the tag — MIT requires it in a redistribution.

Three things that cost time, all now encoded in the script:

### `tar` is two different programs, and Git Bash picks the wrong one

Windows 10+ ships **bsdtar** at `System32\tar.exe`, which reads zip *and*
tar.gz. A Git Bash/MSYS shell puts **GNU** tar first on PATH, which reads
neither zip nor an absolute Windows path — it parses `C:\...` as `host:path`
and fails with `Cannot connect to C: resolve failed`, an error naming neither
cause. `systemTar()` addresses the system binary absolutely on Windows, and
extraction runs with `cwd` set so no drive letter is ever an argument.

### js-debug inherits this repo's `"type": "module"` and dies

The tarball ships no `package.json`, so Node decides CommonJS-or-ESM by walking
**up** from the script. Under `pnpm tauri dev` the resources sit in
`target/debug/`, whose nearest ancestor `package.json` is *this repository's*,
which is `"type": "module"`. The adapter is a CommonJS bundle, so it dies on
`Dynamic require of "fs" is not supported` before printing its port, and the
session just times out waiting for one. The identical file works fine outside
the repo — which is exactly what makes it easy to "verify" wrongly. The script
writes `{"type":"commonjs"}` into the extracted directory to make the answer
local.

### A stamp needs a FORMAT number, not just version+hash

`isCurrent` skips a download when the stamp matches. A pinned version and hash
cannot express "same archive, different post-processing", so the fix above
would have been skipped on every machine that had already extracted. `FORMAT`
is in the stamp and is bumped whenever the written layout changes.

## Resolution order is pin → bundle → PATH

`CB_DAP_DOTNET`/`CB_DAP_NODE` are explicit instructions and win. The bundle is
next: it is the version the app was built against. A copy on PATH is any
version at all, so it is the fallback. An absent or empty resource directory is
an ordinary answer (`cargo build` makes none; an offline build makes an empty
one) and the search continues rather than failing.

## A `.js` adapter with no Node is refused at resolution, not at spawn

js-debug is JavaScript, so the spawn layer runs it as `node <script>`. Without
Node that fails with "program not found: node" from a layer with no idea why
this app wanted Node. `registry::needs_node` turns it into a `NotFound` naming
the real reason, which keeps the six-answer rule intact.

# Notes — terminal shell picker

## `detected_shells` is the deliberate opposite of `pick_shell`, in the same file

Both live in `crates/core/src/pty/shell.rs` and answer opposite questions:

- `pick_shell` answers *"what do I spawn"* and falls back to its **last**
  candidate even when none are available, so the spawn error can name something.
- `detected_shells` answers *"what can the user pick"* and **omits** anything it
  could not find, because a row that cannot launch is worse than a shorter list.

The doc comment on **both** functions now says so explicitly, precisely because
the next reader's instinct is to make one match the other. Do not.

Corollary: an empty `shells` list is a legitimate answer, not a failure.
`commands/terminal.rs::spec_program` still falls back to `default_shell()` when
the frontend names no program, so a machine where nothing was detected keeps the
exact behaviour it had before the feature existed.

## Why `wsl` is not listed (and this is the interesting one)

`wsl.exe` ships in `System32` on every modern Windows install whether or not a
distribution exists. So file presence is **not** evidence a shell will start.
Deciding honestly needs `wsl.exe -l -q` — a subprocess spawn, its latency and
its hang risk inside what is meant to be a cheap "what is here" probe. Listing
it on file presence would be exactly the guess this codebase forbids, so it is
not a candidate at all.

`System32\bash.exe` is the same trap one level down: it is the legacy WSL
launcher, and it fails outright with no distro installed. `is_wsl_bash_launcher`
omits a `bash` whose parent directory is `System32`/`SysWOW64`. **Stated as the
heuristic it is** — it would also omit a genuine bash somebody copied into
`System32`, and omitting is the safe side.

`git-bash.exe` is excluded for an unrelated reason: it opens its own MinTTY
window rather than running a shell attached to the PTY we allocated.

## The path is the answer, the id is the preference

`ShellInfo.program` is the resolved absolute path. Detection proved *that file*
exists; re-resolving `"pwsh"` at spawn time re-runs the PATHEXT walk and could
launch a different file than the one the user picked. Staleness is not a problem
because the frontend persists the **id** and the path is re-detected on use.

It is also the only thing that tells two `bash` installations apart, which is
why the UI shows it as the row's tooltip.

## A shell whose path `cmd.exe` would re-read is not offered

`detected_shells` runs `pty::argv::check_batch_argv` over the resolved path.
A Scoop-style `.cmd` shim under a directory containing `&`, `%`, `^`, `<`, `>`,
`|` or `"` cannot be spawned as written (portable-pty quotes with MSVC rules
only; `cmd.exe` then re-parses the line). The spawn-seam guard in
`PtyManager::open_inner` remains the enforcement — this is only about not
advertising a row that would fail the moment it was clicked. The asymmetry is
kept: the **same hazardous directory is fine for a real `.exe`**, pinned by
`the_same_hazardous_directory_is_fine_for_a_real_executable`.

## Tooling gotcha hit while doing this (not a code issue)

Writing Rust source through a bash heredoc via the Bash tool **mangled
backslashes** — `"C:\\Program Files\\..."` reached the file as
`"C:\Program Files\..."` and became `unknown character escape: 'p'`. Fixed by
using a raw string literal (`r"C:\Program Files\..."`) and by doing the edit
with the Edit tool rather than a heredoc. Splice scripts that read both sides
**from disk** are unaffected.

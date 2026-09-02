# Completed

## Debug froze the .NET debuggee instead of running it — fixed

**Root cause.** `run_protocol` read the adapter's protocol stream and emitted to
the webview in the *same* loop, so every message's IPC cost was charged to the
read rate. When the adapter outran it the pipe filled, NetCoreDbg blocked writing
from inside a runtime debug callback — which holds every debuggee thread
suspended — and the application never resumed. No output, no error, no exit.

**Fix.**

- New pure module `crates/core/src/dap/coalesce.rs` (+ `coalesce_tests.rs`, 9
  tests): `Coalescer` merges adjacent same-stream output chunks, capped at
  `MAX_BATCH_BYTES` (64 KiB). Never merges across streams; never splits an
  oversized chunk. Registered in `dap/mod.rs` and its layering table.
- `src-tauri/src/commands/debug.rs`:
  - `pump_protocol` — a task that does nothing but read, decode and forward into
    an **unbounded** queue. Unbounded is the fix, not an oversight: a bounded
    queue makes the reader wait on the consumer, which is the deadlock.
  - `receive` — pulls from the queue, and flushes pending output **before** it
    blocks (an empty queue means we are level with the adapter and the next
    message may never come). Flushing after the wait would strand a quiet app's
    last line.
  - `Adapter.reader` is now `Option<Reader>` so the stream can be *moved* into
    the pump task.
  - Output events go through the coalescer; anything that is not an output event
    flushes first (`is_output_event`), so a state change cannot overtake output
    still held back. Explicit flushes on the error return and after the loop.

**Also fixed, found while reading.** `creation_flags` *replaces* the flag word
rather than OR-ing into it, so `configure_process_group` followed by `no_window`
silently dropped `CREATE_NEW_PROCESS_GROUP`. Two sites in `debug.rs`: the adapter
spawn now calls `configure_process_group` alone (it already sets
`CREATE_NO_WINDOW`, and the group is what `kill_tree` needs since NetCoreDbg has
a debuggee child), and the MSBuild `-getProperty:TargetPath` evaluation calls
`no_window` alone (it must not join a supervised group).

**Gate.** `cargo check --workspace --all-targets` clean; `cargo clippy
--workspace --all-targets` clean; `cargo fmt --all --check` clean; `cargo test -p
cb-core` = 2944 passed, 2 failed — both failures (`git::coverage::tests::
two_declared_labels_covering_one_file_surface_as_candidates`,
`sql::driver::sqlite::tests::the_only_call_that_opens_a_handle_is_the_one_under_the_deadline`)
**confirmed pre-existing** by stashing the change and re-running them.
`docs:index` regenerated, `docs:check` passes.

The coalescer rules were mutation-tested: allowing a cross-stream merge and
removing the cap each break a test. (Making an oversized *lone* chunk "overflow"
is an equivalent mutant — with nothing pending, `take()` is `None` either way.)

## Verification and release (2026-09-02)

Confirmed working in the real app by the user: Debug on `ONEflight.Server.Api`
now streams application output instead of freezing silently.

Full gate, all green this time: `pnpm typecheck` clean, `pnpm test` 57 files /
1441 tests passed, `cargo clippy --workspace --all-targets` clean, `cargo fmt
--all --check` clean. The two `cb-core` failures remain and remain pre-existing.

**The pnpm junction block was not in effect this session** — `Test-Path
node_modules/react/package.json` returned `True` and pnpm scripts ran normally.
So it is intermittent, not permanent; check before declaring the frontend gate
unreachable.

Docs updated: `CLAUDE.md` (the `dap/` paragraph now carries the deadlock
invariant), `AGENTS.md`, `README.md`, `docs/architecture/core-crate.md` (a new
"A client that stops reading deadlocks the debuggee" section), and a regenerated
`docs/INDEX.md`. Version bumped 1.2.0 → 1.2.1 in `package.json`,
`src-tauri/tauri.conf.json` and the workspace `Cargo.toml`, and a fresh installer
built (`pnpm sidecar:build` first, since `tauri build` does not chain it).

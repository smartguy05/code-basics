# Completed — blocking-kill-stalls-runtime (B14)

## Root cause
`kill_tree` is synchronous (Windows shells out to `taskkill` and blocks on exit;
Unix signals + detached escalation thread). It was called inline from async
contexts (`Supervisor::cancel`, four LSP `transport.rs` sites, and `Transport::drop`).
On a single-worker `current_thread` tokio runtime the blocking kill parks the sole
worker.

## Fix
`crates/core/src/process/kill.rs`:
- Kept `pub fn kill_tree(pid) -> bool` unchanged (still needed by Drop's
  no-runtime branch and by the offloaded body).
- Added `async fn spawn_blocking_kill<F: FnOnce()->bool + Send + 'static>(f) -> bool`
  = `tokio::task::spawn_blocking(f).await.unwrap_or(false)`.
- Added `pub async fn kill_tree_async(pid) -> bool` = `spawn_blocking_kill(move || kill_tree(pid)).await`.

`crates/core/src/process/mod.rs`:
- Re-export extended to include `kill_tree_async`.
- `cancel()` now `kill::kill_tree_async(pid).await`.

`crates/core/src/lsp/transport.rs`:
- Import extended with `kill_tree_async`.
- Four async sites (`shutdown`, `write_loop`, `read_loop` read-error, framing-error)
  now `kill_tree_async(pid).await`.
- `Drop` cannot await: `Handle::try_current()` => `Ok` fire-and-forget
  `handle.spawn_blocking(move || kill_tree(pid))`; `Err` inline `kill_tree(pid)`.

## Test (tests-first)
`kill.rs` inline `#[tokio::test] an_offloaded_kill_does_not_stall_the_current_thread_runtime`
(single-worker current_thread, timing-free injected parked body). Confirmed red
first (did not compile — `spawn_blocking_kill` missing), then green.

## Gate
- `cargo test -p cb-core --lib process::` — 32 passed.
- `cargo test -p cb-core --test lsp_transport` — 26 passed (incl. `dropping_the_transport_kills_the_whole_tree`).
- `a_write_failure_kills_the_process_it_can_no_longer_reach` — passed.
- `cargo fmt --check` — clean.
No IPC/type changes. `kill_tree` semantics unchanged.

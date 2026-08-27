//! Platform-specific process *tree* termination.
//!
//! Killing only the process we spawned is not enough. `dotnet run` builds and
//! then launches the built assembly as a child; `npm run dev` wraps a bundler.
//! Terminating just the wrapper leaves the real application alive, holding its
//! port, so the next run fails to bind. Both platforms therefore need an
//! explicit group/tree kill, arranged at spawn time.

/// Arrange for the child to be killable as a group.
///
/// On Unix the child is put into its own process group so a negative-pid
/// signal reaches every descendant. On Windows it gets a new process group so
/// console signals are not shared with the app itself.
pub fn configure_process_group(cmd: &mut tokio::process::Command) {
    #[cfg(unix)]
    {
        use std::io;
        // SAFETY: `setpgid` is async-signal-safe and so legal to call between
        // fork and exec, which is the only place this closure runs.
        unsafe {
            cmd.pre_exec(|| {
                if libc::setpgid(0, 0) == -1 {
                    return Err(io::Error::last_os_error());
                }
                Ok(())
            });
        }
    }

    #[cfg(windows)]
    {
        cmd.creation_flags(windows_creation_flags());
    }

    #[cfg(not(any(unix, windows)))]
    {
        let _ = cmd;
    }
}

/// Windows process-creation flag: run the child without allocating a console
/// window. The Tauri shell is a GUI process with no console of its own, so a
/// console-subsystem child — `dotnet`, a `.cmd`/`.bat` package-manager shim run
/// via `cmd.exe`, `git`, `taskkill` — makes Windows allocate a **visible**
/// console window for it. Without this flag, running a project pops a queue of
/// terminal windows; with it, the same output still streams to the in-app
/// console over the captured pipes.
pub const CREATE_NO_WINDOW: u32 = 0x0800_0000;

/// Windows process-creation flag: give the child its own process group, so a
/// console Ctrl+C aimed at the app is not shared with it.
pub const CREATE_NEW_PROCESS_GROUP: u32 = 0x0000_0200;

/// The creation flags for a supervised child that must be both windowless and
/// killable as a group: [`CREATE_NO_WINDOW`] | [`CREATE_NEW_PROCESS_GROUP`].
/// Defined once so the two flags never drift apart at a spawn site, and defined
/// on every platform (it is pure arithmetic) so the value is unit-testable
/// without a Windows host.
pub fn windows_creation_flags() -> u32 {
    CREATE_NO_WINDOW | CREATE_NEW_PROCESS_GROUP
}

/// Suppress the console window for a raw [`std::process::Command`] on Windows.
///
/// For the auxiliary console tools this crate (and the quality-gate runner)
/// shell out to — `taskkill`, `git`, the `dotnet` SDK evaluation — which must
/// **not** join a supervised child's process group ([`configure_process_group`]
/// is the only thing that arranges that) but should still never flash a console
/// window. A no-op off Windows.
#[cfg(windows)]
pub fn no_window(cmd: &mut std::process::Command) {
    use std::os::windows::process::CommandExt;
    cmd.creation_flags(CREATE_NO_WINDOW);
}

/// Terminate `pid` and every process descended from it.
///
/// Returns whether the termination request was successfully delivered.
///
/// # The group *and* the process, on Unix
///
/// This used to signal `-pid` alone. That is the right thing for a child
/// [`configure_process_group`] made a group leader, and both callers in this
/// repository spawn that way — but a pid that is *not* a group leader has no
/// group of its own, `kill(-pid)` fails with `ESRCH`, and **nothing at all is
/// killed, not even the process named in the argument.** A function called
/// `kill_tree` that silently kills nothing is the worst available failure, and
/// its correctness rested entirely on every caller having spawned the process a
/// particular way.
///
/// That was never executed until this workspace was run on Linux for the first
/// time, where `a_write_failure_kills_the_process_it_can_no_longer_reach` —
/// which spawns its stand-in with plain `std::process::Command` — failed with
/// *"the process outlived a write failure — that is the leak"*.
///
/// So both are signalled. The direct `kill(pid)` is redundant whenever the group
/// exists (the leader is a member of its own group and has already been
/// signalled), and is the whole effect when it does not. This also matches what
/// the Windows arm has always done: `taskkill /PID x /T` kills `x` and its tree
/// with no precondition about how `x` was started.
pub fn kill_tree(pid: u32) -> bool {
    #[cfg(unix)]
    {
        let pid = pid as libc::pid_t;

        // SAFETY: `kill` with a negative pid targets a process group and with a
        // positive one a single process; neither can do anything but return
        // ESRCH for a target that is not there.
        let group = unsafe { libc::kill(-pid, libc::SIGTERM) } == 0;
        let single = unsafe { libc::kill(pid, libc::SIGTERM) } == 0;

        // Escalate on a detached thread so callers are never blocked. A well
        // behaved process will have exited long before this fires, leaving the
        // SIGKILL to fail harmlessly with ESRCH.
        std::thread::spawn(move || {
            std::thread::sleep(std::time::Duration::from_secs(2));
            unsafe {
                libc::kill(-pid, libc::SIGKILL);
                libc::kill(pid, libc::SIGKILL);
            }
        });

        group || single
    }

    #[cfg(windows)]
    {
        // `taskkill /T` walks the child tree; `/F` makes it unconditional.
        // Shelling out avoids hand-rolling Job Object lifetime management.
        let mut cmd = std::process::Command::new("taskkill");
        cmd.args(["/PID", &pid.to_string(), "/T", "/F"])
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null());
        no_window(&mut cmd);
        cmd.status().map(|s| s.success()).unwrap_or(false)
    }

    #[cfg(not(any(unix, windows)))]
    {
        let _ = pid;
        false
    }
}

/// Run a blocking kill body on the blocking pool, awaiting its result.
///
/// [`kill_tree`] is synchronous — on Windows it shells out to `taskkill` and
/// blocks on its exit; on Unix it issues signals and spawns a detached
/// escalation thread. Called inline from an async task, that blocking work runs
/// on a runtime worker; on a single-worker `current_thread` runtime it stalls
/// the sole worker. This seam offloads the body to a dedicated blocking thread
/// so no async worker is ever occupied by it. A join error (a panic in the
/// body) is reported as failure, matching every other "could not kill" path.
async fn spawn_blocking_kill<F>(f: F) -> bool
where
    F: FnOnce() -> bool + Send + 'static,
{
    tokio::task::spawn_blocking(f).await.unwrap_or(false)
}

/// The async counterpart to [`kill_tree`], for callers already on a runtime.
///
/// Offloads the blocking [`kill_tree`] body to the blocking pool so it cannot
/// stall a runtime worker (see [`spawn_blocking_kill`]). The synchronous
/// [`kill_tree`] is kept for [`Drop`] and other no-runtime callers.
pub async fn kill_tree_async(pid: u32) -> bool {
    spawn_blocking_kill(move || kill_tree(pid)).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::Arc;
    use std::time::Duration;

    /// No `flavor` on the attribute means a single-worker `current_thread`
    /// runtime — the one `notes.md` records as stalling when a blocking kill is
    /// run inline on the sole worker. This drives the offload seam directly with
    /// an injected, timing-free blocking body: it sets `started`, then parks on a
    /// channel until an async releaser (which can only run if the worker is free)
    /// wakes it. Inline, the worker parks, the releaser never runs, and the 5s
    /// timeout trips; offloaded to the blocking pool, both halves complete.
    #[test]
    fn windows_creation_flags_are_windowless_and_a_new_group() {
        // The supervised-child flags must carry both bits: no console window
        // (so running a project does not pop a terminal) and a new process
        // group (so tree-kill works). 0x0800_0000 | 0x0000_0200 == 0x0800_0200.
        assert_eq!(windows_creation_flags(), 0x0800_0200);
        assert_eq!(
            windows_creation_flags() & CREATE_NO_WINDOW,
            CREATE_NO_WINDOW
        );
        assert_eq!(
            windows_creation_flags() & CREATE_NEW_PROCESS_GROUP,
            CREATE_NEW_PROCESS_GROUP
        );
    }

    #[test]
    fn the_auxiliary_no_window_flag_suppresses_the_console_only() {
        // The raw-command helper suppresses the window but must NOT set the
        // process-group bit — taskkill/git/dotnet must not join a supervised
        // child's group.
        assert_eq!(CREATE_NO_WINDOW, 0x0800_0000);
        assert_eq!(CREATE_NO_WINDOW & CREATE_NEW_PROCESS_GROUP, 0);
    }

    #[tokio::test]
    async fn an_offloaded_kill_does_not_stall_the_current_thread_runtime() {
        let started = Arc::new(AtomicBool::new(false));
        let (tx, rx) = std::sync::mpsc::channel::<()>();

        let body_started = started.clone();
        let kill = spawn_blocking_kill(move || {
            body_started.store(true, Ordering::SeqCst);
            // Park the caller. On the single worker this is a deadlock; on the
            // blocking pool it merely waits for the releaser below.
            rx.recv().is_ok()
        });

        let releaser = async {
            while !started.load(Ordering::SeqCst) {
                tokio::task::yield_now().await;
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
            tx.send(())
                .expect("blocking body still waiting on the channel");
        };

        let (killed, ()) = tokio::time::timeout(Duration::from_secs(5), async {
            tokio::join!(kill, releaser)
        })
        .await
        .expect("the kill blocked the runtime worker");

        assert!(killed, "the released body reported success");
    }
}

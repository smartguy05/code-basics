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
        const CREATE_NEW_PROCESS_GROUP: u32 = 0x0000_0200;
        cmd.creation_flags(CREATE_NEW_PROCESS_GROUP);
    }

    #[cfg(not(any(unix, windows)))]
    {
        let _ = cmd;
    }
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
        std::process::Command::new("taskkill")
            .args(["/PID", &pid.to_string(), "/T", "/F"])
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .map(|s| s.success())
            .unwrap_or(false)
    }

    #[cfg(not(any(unix, windows)))]
    {
        let _ = pid;
        false
    }
}

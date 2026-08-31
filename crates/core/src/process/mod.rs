//! Process supervision: spawn, stream, cancel.
//!
//! Every external command the app runs — test runs, application launches, git
//! network operations — goes through here, so there is exactly one place that
//! understands environment layering, working directories and killing a process
//! *tree* rather than just the process we spawned.

mod chunker;
mod kill;
mod resolve;

pub use chunker::{LineSplitter, Utf8Chunker};
/// The windowless-spawn helper for raw [`std::process::Command`] sites (the ones
/// that must not join a supervised child's process group): `taskkill`, `git`,
/// the `dotnet` SDK evaluation, and the quality-gate runners. A no-op off
/// Windows. See [`kill::no_window`].
#[cfg(windows)]
pub use kill::no_window;
/// Re-exported as a **pair**, because using either alone is a bug.
///
/// [`kill_tree`]'s Unix body signals a process *group*, which only reaches
/// descendants if [`configure_process_group`] put the child into one at spawn
/// time. [`crate::lsp::transport`] needs both: it does not go through
/// [`Supervisor`] (that spawns with `stdin` null and blocks until exit, neither
/// of which suits a long-lived duplex protocol), and a language server that is
/// killed one process at a time orphans Roslyn's `BuildHost-netcore` /
/// `BuildHost-net472` children for the life of the user's session.
pub use kill::{configure_process_group, kill_tree, kill_tree_async};
/// Re-exported rather than making the module public, matching how `chunker`'s
/// two useful items are surfaced: the PATHEXT walk is the whole of `resolve`'s
/// interest to anyone else, and its helpers are not.
///
/// [`crate::lsp`] needs it because `typescript-language-server` and
/// `pyright-langserver` install as npm `.cmd` shims, which `CreateProcess`
/// cannot resolve from a bare name — the same reason `spawn` below uses it.
pub use resolve::resolve_program;

use std::collections::HashMap;
use std::process::Stdio;
use std::sync::Arc;
use std::time::Instant;

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use specta::Type;
use tokio::io::AsyncReadExt;
use tokio::sync::{mpsc, Mutex};

use crate::model::Invocation;

/// Which stream a chunk of output arrived on.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub enum Stream {
    Stdout,
    Stderr,
}

/// Events emitted over the lifetime of one spawned process.
//
// `rename_all` covers the variant names only; without `rename_all_fields`,
// `duration_ms` crosses IPC as snake_case and the UI reads `undefined`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Type)]
#[serde(
    tag = "type",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum ProcessEvent {
    /// The process is running. Carries the resolved command line for display.
    Started {
        pid: Option<u32>,
        program: String,
        args: Vec<String>,
        cwd: String,
    },
    /// A chunk of raw output, ANSI escapes and all, for the terminal view.
    Output { stream: Stream, text: String },
    /// The process ended. `code` is `None` when killed by a signal.
    Exited {
        code: Option<i32>,
        success: bool,
        duration_ms: u64,
        /// True when the exit was the result of an explicit cancel.
        cancelled: bool,
    },
    /// The process could not be started at all, or supervision failed.
    Failed { message: String },
}

/// Handle to one running process, held by the [`Supervisor`].
struct Running {
    pid: Option<u32>,
    /// Set before killing so the exit is reported as a cancellation rather
    /// than a failure.
    cancel: Arc<std::sync::atomic::AtomicBool>,
    /// Distinguishes this run from a later run that reused the same id. A
    /// superseded run's exit must not evict the run that replaced it, so the
    /// exit removes the map entry only while this token still owns it.
    token: u64,
}

/// Owns every process the app has started.
///
/// Cloning is cheap and shares state, so the Tauri layer can keep one in
/// managed state and reach it from any command.
#[derive(Clone, Default)]
pub struct Supervisor {
    running: Arc<Mutex<HashMap<String, Running>>>,
    /// The running-process registry, when this supervisor should feed the
    /// Running panel and crash-orphan file. `None` for the plain [`Supervisor`]
    /// the process tests use, which record nothing.
    store: Option<crate::running::RunningStore>,
    /// Monotonic source of per-run ownership tokens (see [`Running::token`]).
    next_token: Arc<std::sync::atomic::AtomicU64>,
}

impl Supervisor {
    pub fn new() -> Self {
        Self::default()
    }

    /// A supervisor that records its runs into `store` (see [`Supervisor::run_tracked`]).
    pub fn with_store(store: crate::running::RunningStore) -> Self {
        Self {
            running: Arc::new(Mutex::new(HashMap::new())),
            store: Some(store),
            next_token: Arc::new(std::sync::atomic::AtomicU64::new(0)),
        }
    }

    /// Run `invocation` to completion, streaming events to `events`.
    ///
    /// `id` names the process for later [`Supervisor::cancel`] calls. Reusing an
    /// id restarts: the process already running under it is stopped and awaited
    /// (freeing its port) before the replacement spawns, and the registry then
    /// tracks the replacement.
    ///
    /// Returns the exit code, or `None` if the process was signalled or
    /// cancelled.
    pub async fn run(
        &self,
        id: &str,
        invocation: &Invocation,
        events: mpsc::Sender<ProcessEvent>,
    ) -> Result<Option<i32>> {
        self.run_inner(id, invocation, events, None).await
    }

    /// As [`Supervisor::run`], but also record the process in the running-process
    /// registry (if this supervisor has one) for the Running panel and the
    /// crash-orphan file. The record is added the instant the pid is known and
    /// removed the instant the process is reaped, so the registry's live set is
    /// always this map's live set.
    pub async fn run_tracked(
        &self,
        id: &str,
        invocation: &Invocation,
        events: mpsc::Sender<ProcessEvent>,
        meta: crate::running::RunMeta,
    ) -> Result<Option<i32>> {
        self.run_inner(id, invocation, events, Some(meta)).await
    }

    async fn run_inner(
        &self,
        id: &str,
        invocation: &Invocation,
        events: mpsc::Sender<ProcessEvent>,
        meta: Option<crate::running::RunMeta>,
    ) -> Result<Option<i32>> {
        let started = Instant::now();

        // A bare name like `pnpm` only exists as a `.cmd` shim on Windows,
        // which CreateProcess will not resolve; see process/resolve.rs.
        let mut cmd = tokio::process::Command::new(resolve::resolve_program(&invocation.program));
        cmd.args(&invocation.args)
            .current_dir(&invocation.cwd)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            // Safety net for a runtime teardown that drops this future without
            // going through `cancel` (e.g. the process is left holding the
            // `Child` when the app's Tokio runtime shuts down). This reaps only
            // the direct child, not its tree — the app's exit sweep is what
            // kills grandchildren like `dotnet run`'s built assembly.
            .kill_on_drop(true);

        for (k, v) in &invocation.env {
            cmd.env(k, v);
        }

        // Force machine-friendly, colour-capable output where the ecosystem
        // supports it. All of these are overridable by the config's own env.
        if !invocation.env.contains_key("FORCE_COLOR") {
            cmd.env("FORCE_COLOR", "1");
        }
        // .NET's console logger turns colour off when output is redirected,
        // which is exactly how everything here runs. This configuration key
        // (the env form of Logging:Console:FormatterOptions:ColorBehavior)
        // turns it back on for the terminal view.
        if !invocation
            .env
            .contains_key("Logging__Console__FormatterOptions__ColorBehavior")
        {
            cmd.env(
                "Logging__Console__FormatterOptions__ColorBehavior",
                "Enabled",
            );
        }

        kill::configure_process_group(&mut cmd);

        // Restart safety: if this id is already running, stop the old process
        // and wait for the tree-kill to complete before spawning the new one.
        // Otherwise the replacement races the original for its port and the
        // original is orphaned. `cancel` is a no-op returning `false` when
        // nothing is registered under `id`, so a first run pays nothing.
        self.cancel(id).await;

        let mut child = match cmd.spawn() {
            Ok(c) => c,
            Err(e) => {
                let message = format!(
                    "failed to start `{}` in {}: {e}",
                    invocation.program,
                    invocation.cwd.display()
                );
                let _ = events
                    .send(ProcessEvent::Failed {
                        message: message.clone(),
                    })
                    .await;
                return Err(anyhow::anyhow!(message));
            }
        };

        let pid = child.id();
        let cancel = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let token = self
            .next_token
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        self.running.lock().await.insert(
            id.to_string(),
            Running {
                pid,
                cancel: cancel.clone(),
                token,
            },
        );

        // Record in the running-process registry, if tracked. Only with a pid —
        // a pid-less spawn cannot be found, killed or identity-checked later.
        if let (Some(store), Some(meta), Some(pid)) = (&self.store, &meta, pid) {
            store.record(crate::running::observe(
                pid,
                id,
                meta.clone(),
                &invocation.program,
            ));
        }

        let _ = events
            .send(ProcessEvent::Started {
                pid,
                program: invocation.program.clone(),
                args: invocation.args.clone(),
                cwd: invocation.cwd.display().to_string(),
            })
            .await;

        let stdout = child.stdout.take().context("stdout was not piped")?;
        let stderr = child.stderr.take().context("stderr was not piped")?;

        let out_task = tokio::spawn(pump(stdout, Stream::Stdout, events.clone()));
        let err_task = tokio::spawn(pump(stderr, Stream::Stderr, events.clone()));

        let status = child.wait().await;

        // Drain both pipes before reporting exit, so no output is lost after
        // the process ends.
        let _ = out_task.await;
        let _ = err_task.await;

        // Remove only while this run still owns the id. A restart cancels this
        // run and installs a replacement under the same id; when this (now
        // superseded) run finally reaps, the entry belongs to the replacement
        // and must be left alone — both in the map and in the registry.
        {
            let mut guard = self.running.lock().await;
            if guard.get(id).map(|r| r.token) == Some(token) {
                guard.remove(id);
                // Deregister from the running-process registry on the same reap,
                // so the panel and the crash-orphan file stop showing it the
                // moment it ends.
                if let (Some(store), Some(meta)) = (&self.store, &meta) {
                    store.remove(&meta.root, id);
                }
            }
        }

        let cancelled = cancel.load(std::sync::atomic::Ordering::SeqCst);
        let duration_ms = started.elapsed().as_millis() as u64;

        match status {
            Ok(status) => {
                let code = status.code();
                let _ = events
                    .send(ProcessEvent::Exited {
                        code,
                        success: status.success() && !cancelled,
                        duration_ms,
                        cancelled,
                    })
                    .await;
                Ok(code)
            }
            Err(e) => {
                let message = format!("failed while waiting for process: {e}");
                let _ = events
                    .send(ProcessEvent::Failed {
                        message: message.clone(),
                    })
                    .await;
                Err(anyhow::anyhow!(message))
            }
        }
    }

    /// Terminate a running process and its children.
    ///
    /// Returns `false` when no process is registered under `id` — normally
    /// because it already exited.
    pub async fn cancel(&self, id: &str) -> bool {
        let (target, token) = {
            let guard = self.running.lock().await;
            match guard.get(id) {
                Some(r) => {
                    r.cancel.store(true, std::sync::atomic::Ordering::SeqCst);
                    (r.pid, r.token)
                }
                None => return false,
            }
        };

        match target {
            Some(pid) => {
                if kill::kill_tree_async(pid).await {
                    return true;
                }

                // A process can be reaped between the registry lookup above
                // and `taskkill` reaching it. In that race the cancellation
                // request still succeeded; do not report failure merely
                // because the target disappeared first. Give the waiter a
                // bounded chance to remove this exact run, while preserving a
                // real kill failure when it continues to own the id.
                for _ in 0..50 {
                    let still_owns_id =
                        self.running.lock().await.get(id).map(|r| r.token) == Some(token);
                    if !still_owns_id {
                        return true;
                    }
                    tokio::time::sleep(std::time::Duration::from_millis(10)).await;
                }
                false
            }
            // Spawned but no pid means it already exited.
            None => false,
        }
    }

    /// Ids of every process currently running.
    pub async fn running_ids(&self) -> Vec<String> {
        self.running.lock().await.keys().cloned().collect()
    }

    /// The pid of a running process, or `None` if it is not running or never
    /// reported one.
    ///
    /// The two `None`s are deliberately not distinguished: both mean there is
    /// nothing to attach to or signal, and a caller that acted differently on
    /// "running but pid-less" would be acting on a process that has already
    /// gone.
    pub async fn pid(&self, id: &str) -> Option<u32> {
        self.running.lock().await.get(id).and_then(|r| r.pid)
    }

    /// Every running process with its pid, for callers that need both.
    ///
    /// Sorted by id so a listing is stable between calls — a `HashMap` would
    /// otherwise reorder a menu under the user's cursor.
    pub async fn running(&self) -> Vec<(String, Option<u32>)> {
        let mut all: Vec<(String, Option<u32>)> = self
            .running
            .lock()
            .await
            .iter()
            .map(|(id, r)| (id.clone(), r.pid))
            .collect();
        all.sort_by(|a, b| a.0.cmp(&b.0));
        all
    }

    pub async fn is_running(&self, id: &str) -> bool {
        self.running.lock().await.contains_key(id)
    }
}

/// Read one pipe to EOF, forwarding decoded chunks as they arrive.
async fn pump<R>(mut reader: R, stream: Stream, events: mpsc::Sender<ProcessEvent>)
where
    R: tokio::io::AsyncRead + Unpin,
{
    let mut chunker = Utf8Chunker::new();
    let mut buf = [0u8; 8192];

    loop {
        match reader.read(&mut buf).await {
            Ok(0) => break,
            Ok(n) => {
                let text = chunker.push(&buf[..n]);
                if !text.is_empty()
                    && events
                        .send(ProcessEvent::Output { stream, text })
                        .await
                        .is_err()
                {
                    // Receiver dropped — nothing left to report to.
                    return;
                }
            }
            Err(_) => break,
        }
    }

    let tail = chunker.finish();
    if !tail.is_empty() {
        let _ = events
            .send(ProcessEvent::Output { stream, text: tail })
            .await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::Invocation;
    use std::collections::BTreeMap;
    use std::path::PathBuf;

    fn invocation(program: &str, args: &[&str]) -> Invocation {
        Invocation {
            program: program.to_string(),
            args: args.iter().map(|s| s.to_string()).collect(),
            cwd: std::env::temp_dir(),
            env: BTreeMap::new(),
            report: None,
            coverage: None,
            warnings: Vec::new(),
        }
    }

    async fn collect(inv: &Invocation) -> (Vec<ProcessEvent>, Option<i32>) {
        let sup = Supervisor::new();
        let (tx, mut rx) = mpsc::channel(256);
        let code = sup.run("test", inv, tx).await.ok().flatten();
        let mut events = Vec::new();
        while let Ok(e) = rx.try_recv() {
            events.push(e);
        }
        (events, code)
    }

    fn output_text(events: &[ProcessEvent], want: Stream) -> String {
        events
            .iter()
            .filter_map(|e| match e {
                ProcessEvent::Output { stream, text } if *stream == want => Some(text.as_str()),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn events_serialise_with_the_keys_the_ui_reads() {
        // `src/ipc/types.ts` mirrors this by hand, like the model types.
        let event = ProcessEvent::Exited {
            code: Some(1),
            success: false,
            duration_ms: 5,
            cancelled: false,
        };
        let json = serde_json::to_value(&event).unwrap();
        let mut keys: Vec<String> = json.as_object().unwrap().keys().cloned().collect();
        keys.sort();

        assert_eq!(keys, ["cancelled", "code", "durationMs", "success", "type"]);
        assert_eq!(json["type"], "exited");
    }

    #[tokio::test]
    async fn streams_stdout_and_reports_success() {
        let inv = invocation("sh", &["-c", "printf 'hello\\nworld\\n'"]);
        let (events, code) = collect(&inv).await;

        assert_eq!(code, Some(0));
        assert_eq!(output_text(&events, Stream::Stdout), "hello\nworld\n");
        assert!(matches!(events.first(), Some(ProcessEvent::Started { .. })));
        assert!(matches!(
            events.last(),
            Some(ProcessEvent::Exited {
                success: true,
                cancelled: false,
                ..
            })
        ));
    }

    #[tokio::test]
    async fn separates_stderr_from_stdout() {
        let inv = invocation("sh", &["-c", "printf 'out' ; printf 'err' >&2"]);
        let (events, _) = collect(&inv).await;

        assert_eq!(output_text(&events, Stream::Stdout), "out");
        assert_eq!(output_text(&events, Stream::Stderr), "err");
    }

    #[tokio::test]
    async fn reports_nonzero_exit_as_failure() {
        let inv = invocation("sh", &["-c", "exit 3"]);
        let (events, code) = collect(&inv).await;

        assert_eq!(code, Some(3));
        assert!(matches!(
            events.last(),
            Some(ProcessEvent::Exited {
                success: false,
                code: Some(3),
                ..
            })
        ));
    }

    #[tokio::test]
    async fn missing_program_fails_without_panicking() {
        let inv = invocation("definitely-not-a-real-program-xyz", &[]);
        let sup = Supervisor::new();
        let (tx, mut rx) = mpsc::channel(16);

        assert!(sup.run("test", &inv, tx).await.is_err());
        assert!(matches!(rx.try_recv(), Ok(ProcessEvent::Failed { .. })));
    }

    /// The reproduction for "failed to start `pnpm` ...: program not found".
    /// pnpm/npm/yarn install as `.cmd` shims on Windows, which `CreateProcess`
    /// will not resolve from a bare name. Guarded: only asserts on a machine
    /// where pnpm is actually installed as a shim.
    #[cfg(windows)]
    #[tokio::test]
    async fn a_package_manager_cmd_shim_spawns_by_bare_name() {
        use std::path::Path;
        if resolve::resolve_program("pnpm") == Path::new("pnpm") {
            return; // no pnpm on this machine - nothing to prove
        }

        let inv = invocation("pnpm", &["--version"]);
        let (events, code) = collect(&inv).await;

        assert_eq!(code, Some(0), "pnpm did not spawn; events: {events:?}");
        assert!(!output_text(&events, Stream::Stdout).trim().is_empty());
    }

    #[tokio::test]
    async fn env_and_cwd_are_applied() {
        let dir = tempfile::tempdir().unwrap();
        let mut inv = invocation("sh", &["-c", "printf '%s|%s' \"$CB_MARKER\" \"$(pwd -P)\""]);
        inv.cwd = dir.path().to_path_buf();
        inv.env.insert("CB_MARKER".into(), "set".into());

        let (events, _) = collect(&inv).await;
        let out = output_text(&events, Stream::Stdout);
        let (marker, cwd) = out.split_once('|').expect("marker and cwd");

        assert_eq!(marker, "set");
        // The unique tempdir name is enough to prove the cwd was applied, and
        // comparing only it stays immune to path-notation differences: /tmp is
        // a symlink on macOS, and on Windows `sh` is Git Bash, whose `pwd`
        // prints MSYS-style paths (`/c/Users/...`).
        assert_eq!(
            PathBuf::from(cwd).file_name(),
            dir.path().file_name(),
            "shell cwd {cwd} does not match {}",
            dir.path().display()
        );
    }

    #[tokio::test]
    async fn cancel_stops_a_long_running_process() {
        let sup = Supervisor::new();
        let (tx, mut rx) = mpsc::channel(64);
        let inv = invocation("sh", &["-c", "sleep 60"]);

        let runner = {
            let sup = sup.clone();
            tokio::spawn(async move { sup.run("long", &inv, tx).await })
        };

        // Wait for it to actually be up before cancelling. Bounded, because an
        // unbounded spin turns a failed spawn into a hung suite with no output.
        tokio::time::timeout(std::time::Duration::from_secs(30), async {
            loop {
                if sup.is_running("long").await {
                    break;
                }
                tokio::time::sleep(std::time::Duration::from_millis(10)).await;
            }
        })
        .await
        .expect("process never started - is sh on PATH? (run cargo test from Git Bash)");
        assert!(sup.cancel("long").await);

        let result = tokio::time::timeout(std::time::Duration::from_secs(10), runner)
            .await
            .expect("cancel should terminate the process promptly");
        assert!(result.is_ok());

        let mut saw_cancelled_exit = false;
        while let Ok(e) = rx.try_recv() {
            if let ProcessEvent::Exited {
                cancelled, success, ..
            } = e
            {
                saw_cancelled_exit = cancelled && !success;
            }
        }
        assert!(
            saw_cancelled_exit,
            "exit should be reported as a cancellation"
        );
        assert!(!sup.is_running("long").await);
    }

    #[tokio::test]
    async fn cancel_kills_the_whole_process_tree() {
        // The shell spawns a grandchild that writes to a marker file after a
        // delay. Killing only the shell would leave the grandchild alive to
        // create it — exactly the orphaned-`dotnet run` failure mode.
        let dir = tempfile::tempdir().unwrap();
        let marker = dir.path().join("orphan-survived");
        let script = format!("(sleep 2; touch {}) & sleep 60", marker.display());

        let sup = Supervisor::new();
        let (tx, _rx) = mpsc::channel(64);
        let inv = invocation("sh", &["-c", &script]);

        let runner = {
            let sup = sup.clone();
            tokio::spawn(async move { sup.run("tree", &inv, tx).await })
        };
        tokio::time::timeout(std::time::Duration::from_secs(30), async {
            loop {
                if sup.is_running("tree").await {
                    break;
                }
                tokio::time::sleep(std::time::Duration::from_millis(10)).await;
            }
        })
        .await
        .expect("process never started - is sh on PATH? (run cargo test from Git Bash)");
        sup.cancel("tree").await;
        let _ = tokio::time::timeout(std::time::Duration::from_secs(10), runner).await;

        // Outlive the grandchild's delay, then confirm it never ran.
        tokio::time::sleep(std::time::Duration::from_secs(3)).await;
        assert!(
            !marker.exists(),
            "grandchild survived the kill — the process group was not terminated"
        );
    }

    #[tokio::test]
    async fn a_running_process_can_be_found_by_id_with_its_pid() {
        // The pid is what makes a running process inspectable: the object
        // inspector attaches by pid, and until now the supervisor was the only
        // thing that knew it and would not say.
        let sup = Supervisor::new();
        let (tx, _rx) = mpsc::channel(64);
        let inv = invocation("sh", &["-c", "sleep 60"]);

        let runner = {
            let sup = sup.clone();
            tokio::spawn(async move { sup.run("app", &inv, tx).await })
        };
        tokio::time::timeout(std::time::Duration::from_secs(30), async {
            loop {
                if sup.is_running("app").await {
                    break;
                }
                tokio::time::sleep(std::time::Duration::from_millis(10)).await;
            }
        })
        .await
        .expect("process never started - is sh on PATH? (run cargo test from Git Bash)");

        let pid = sup.pid("app").await.expect("a running process has a pid");
        assert!(pid > 0);
        assert_eq!(sup.running().await, vec![("app".to_string(), Some(pid))]);

        sup.cancel("app").await;
        let _ = tokio::time::timeout(std::time::Duration::from_secs(10), runner).await;

        // Nothing to attach to once it has gone, and saying otherwise would
        // send a capture at a pid the OS may since have reused.
        assert_eq!(sup.pid("app").await, None);
        assert!(sup.running().await.is_empty());
    }

    async fn wait_running(sup: &Supervisor, id: &str) {
        tokio::time::timeout(std::time::Duration::from_secs(30), async {
            loop {
                if sup.is_running(id).await {
                    break;
                }
                tokio::time::sleep(std::time::Duration::from_millis(10)).await;
            }
        })
        .await
        .expect("process never started - is sh on PATH? (run cargo test from Git Bash)");
    }

    #[tokio::test]
    async fn restarting_an_id_stops_the_old_process_and_keeps_the_new_cancellable() {
        // Reproduction for the restart bug: reusing an id must (a) stop the old
        // process so it stops holding its port, and (b) leave the registry
        // pointing at the NEW process — the old run's late exit must not evict
        // the entry that replaced it, or the new run becomes uncancellable.
        let sup = Supervisor::new();

        let (tx1, _rx1) = mpsc::channel(64);
        let run1 = {
            let sup = sup.clone();
            let inv = invocation("sh", &["-c", "sleep 60"]);
            tokio::spawn(async move { sup.run("svc", &inv, tx1).await })
        };
        wait_running(&sup, "svc").await;
        let pid1 = sup.pid("svc").await.expect("first run has a pid");

        // Restart under the same id.
        let (tx2, _rx2) = mpsc::channel(64);
        let run2 = {
            let sup = sup.clone();
            let inv = invocation("sh", &["-c", "sleep 60"]);
            tokio::spawn(async move { sup.run("svc", &inv, tx2).await })
        };

        // The registry must move to a *different* pid — proving the old process
        // was stopped and the new one recorded in its place.
        let pid2 = tokio::time::timeout(std::time::Duration::from_secs(30), async {
            loop {
                if let Some(p) = sup.pid("svc").await {
                    if p != pid1 {
                        break p;
                    }
                }
                tokio::time::sleep(std::time::Duration::from_millis(10)).await;
            }
        })
        .await
        .expect("restart never produced a new pid");
        assert_ne!(pid1, pid2);

        // The first run's task ends because its process was killed.
        let _ = tokio::time::timeout(std::time::Duration::from_secs(10), run1)
            .await
            .expect("old run should end after being superseded");

        // Only the new process is registered, and it is still cancellable.
        assert_eq!(sup.pid("svc").await, Some(pid2));
        assert!(
            sup.cancel("svc").await,
            "the new run must still be cancellable after the old one exited"
        );
        let _ = tokio::time::timeout(std::time::Duration::from_secs(10), run2)
            .await
            .expect("new run should end when cancelled");
        assert!(!sup.is_running("svc").await);
    }

    #[tokio::test]
    async fn an_unknown_id_has_no_pid() {
        let sup = Supervisor::new();
        assert_eq!(sup.pid("never-started").await, None);
        assert!(sup.running().await.is_empty());
    }

    #[tokio::test]
    async fn cancelling_an_unknown_id_is_harmless() {
        let sup = Supervisor::new();
        assert!(!sup.cancel("never-started").await);
    }
}

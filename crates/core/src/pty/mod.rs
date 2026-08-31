//! Interactive pseudo-terminal sessions.
//!
//! This is the app's **second** process-spawning path, and the only
//! bidirectional one. [`crate::process::Supervisor`] spawns with `stdin` null
//! and streams output to exit — right for a test run, useless for a shell you
//! type into. A [`PtyManager`] allocates a real pseudo-terminal (ConPTY on
//! Windows, forkpty on Unix) so a program sees a TTY, keeps stdin open for
//! keystrokes, and can be resized as the window changes. Claude Code's TUI, or
//! any interactive program, runs in it unchanged.
//!
//! Like `Supervisor`, a `PtyManager` is a cheap-to-clone handle over a shared
//! map of live sessions keyed by an id the caller mints, so the Tauri layer can
//! hold one in managed state and reach any session by id.
//!
//! The map is a `std::sync::Mutex`, not a tokio one, on purpose: every critical
//! section is a quick map operation with nothing awaited inside it, and the
//! reader and waiter threads that touch it run on plain OS threads with no async
//! runtime — a tokio mutex would need a runtime handle they do not have.

mod argv;
pub use argv::{batch_argv_refusal, check_batch_argv, is_batch_target};

mod shell;

pub use shell::{clamp_size, default_shell};

mod model;
pub use model::{PtySpec, TerminalEvent};

use std::collections::HashMap;
use std::io::{Read, Write};
use std::sync::{Arc, Mutex};

use anyhow::{Context, Result};
use portable_pty::{native_pty_system, CommandBuilder, MasterPty, PtySize};
use tokio::sync::mpsc;

use crate::process::{kill_tree_async, resolve_program, Utf8Chunker};

/// One live terminal, held by the [`PtyManager`].
struct PtySession {
    /// The control end, kept for [`PtyManager::resize`]. Dropping it closes the
    /// pseudo-terminal.
    master: Box<dyn MasterPty + Send>,
    /// The write end, for keystrokes. A separate handle from `master`, so a
    /// write does not contend with a resize.
    writer: Box<dyn Write + Send>,
    /// The shell's process id, for tree-killing on close. `None` if the OS
    /// never reported one (already gone).
    pid: Option<u32>,
}

/// Owns every terminal session the app has opened.
#[derive(Clone, Default)]
pub struct PtyManager {
    sessions: Arc<Mutex<HashMap<String, PtySession>>>,
    /// The running-process registry, when terminals should feed the Running
    /// panel and crash-orphan file. `None` for the plain manager the pty tests
    /// use, which records nothing.
    store: Option<crate::running::RunningStore>,
}

impl PtyManager {
    pub fn new() -> Self {
        Self::default()
    }

    /// A manager that records its terminals into `store` (see [`PtyManager::open_tracked`]).
    pub fn with_store(store: crate::running::RunningStore) -> Self {
        Self {
            sessions: Arc::new(Mutex::new(HashMap::new())),
            store: Some(store),
        }
    }

    /// Open a terminal running `spec` and stream its output to `events`.
    ///
    /// `id` names the session for later [`PtyManager::write`],
    /// [`PtyManager::resize`] and [`PtyManager::close`] calls. Returns once the
    /// shell is spawned and registered; output then arrives asynchronously on
    /// `events` until the shell exits.
    ///
    /// A blocking reader thread pumps the master, because portable-pty's reader
    /// is a synchronous `Read` and a PTY has no async form; a second thread
    /// waits on the child and reports the exit. Both send over `events` with
    /// `blocking_send`, which is legal off the async runtime.
    pub fn open(&self, id: &str, spec: PtySpec, events: mpsc::Sender<TerminalEvent>) -> Result<()> {
        self.open_inner(id, spec, events, None)
    }

    /// As [`PtyManager::open`], but also record the terminal in the running-process
    /// registry (if this manager has one) so it shows in the Running panel and is
    /// recoverable as an orphan after a crash. `meta` carries the workspace root
    /// and the initial title; a later rename is reflected with
    /// [`crate::running::RunningStore::update_label`].
    pub fn open_tracked(
        &self,
        id: &str,
        spec: PtySpec,
        events: mpsc::Sender<TerminalEvent>,
        meta: crate::running::RunMeta,
    ) -> Result<()> {
        self.open_inner(id, spec, events, Some(meta))
    }

    fn open_inner(
        &self,
        id: &str,
        spec: PtySpec,
        events: mpsc::Sender<TerminalEvent>,
        meta: Option<crate::running::RunMeta>,
    ) -> Result<()> {
        // A bare name only exists as a shim on Windows, which the spawn cannot
        // resolve; resolve it the same way `Supervisor` does. On Unix this is an
        // identity. Resolved **first**, before anything is allocated, because
        // whether the target is a batch shim decides whether these arguments can
        // be passed at all — see `argv`.
        let program = resolve_program(&spec.shell);
        // Refuse before spawning, never after: a `.cmd`/`.bat` target means
        // `cmd.exe` re-parses the command line portable-pty built with MSVC
        // quoting alone, so an argument carrying `&`, `%` and friends would run
        // as something else. A real executable is unaffected and nothing here
        // applies to it.
        if let Err(reason) = argv::check_batch_argv(&program, &spec.args) {
            anyhow::bail!(reason);
        }

        let (cols, rows) = shell::clamp_size(spec.cols, spec.rows);

        let pty_system = native_pty_system();
        let pair = pty_system
            .openpty(PtySize {
                rows,
                cols,
                pixel_width: 0,
                pixel_height: 0,
            })
            .context("failed to allocate a pseudo-terminal")?;

        let mut cmd = CommandBuilder::new(&program);
        cmd.args(&spec.args);
        cmd.cwd(&spec.cwd);
        // The child inherits the full parent environment by default (PATH and
        // all), which is what we want — except for the markers that say this
        // process is *inside* a Claude Code session. Left in place they make a
        // `claude` run in the terminal act as a nested child (transcripts off,
        // parent IPC socket reused); a user-opened terminal is a fresh
        // top-level shell, so strip them. `spec.env` below can put any back.
        for (key, _) in std::env::vars_os() {
            if let Some(key) = key.to_str() {
                if shell::is_session_marker(key) {
                    cmd.env_remove(key);
                }
            }
        }
        // TERM makes interactive programs emit colour and use the alternate
        // screen; the caller's env still wins.
        if !spec.env.contains_key("TERM") {
            cmd.env("TERM", "xterm-256color");
        }
        for (k, v) in &spec.env {
            cmd.env(k, v);
        }

        let portable_pty::PtyPair { slave, master } = pair;
        let mut child = slave
            .spawn_command(cmd)
            .with_context(|| format!("failed to start `{}`", spec.shell))?;
        // Drop the slave now: with it still open the master never sees EOF when
        // the child exits, and the reader thread would hang forever.
        drop(slave);

        let pid = child.process_id();
        let reader = master
            .try_clone_reader()
            .context("failed to open the terminal reader")?;
        let writer = master
            .take_writer()
            .context("failed to open the terminal writer")?;

        self.sessions.lock().unwrap().insert(
            id.to_string(),
            PtySession {
                master,
                writer,
                pid,
            },
        );

        // Record in the running-process registry, if tracked. Only with a pid —
        // a pid-less terminal cannot be found, killed or identity-checked later.
        if let (Some(store), Some(meta), Some(pid)) = (&self.store, &meta, pid) {
            store.record(crate::running::observe(pid, id, meta.clone(), &spec.shell));
        }

        // Reader: raw bytes → decoded chunks → Output events, to EOF.
        let out_events = events.clone();
        std::thread::spawn(move || pump_reader(reader, out_events));

        // Waiter: block on the child, then report the exit and drop the session
        // so the map does not grow without bound. A plain OS thread — the map's
        // `std::sync::Mutex` is why this needs no runtime handle.
        let sessions = Arc::clone(&self.sessions);
        let id = id.to_string();
        // Move the registry handle + root into the waiter so the reap also
        // deregisters the terminal from the Running panel and orphan file.
        let store = self.store.clone();
        let root = meta.map(|m| m.root);
        std::thread::spawn(move || {
            let event = match child.wait() {
                Ok(status) => TerminalEvent::Exited {
                    code: Some(status.exit_code() as i32),
                    success: status.success(),
                },
                Err(e) => TerminalEvent::Failed {
                    message: format!("terminal ended unexpectedly: {e}"),
                },
            };
            // Remove before announcing, so a client acting on the exit sees the
            // session already gone.
            sessions.lock().unwrap().remove(&id);
            if let (Some(store), Some(root)) = (&store, &root) {
                store.remove(root, &id);
            }
            let _ = events.blocking_send(event);
        });

        Ok(())
    }

    /// Send keystrokes (or any bytes) to a terminal. Errors if the session is
    /// gone.
    pub fn write(&self, id: &str, data: &str) -> Result<()> {
        let mut guard = self.sessions.lock().unwrap();
        let session = guard
            .get_mut(id)
            .with_context(|| format!("no terminal {id}"))?;
        session.writer.write_all(data.as_bytes())?;
        session.writer.flush()?;
        Ok(())
    }

    /// Resize a terminal. Silently does nothing when the session is gone — a
    /// resize racing a close is ordinary and not worth an error.
    pub fn resize(&self, id: &str, cols: u16, rows: u16) -> Result<()> {
        let (cols, rows) = shell::clamp_size(cols, rows);
        let guard = self.sessions.lock().unwrap();
        if let Some(session) = guard.get(id) {
            session
                .master
                .resize(PtySize {
                    rows,
                    cols,
                    pixel_width: 0,
                    pixel_height: 0,
                })
                .context("failed to resize the terminal")?;
        }
        Ok(())
    }

    /// Close a terminal, killing its whole process tree.
    ///
    /// Tree-killing rather than dropping the master alone: a shell running
    /// `claude` has spawned a `node` child, and closing the pane must not leave
    /// it orphaned — the same reason `Supervisor::cancel` kills a tree. Returns
    /// `false` when no session was open under `id` (already exited).
    pub async fn close(&self, id: &str) -> bool {
        // Take the session out under the lock, then kill off-lock: `kill_tree`
        // spawns a helper process and must not run while the map is held.
        let pid = {
            let mut guard = self.sessions.lock().unwrap();
            match guard.remove(id) {
                // Dropping the removed session drops the master, closing the
                // PTY; the pid is what the tree-kill needs.
                Some(session) => session.pid,
                None => return false,
            }
        };
        if let Some(pid) = pid {
            kill_tree_async(pid).await;
        }
        true
    }

    /// Whether a session is open under `id`.
    pub fn is_open(&self, id: &str) -> bool {
        self.sessions.lock().unwrap().contains_key(id)
    }

    /// The ids of every open session, sorted so a listing is stable.
    pub fn list(&self) -> Vec<String> {
        let mut ids: Vec<String> = self.sessions.lock().unwrap().keys().cloned().collect();
        ids.sort();
        ids
    }
}

/// Read the master to EOF, forwarding decoded chunks as `Output` events.
fn pump_reader(mut reader: Box<dyn Read + Send>, events: mpsc::Sender<TerminalEvent>) {
    let mut chunker = Utf8Chunker::new();
    let mut buf = [0u8; 8192];

    loop {
        match reader.read(&mut buf) {
            Ok(0) => break,
            Ok(n) => {
                let text = chunker.push(&buf[..n]);
                if !text.is_empty()
                    && events
                        .blocking_send(TerminalEvent::Output { text })
                        .is_err()
                {
                    return; // receiver dropped — nothing to report to
                }
            }
            Err(_) => break,
        }
    }

    let tail = chunker.finish();
    if !tail.is_empty() {
        let _ = events.blocking_send(TerminalEvent::Output { text: tail });
    }
}

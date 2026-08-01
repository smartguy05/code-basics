//! Process supervision: spawn, stream, cancel.
//!
//! Every external command the app runs — test runs, application launches, git
//! network operations — goes through here, so there is exactly one place that
//! understands environment layering, working directories and killing a process
//! *tree* rather than just the process we spawned.

mod chunker;
mod kill;

pub use chunker::{LineSplitter, Utf8Chunker};

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
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Type)]
#[serde(tag = "type", rename_all = "camelCase")]
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
}

/// Owns every process the app has started.
///
/// Cloning is cheap and shares state, so the Tauri layer can keep one in
/// managed state and reach it from any command.
#[derive(Clone, Default)]
pub struct Supervisor {
    running: Arc<Mutex<HashMap<String, Running>>>,
}

impl Supervisor {
    pub fn new() -> Self {
        Self::default()
    }

    /// Run `invocation` to completion, streaming events to `events`.
    ///
    /// `id` names the process for later [`Supervisor::cancel`] calls; reusing an
    /// id replaces the previous entry, which is what a "restart" does.
    ///
    /// Returns the exit code, or `None` if the process was signalled or
    /// cancelled.
    pub async fn run(
        &self,
        id: &str,
        invocation: &Invocation,
        events: mpsc::Sender<ProcessEvent>,
    ) -> Result<Option<i32>> {
        let started = Instant::now();

        let mut cmd = tokio::process::Command::new(&invocation.program);
        cmd.args(&invocation.args)
            .current_dir(&invocation.cwd)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());

        for (k, v) in &invocation.env {
            cmd.env(k, v);
        }

        // Force machine-friendly, colour-capable output where the ecosystem
        // supports it. Both are overridable by the config's own env.
        if !invocation.env.contains_key("FORCE_COLOR") {
            cmd.env("FORCE_COLOR", "1");
        }

        kill::configure_process_group(&mut cmd);

        let mut child = match cmd.spawn() {
            Ok(c) => c,
            Err(e) => {
                let message = format!(
                    "failed to start `{}` in {}: {e}",
                    invocation.program,
                    invocation.cwd.display()
                );
                let _ = events.send(ProcessEvent::Failed { message: message.clone() }).await;
                return Err(anyhow::anyhow!(message));
            }
        };

        let pid = child.id();
        let cancel = Arc::new(std::sync::atomic::AtomicBool::new(false));
        self.running
            .lock()
            .await
            .insert(id.to_string(), Running { pid, cancel: cancel.clone() });

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

        self.running.lock().await.remove(id);

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
                let _ = events.send(ProcessEvent::Failed { message: message.clone() }).await;
                Err(anyhow::anyhow!(message))
            }
        }
    }

    /// Terminate a running process and its children.
    ///
    /// Returns `false` when no process is registered under `id` — normally
    /// because it already exited.
    pub async fn cancel(&self, id: &str) -> bool {
        let target = {
            let guard = self.running.lock().await;
            match guard.get(id) {
                Some(r) => {
                    r.cancel.store(true, std::sync::atomic::Ordering::SeqCst);
                    r.pid
                }
                None => return false,
            }
        };

        match target {
            Some(pid) => kill::kill_tree(pid),
            // Spawned but no pid means it already exited.
            None => false,
        }
    }

    /// Ids of every process currently running.
    pub async fn running_ids(&self) -> Vec<String> {
        self.running.lock().await.keys().cloned().collect()
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
                    && events.send(ProcessEvent::Output { stream, text }).await.is_err()
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
        let _ = events.send(ProcessEvent::Output { stream, text: tail }).await;
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

    #[tokio::test]
    async fn streams_stdout_and_reports_success() {
        let inv = invocation("sh", &["-c", "printf 'hello\\nworld\\n'"]);
        let (events, code) = collect(&inv).await;

        assert_eq!(code, Some(0));
        assert_eq!(output_text(&events, Stream::Stdout), "hello\nworld\n");
        assert!(matches!(events.first(), Some(ProcessEvent::Started { .. })));
        assert!(matches!(
            events.last(),
            Some(ProcessEvent::Exited { success: true, cancelled: false, .. })
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
            Some(ProcessEvent::Exited { success: false, code: Some(3), .. })
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
        // Compare canonicalised paths: /tmp is a symlink on macOS.
        assert_eq!(
            PathBuf::from(cwd),
            dir.path().canonicalize().unwrap_or_else(|_| dir.path().to_path_buf())
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

        // Wait for it to actually be up before cancelling.
        loop {
            if sup.is_running("long").await {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }
        assert!(sup.cancel("long").await);

        let result = tokio::time::timeout(std::time::Duration::from_secs(10), runner)
            .await
            .expect("cancel should terminate the process promptly");
        assert!(result.is_ok());

        let mut saw_cancelled_exit = false;
        while let Ok(e) = rx.try_recv() {
            if let ProcessEvent::Exited { cancelled, success, .. } = e {
                saw_cancelled_exit = cancelled && !success;
            }
        }
        assert!(saw_cancelled_exit, "exit should be reported as a cancellation");
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
        loop {
            if sup.is_running("tree").await {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }
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
    async fn cancelling_an_unknown_id_is_harmless() {
        let sup = Supervisor::new();
        assert!(!sup.cancel("never-started").await);
    }
}

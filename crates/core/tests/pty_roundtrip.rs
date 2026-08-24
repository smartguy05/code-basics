//! End-to-end PTY test: prove a real pseudo-terminal round-trips stdin to
//! stdout.
//!
//! This is the one impure spawn path in the `pty` module, the counterpart to
//! how `lsp::transport`/`session` are the only LSP layers that touch a process.
//! Everything decidable — shell selection, size clamping, event shapes — is
//! unit-tested without a process; what only a real PTY can show is that a
//! keystroke written to the master comes back out having been read by the
//! child. Guarded on `sh` being resolvable, like the `process::` tests (run
//! `cargo test` from Git Bash on Windows).
//!
//! ## The DSR responder
//!
//! A terminal emulator must answer a Device Status Report query (`ESC [ 6 n`,
//! "where is the cursor?") with a cursor-position report. xterm.js does this
//! automatically, so the real app never thinks about it — but Git Bash's bash
//! under ConPTY **blocks at startup** until it gets that answer, emitting
//! nothing else. These tests therefore stand in for the terminal and answer the
//! query themselves; on Linux, where `sh -c` emits no such query, the responder
//! simply never fires.

use std::collections::BTreeMap;
use std::path::Path;
use std::time::Duration;

use cb_core::process::resolve_program;
use cb_core::pty::{PtyManager, PtySpec, TerminalEvent};
use tokio::sync::mpsc;

/// The DSR cursor-position query a terminal is expected to answer.
const DSR_QUERY: &str = "\u{1b}[6n";
/// A plausible cursor-position report (row 1, column 1), as xterm would send.
const DSR_REPLY: &str = "\u{1b}[1;1R";

fn sh_available() -> bool {
    resolve_program("sh") != Path::new("sh")
}

fn spec(args: &[&str]) -> PtySpec {
    PtySpec {
        shell: "sh".into(),
        args: args.iter().map(|s| s.to_string()).collect(),
        cwd: std::env::temp_dir(),
        cols: 80,
        rows: 24,
        env: BTreeMap::new(),
    }
}

/// Drain output until it contains `needle`, answering any DSR query the way a
/// real terminal would, bounded by a timeout so a hang fails loudly.
async fn collect_until(
    mgr: &PtyManager,
    id: &str,
    mut rx: mpsc::Receiver<TerminalEvent>,
    needle: &str,
) -> String {
    let mut out = String::new();
    let _ = tokio::time::timeout(Duration::from_secs(20), async {
        while let Some(event) = rx.recv().await {
            if let TerminalEvent::Output { text } = event {
                if text.contains(DSR_QUERY) {
                    let _ = mgr.write(id, DSR_REPLY);
                }
                out.push_str(&text);
                if out.contains(needle) {
                    break;
                }
            }
        }
    })
    .await;
    out
}

#[tokio::test]
async fn a_keystroke_written_to_the_pty_is_read_by_the_child_and_echoed_back() {
    if !sh_available() {
        return; // no sh on this machine — nothing to prove
    }

    let mgr = PtyManager::new();
    let (tx, rx) = mpsc::channel(256);
    // The child reads one line from stdin and echoes it back with a marker, so
    // the marker in the output proves the write reached the child's stdin.
    mgr.open("t", spec(&["-c", "read x; echo got:$x"]), tx)
        .expect("open a pty running sh");

    // Give the shell its startup moment (and the DSR handshake) before typing.
    // Enter is a carriage return over a PTY — the line discipline maps `\r` to
    // `\n`, so a bare `\n` never completes `read`. This is the same byte
    // xterm's `onData` sends when the user presses Enter.
    tokio::time::sleep(Duration::from_millis(300)).await;
    mgr.write("t", "hi\r").expect("write to the pty");

    let out = collect_until(&mgr, "t", rx, "got:hi").await;
    assert!(
        out.contains("got:hi"),
        "the child never echoed the written line; saw: {out:?}"
    );
}

#[tokio::test]
async fn a_terminal_reports_its_exit_and_drops_from_the_listing() {
    if !sh_available() {
        return;
    }

    let mgr = PtyManager::new();
    let (tx, mut rx) = mpsc::channel(256);
    mgr.open("t", spec(&["-c", "exit 3"]), tx)
        .expect("open a short-lived pty");

    let mut exit_code = None;
    let _ = tokio::time::timeout(Duration::from_secs(20), async {
        while let Some(event) = rx.recv().await {
            match event {
                TerminalEvent::Output { text } if text.contains(DSR_QUERY) => {
                    let _ = mgr.write("t", DSR_REPLY);
                }
                TerminalEvent::Exited { code, .. } => {
                    exit_code = code;
                    break;
                }
                _ => {}
            }
        }
    })
    .await;

    assert_eq!(
        exit_code,
        Some(3),
        "the terminal never reported its exit code"
    );
    // The waiter removes the session on exit; give it a moment to win the lock.
    tokio::time::sleep(Duration::from_millis(200)).await;
    assert!(!mgr.is_open("t"), "an exited session stayed in the listing");
}

#[tokio::test]
async fn closing_a_terminal_removes_it_from_the_listing() {
    if !sh_available() {
        return;
    }

    let mgr = PtyManager::new();
    let (tx, _rx) = mpsc::channel(256);
    // A shell that would otherwise sit idle forever, so only the close ends it.
    mgr.open("t", spec(&["-c", "sleep 60"]), tx)
        .expect("open a long-lived pty");

    assert!(mgr.is_open("t"));
    assert!(mgr.close("t").await, "close reported nothing to close");
    assert!(!mgr.is_open("t"));
    // Closing an already-closed session is harmless and reports false.
    assert!(!mgr.close("t").await);
}

#[tokio::test]
async fn a_claude_session_marker_is_stripped_from_the_terminal_environment() {
    if !sh_available() {
        return;
    }

    // Simulate the app having been launched from inside a Claude Code session:
    // the marker is in this process's environment, which the PTY would inherit.
    // (`set_var` is safe on edition 2021; no other test reads this variable.)
    std::env::set_var("CLAUDE_CODE_CHILD_SESSION", "1");

    let mgr = PtyManager::new();
    let (tx, rx) = mpsc::channel(256);
    // Print the variable delimited so an empty value is visible as `MARK[]`.
    mgr.open(
        "t",
        spec(&["-c", "printf 'MARK[%s]' \"$CLAUDE_CODE_CHILD_SESSION\""]),
        tx,
    )
    .expect("open a pty");

    let out = collect_until(&mgr, "t", rx, "MARK[").await;
    std::env::remove_var("CLAUDE_CODE_CHILD_SESSION");

    assert!(
        out.contains("MARK[]"),
        "the child-session marker leaked into the terminal (would disable \
         transcript saving for a nested claude); saw: {out:?}"
    );
}

#[tokio::test]
async fn writing_to_an_unknown_terminal_errors_rather_than_panicking() {
    let mgr = PtyManager::new();
    assert!(mgr.write("never-opened", "x").is_err());
    // Resizing and closing an unknown session are both no-ops, not errors.
    assert!(mgr.resize("never-opened", 100, 40).is_ok());
    assert!(!mgr.close("never-opened").await);
}

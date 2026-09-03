//! The wire types for an interactive terminal session.
//!
//! Deliberately **not** [`crate::process::ProcessEvent`]. A PTY multiplexes
//! what would be stdout and stderr onto one stream, so there is no `stream`
//! field to split on; and the shell prints its own prompt, so there is no
//! `Started` banner to synthesise. What crosses the wire is raw bytes in
//! (`Output`) and one terminal event out (`Exited`/`Failed`).

use std::collections::BTreeMap;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use specta::Type;

/// What to launch, and at what size.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct PtySpec {
    /// The shell (or any program) to run in the PTY.
    pub shell: String,
    /// Arguments passed to `shell`. Empty for a bare interactive shell.
    pub args: Vec<String>,
    /// Working directory the shell starts in.
    pub cwd: PathBuf,
    /// Initial terminal width in columns.
    pub cols: u16,
    /// Initial terminal height in rows.
    pub rows: u16,
    /// Environment layered over the inherited process environment.
    pub env: BTreeMap<String, String>,
}

/// One shell the app actually found on this machine.
///
/// Only ever constructed for a file that was located, so every field describes
/// something that existed at detection time — there is no "disabled" or
/// "missing" row, because a row the user cannot launch is worse than a shorter
/// list (see [`crate::pty::detected_shells`]).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct ShellInfo {
    /// Stable identifier the frontend persists as the user's preference:
    /// `"pwsh"`, `"powershell"`, `"cmd"`, `"bash"`, … Persisting the **id**
    /// rather than the path is what lets `program` be re-detected on every use
    /// without the stored choice ever going stale.
    pub id: String,
    /// What the picker shows. Asserts nothing that was not verified — `pwsh` is
    /// "PowerShell (pwsh)" and not "PowerShell 7", because `pwsh` 6 exists and
    /// no version was read.
    pub label: String,
    /// The **resolved absolute path** — what actually gets spawned.
    ///
    /// Not the bare name: detection proved *that file* exists, and re-resolving
    /// `"pwsh"` at spawn time re-runs the PATHEXT walk, which could launch a
    /// different file than the one the user picked. It is also the only thing
    /// that tells two `bash` installations apart, which is why the UI shows it.
    pub program: String,
    /// Arguments to pass with `program`. Empty for every shell detected today;
    /// the field exists so a shell that needs one (a login flag, say) can be
    /// added without a wire change.
    pub args: Vec<String>,
}

/// What `list_shells` answers: which shells exist, and which of them the app
/// would launch with no preference at all.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct DetectedShells {
    /// The shells found, in preference order. **Empty is a legitimate answer**,
    /// not a failure: terminals still open, because the terminal command falls
    /// back to [`crate::pty::default_shell`] when the frontend names no program.
    pub shells: Vec<ShellInfo>,
    /// The id [`crate::pty::default_shell`] resolves to, or `None` when it
    /// matches nothing detected.
    ///
    /// No `skip_serializing_if`, deliberately: "we could not identify the
    /// default" must not be indistinguishable from a backend that forgot to
    /// send one, so this crosses as an explicit `null`.
    pub default_id: Option<String>,
}

/// Events emitted over the lifetime of one terminal session.
//
// `rename_all` covers the variant names; `rename_all_fields` covers the fields
// inside them, so `Exited { code }` keeps its key as the UI expects. There is
// no `skip_serializing_if` anywhere here on purpose — a `None` code (killed by
// a signal) must stay visible as `null`, not vanish.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(
    tag = "type",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum TerminalEvent {
    /// A chunk of raw terminal output — ANSI escapes, cursor moves and all —
    /// written straight to xterm without post-processing.
    Output { text: String },
    /// The shell exited. `code` is `None` when it was killed by a signal.
    Exited { code: Option<i32>, success: bool },
    /// The session could not be started, or supervision failed after start.
    Failed { message: String },
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn output_serialises_with_the_keys_the_ui_reads() {
        let event = TerminalEvent::Output {
            text: "hello".into(),
        };
        let json = serde_json::to_value(&event).unwrap();
        let mut keys: Vec<String> = json.as_object().unwrap().keys().cloned().collect();
        keys.sort();
        assert_eq!(keys, ["text", "type"]);
        assert_eq!(json["type"], "output");
    }

    #[test]
    fn exited_serialises_with_the_keys_the_ui_reads() {
        let event = TerminalEvent::Exited {
            code: Some(0),
            success: true,
        };
        let json = serde_json::to_value(&event).unwrap();
        let mut keys: Vec<String> = json.as_object().unwrap().keys().cloned().collect();
        keys.sort();
        assert_eq!(keys, ["code", "success", "type"]);
        assert_eq!(json["type"], "exited");
    }

    #[test]
    fn a_signalled_exit_keeps_a_null_code_rather_than_dropping_it() {
        // The distinction between "exited with code 0" and "killed by a signal"
        // is exactly the kind this codebase refuses to collapse. `null` must
        // survive serialisation, not disappear.
        let event = TerminalEvent::Exited {
            code: None,
            success: false,
        };
        let json = serde_json::to_value(&event).unwrap();
        assert!(json.as_object().unwrap().contains_key("code"));
        assert!(json["code"].is_null());
    }

    #[test]
    fn failed_serialises_with_the_keys_the_ui_reads() {
        let event = TerminalEvent::Failed {
            message: "no such shell".into(),
        };
        let json = serde_json::to_value(&event).unwrap();
        let mut keys: Vec<String> = json.as_object().unwrap().keys().cloned().collect();
        keys.sort();
        assert_eq!(keys, ["message", "type"]);
        assert_eq!(json["type"], "failed");
    }

    #[test]
    fn spec_serialises_camel_cased() {
        let spec = PtySpec {
            shell: "sh".into(),
            args: vec!["-c".into(), "echo hi".into()],
            cwd: PathBuf::from("/tmp"),
            cols: 80,
            rows: 24,
            env: BTreeMap::new(),
        };
        let json = serde_json::to_value(&spec).unwrap();
        let mut keys: Vec<String> = json.as_object().unwrap().keys().cloned().collect();
        keys.sort();
        assert_eq!(keys, ["args", "cols", "cwd", "env", "rows", "shell"]);
    }

    #[test]
    fn shell_info_serialises_with_the_keys_the_ui_reads() {
        // Every field here is a single word, so `rename_all = "camelCase"` is a
        // no-op on all four — which is exactly the trap
        // `docs/architecture/ipc-contract.md` warns about: the rename looks
        // like it is doing work it is not, so the keys are pinned regardless.
        let shell = ShellInfo {
            id: "pwsh".into(),
            label: "PowerShell (pwsh)".into(),
            program: r"C:\Program Files\PowerShell\7\pwsh.exe".into(),
            args: Vec::new(),
        };
        let json = serde_json::to_value(&shell).unwrap();
        let mut keys: Vec<String> = json.as_object().unwrap().keys().cloned().collect();
        keys.sort();
        assert_eq!(keys, ["args", "id", "label", "program"]);
    }

    #[test]
    fn detected_shells_serialises_with_the_keys_the_ui_reads() {
        // Asserted with `default_id: None` on purpose: the key must be present
        // as an explicit `null`. "We could not identify the default" and "the
        // backend forgot to send one" are different facts, and an omitted key
        // makes them the same fact in the UI.
        let detected = DetectedShells {
            shells: Vec::new(),
            default_id: None,
        };
        let json = serde_json::to_value(&detected).unwrap();
        let mut keys: Vec<String> = json.as_object().unwrap().keys().cloned().collect();
        keys.sort();
        assert_eq!(keys, ["defaultId", "shells"]);
        assert!(json["defaultId"].is_null());
    }
}

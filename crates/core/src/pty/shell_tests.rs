use super::*;

#[test]
fn clamp_size_floors_zero_dimensions_to_one() {
    assert_eq!(clamp_size(0, 0), (1, 1));
    assert_eq!(clamp_size(80, 0), (80, 1));
    assert_eq!(clamp_size(0, 24), (1, 24));
}

#[test]
fn clamp_size_leaves_ordinary_dimensions_alone() {
    assert_eq!(clamp_size(80, 24), (80, 24));
    assert_eq!(clamp_size(200, 50), (200, 50));
}

#[test]
fn pick_shell_returns_the_first_available_candidate() {
    let chosen = pick_shell(&["pwsh", "powershell", "cmd"], |c| c == "powershell");
    assert_eq!(chosen, "powershell");
}

#[test]
fn pick_shell_prefers_the_earliest_available_when_several_match() {
    let chosen = pick_shell(&["pwsh", "powershell", "cmd"], |_| true);
    assert_eq!(chosen, "pwsh");
}

#[test]
fn pick_shell_falls_back_to_the_last_candidate_when_none_are_available() {
    // The last candidate is the one that effectively always exists, so a
    // fallback to it is the safe default — and the spawn error, if it comes to
    // that, still names something concrete.
    let chosen = pick_shell(&["pwsh", "powershell", "cmd"], |_| false);
    assert_eq!(chosen, "cmd");
}

#[test]
fn pick_shell_with_no_candidates_is_empty_rather_than_panicking() {
    let chosen = pick_shell(&[], |_| true);
    assert_eq!(chosen, "");
}

#[test]
fn session_markers_are_recognised() {
    // The named culprit and the rest of Claude Code's injected set.
    assert!(is_session_marker("CLAUDE_CODE_CHILD_SESSION"));
    assert!(is_session_marker("CLAUDE_CODE_MESSAGING_SOCKET"));
    assert!(is_session_marker("CLAUDE_CODE_MESSAGING_TOKEN"));
    assert!(is_session_marker("CLAUDE_CODE_SESSION_ID"));
    assert!(is_session_marker("CLAUDE_CODE_ENTRYPOINT"));
    assert!(is_session_marker("CLAUDE_CODE_EXECPATH"));
    // A future CLAUDE_CODE_* marker is caught without a code change.
    assert!(is_session_marker("CLAUDE_CODE_SOME_NEW_THING"));
    // The bare markers outside the namespace.
    assert!(is_session_marker("CLAUDECODE"));
    assert!(is_session_marker("CLAUDE_PID"));
    assert!(is_session_marker("CLAUDE_EFFORT"));
    assert!(is_session_marker("AI_AGENT"));
}

#[test]
fn ordinary_and_user_owned_variables_are_left_alone() {
    // A clean terminal must keep the environment it needs to work.
    assert!(!is_session_marker("PATH"));
    assert!(!is_session_marker("HOME"));
    assert!(!is_session_marker("TERM"));
    // A user's own unrelated CLAUDE_* variable is not a session marker — only
    // the reserved CLAUDE_CODE_ namespace and the exact injected names are.
    assert!(!is_session_marker("CLAUDE_API_KEY"));
    assert!(!is_session_marker("CLAUDE_CONFIG"));
    assert!(!is_session_marker("ANTHROPIC_API_KEY"));
}

#[test]
fn default_shell_names_a_non_empty_program() {
    // Cross-platform smoke test: whatever the host, a terminal has something to
    // launch. The specific choice is environment-dependent and covered by the
    // pure `pick_shell` tests above.
    assert!(!default_shell().is_empty());
}

// ---------------------------------------------------------------------------
// Detection: listing the shells that are actually here
// ---------------------------------------------------------------------------

use std::path::{Path, PathBuf};

fn candidate(id: &str, label: &str, probe: &str) -> ShellCandidate {
    ShellCandidate {
        id: id.into(),
        label: label.into(),
        probe: probe.into(),
        args: Vec::new(),
    }
}

/// A `locate` that finds exactly the probes it is given, at the paths given.
fn locates<'a>(
    pairs: &'a [(&'a str, &'a str)],
) -> impl Fn(&ShellCandidate) -> Option<PathBuf> + 'a {
    move |c: &ShellCandidate| {
        pairs
            .iter()
            .find(|(probe, _)| *probe == c.probe)
            .map(|(_, path)| PathBuf::from(path))
    }
}

#[test]
fn detected_shells_omits_a_candidate_that_cannot_be_found() {
    // Never emitted with a broken path, and never emitted "disabled": a row the
    // user can click and cannot launch is worse than a shorter list.
    let candidates = vec![
        candidate("pwsh", "PowerShell (pwsh)", "pwsh"),
        candidate("cmd", "Command Prompt", "cmd"),
    ];
    let found = detected_shells(
        &candidates,
        locates(&[("cmd", "C:/Windows/System32/cmd.exe")]),
    );
    assert_eq!(found.len(), 1);
    assert_eq!(found[0].id, "cmd");
}

#[test]
fn no_candidate_found_is_an_empty_list_not_a_fallback() {
    // The deliberate contrast with `pick_shell`, which falls back to its *last*
    // candidate. Here an empty list is a legitimate answer: the terminal
    // command still calls `default_shell()` when no program is named, so the
    // app degrades to its previous behaviour rather than breaking.
    let candidates = vec![
        candidate("pwsh", "PowerShell (pwsh)", "pwsh"),
        candidate("cmd", "Command Prompt", "cmd"),
    ];
    let found = detected_shells(&candidates, |_| None);
    assert!(found.is_empty(), "{found:?}");
}

#[test]
fn detected_shells_keeps_candidate_preference_order() {
    let candidates = vec![
        candidate("pwsh", "PowerShell (pwsh)", "pwsh"),
        candidate("powershell", "Windows PowerShell", "powershell"),
        candidate("cmd", "Command Prompt", "cmd"),
    ];
    let found = detected_shells(
        &candidates,
        locates(&[
            ("cmd", "C:/Windows/System32/cmd.exe"),
            ("pwsh", "C:/pwsh/pwsh.exe"),
            ("powershell", "C:/Windows/System32/powershell.exe"),
        ]),
    );
    let ids: Vec<&str> = found.iter().map(|s| s.id.as_str()).collect();
    assert_eq!(ids, ["pwsh", "powershell", "cmd"]);
}

#[test]
fn the_program_to_spawn_is_the_resolved_path_not_the_name_probed() {
    // Detection proved *that file* exists; re-resolving the bare name at spawn
    // time could launch a different one.
    let candidates = vec![candidate("pwsh", "PowerShell (pwsh)", "pwsh")];
    let found = detected_shells(
        &candidates,
        locates(&[("pwsh", "C:/Program Files/PowerShell/7/pwsh.exe")]),
    );
    assert_eq!(found[0].program, "C:/Program Files/PowerShell/7/pwsh.exe");
}

#[test]
fn two_candidates_resolving_to_the_same_file_are_listed_once() {
    // `$SHELL=/bin/bash` must not appear twice, and the earlier candidate wins
    // so the nicely-labelled known entry survives.
    let candidates = vec![
        candidate("bash", "Bash", "/bin/bash"),
        candidate("login", "bash", "$SHELL"),
    ];
    let found = detected_shells(
        &candidates,
        locates(&[("/bin/bash", "/bin/bash"), ("$SHELL", "/bin/bash")]),
    );
    assert_eq!(found.len(), 1, "{found:?}");
    assert_eq!(found[0].id, "bash");
    assert_eq!(found[0].label, "Bash");
}

#[cfg(windows)]
#[test]
fn paths_differing_only_in_case_are_the_same_file_on_windows() {
    let candidates = vec![
        candidate("cmd", "Command Prompt", "cmd"),
        candidate("login", "cmd", "$SHELL"),
    ];
    let found = detected_shells(
        &candidates,
        locates(&[
            ("cmd", "C:/Windows/System32/cmd.exe"),
            ("$SHELL", "C:/WINDOWS/SYSTEM32/CMD.EXE"),
        ]),
    );
    assert_eq!(found.len(), 1, "{found:?}");
    assert_eq!(found[0].id, "cmd");
}

#[test]
fn every_detected_shell_has_a_unique_id() {
    // The frontend persists the id, so two rows sharing one would make the
    // stored preference ambiguous. `/bin/zsh` and `/usr/bin/zsh` are both
    // probed under the id `zsh` for exactly this reason: whichever is found
    // first is *the* zsh.
    let candidates = vec![
        candidate("zsh", "Zsh", "/bin/zsh"),
        candidate("zsh", "Zsh", "/usr/bin/zsh"),
    ];
    let found = detected_shells(
        &candidates,
        locates(&[("/bin/zsh", "/bin/zsh"), ("/usr/bin/zsh", "/usr/bin/zsh")]),
    );
    assert_eq!(found.len(), 1, "{found:?}");
    assert_eq!(found[0].program, "/bin/zsh");
}

#[test]
fn a_batch_shim_whose_path_cmd_exe_would_re_read_is_omitted_rather_than_offered() {
    // A Scoop-style `.cmd` shim under a directory containing `&` cannot be
    // spawned: `cmd.exe` re-reads the command line portable-pty built with MSVC
    // quoting alone. `open_inner`'s guard stays the enforcement; this is about
    // not advertising a row that would fail the moment it was clicked.
    let candidates = vec![candidate("pwsh", "PowerShell (pwsh)", "pwsh")];
    let found = detected_shells(
        &candidates,
        locates(&[("pwsh", "C:/dev&test/shims/pwsh.cmd")]),
    );
    assert!(found.is_empty(), "{found:?}");
}

#[test]
fn the_same_hazardous_directory_is_fine_for_a_real_executable() {
    // The asymmetry `pty::argv` exists to keep: MSVC quoting is correct for a
    // real `.exe`, nothing re-parses the line, and refusing it would drop a
    // shell that launches perfectly well.
    let candidates = vec![candidate("pwsh", "PowerShell (pwsh)", "pwsh")];
    let found = detected_shells(&candidates, locates(&[("pwsh", "C:/dev&test/pwsh.exe")]));
    assert_eq!(found.len(), 1, "{found:?}");
}

#[test]
fn a_system32_bash_is_not_offered_because_it_is_the_wsl_launcher() {
    // `System32\bash.exe` is the legacy WSL launcher and fails outright with no
    // distro installed. Omitting is a heuristic — it would also omit a genuine
    // bash copied into System32 — and omitting is the safe side.
    let candidates = vec![candidate("bash", "Bash", "bash")];
    let found = detected_shells(
        &candidates,
        locates(&[("bash", "C:/Windows/System32/bash.exe")]),
    );
    assert!(found.is_empty(), "{found:?}");
}

#[test]
fn a_bash_outside_system32_is_offered() {
    let candidates = vec![candidate("bash", "Bash", "bash")];
    let found = detected_shells(
        &candidates,
        locates(&[("bash", "C:/Program Files/Git/bin/bash.exe")]),
    );
    assert_eq!(found.len(), 1, "{found:?}");
    assert_eq!(found[0].program, "C:/Program Files/Git/bin/bash.exe");
}

#[test]
fn the_wsl_bash_launcher_is_recognised_in_both_system_directories() {
    assert!(is_wsl_bash_launcher(Path::new(
        "C:/Windows/System32/bash.exe"
    )));
    assert!(is_wsl_bash_launcher(Path::new(
        "C:/Windows/SysWOW64/bash.exe"
    )));
    // Case-insensitively, because the filesystem is.
    assert!(is_wsl_bash_launcher(Path::new(
        "C:/WINDOWS/SYSTEM32/BASH.EXE"
    )));
    // A bash anywhere else, and any other program in System32, are not it.
    assert!(!is_wsl_bash_launcher(Path::new(
        "C:/Program Files/Git/bin/bash.exe"
    )));
    assert!(!is_wsl_bash_launcher(Path::new(
        "C:/Windows/System32/cmd.exe"
    )));
    assert!(!is_wsl_bash_launcher(Path::new("/bin/bash")));
}

#[test]
fn the_default_shell_is_identified_among_the_detected_ones() {
    let shells = vec![
        ShellInfo {
            id: "pwsh".into(),
            label: "PowerShell (pwsh)".into(),
            program: "C:/pwsh/pwsh.exe".into(),
            args: Vec::new(),
        },
        ShellInfo {
            id: "cmd".into(),
            label: "Command Prompt".into(),
            program: "C:/Windows/System32/cmd.exe".into(),
            args: Vec::new(),
        },
    ];
    assert_eq!(
        default_among(&shells, Path::new("C:/Windows/System32/cmd.exe")),
        Some("cmd".to_string())
    );
}

#[test]
fn a_default_matching_nothing_detected_is_none_rather_than_the_first() {
    // Abstaining is the point: naming the first entry would tell the user the
    // app launches something it does not.
    let shells = vec![ShellInfo {
        id: "pwsh".into(),
        label: "PowerShell (pwsh)".into(),
        program: "C:/pwsh/pwsh.exe".into(),
        args: Vec::new(),
    }];
    assert_eq!(default_among(&shells, Path::new("/bin/zsh")), None);
    assert_eq!(default_among(&[], Path::new("C:/pwsh/pwsh.exe")), None);
}

#[test]
fn detect_shells_describes_this_machine_without_asserting_which_shells_it_has() {
    // Host-dependent, in the spirit of `default_shell_names_a_non_empty_program`:
    // the invariants are checked, the machine's actual inventory is not.
    let detected = detect_shells();
    let mut ids: Vec<&str> = detected.shells.iter().map(|s| s.id.as_str()).collect();
    let count = ids.len();
    ids.sort_unstable();
    ids.dedup();
    assert_eq!(
        ids.len(),
        count,
        "duplicate shell ids: {:?}",
        detected.shells
    );
    for shell in &detected.shells {
        assert!(
            Path::new(&shell.program).is_file(),
            "detected shell {} points at {}, which is not a file",
            shell.id,
            shell.program
        );
        assert!(!shell.label.is_empty());
    }
    // `default_id`, when present, must name a row that is actually in the list.
    if let Some(id) = &detected.default_id {
        assert!(detected.shells.iter().any(|s| &s.id == id));
    }
}

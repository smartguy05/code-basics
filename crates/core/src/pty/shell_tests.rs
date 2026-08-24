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

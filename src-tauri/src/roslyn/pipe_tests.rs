use super::*;

#[test]
fn the_pipe_name_carries_the_process_id() {
    // A fixed name would make a second application fail to bind while the first
    // answered for a workspace it does not have open.
    assert_eq!(pipe_name(4242), r"\\.\pipe\code-basics.roslyn.4242");
    assert_ne!(pipe_name(1), pipe_name(2));
}

#[test]
fn the_roslyn_and_browser_pipes_never_share_a_name() {
    // Two process-scoped pipes out of one binary: the namespaces must not
    // collide, or an agent's Roslyn call could land on the browser listener.
    assert_ne!(pipe_name(7), crate::browser::pipe::pipe_name(7));
    assert!(pipe_name(7).contains(".roslyn."));
}

#[test]
fn the_pipe_name_is_the_one_the_registry_publishes() {
    let name = pipe_name(std::process::id());
    assert!(name.starts_with(r"\\.\pipe\code-basics.roslyn."));
    assert!(name.ends_with(&std::process::id().to_string()));
}

#[test]
fn a_client_the_os_could_name_is_shown_by_its_file_name() {
    let peer = peer_label(
        Some(12345),
        Some(r"C:\Users\someone\AppData\Roaming\npm\codex.cmd".to_string()),
    );
    assert_eq!(peer.pid, 12345);
    assert_eq!(peer.program, "codex.cmd");
}

#[test]
fn an_unidentifiable_client_is_reported_as_unidentified_rather_than_named() {
    let peer = peer_label(None, None);
    assert_eq!(peer.pid, 0);
    assert_eq!(peer.program, "an unidentified program");
    assert!(!peer.program.contains("code-basics"));
}

#[test]
fn a_named_pid_with_no_readable_image_still_reports_the_pid() {
    let peer = peer_label(Some(999), None);
    assert_eq!(peer.pid, 999);
    assert_eq!(peer.program, "an unidentified program");
}

#[test]
fn a_path_with_no_file_name_falls_back_to_the_whole_string() {
    let peer = peer_label(Some(1), Some(r"C:\".to_string()));
    assert!(!peer.program.is_empty());
}

#[test]
fn this_account_has_a_sid_and_the_descriptor_can_be_built() {
    // The DACL is not optional: a NULL security descriptor on a named pipe
    // grants read access to Everyone. So if this cannot be built, no pipe is
    // created at all.
    let sid = current_user_sid().expect("this account must have a SID");
    assert!(sid.starts_with("S-1-"), "{sid}");
    let descriptor = user_only_descriptor().expect("the descriptor must build");
    assert!(!descriptor.0.is_null());
}

#[test]
fn the_descriptor_names_this_account_and_nobody_else() {
    let sid = current_user_sid().unwrap();
    let sddl = format!("D:P(A;;GA;;;{sid})");
    assert!(sddl.contains(&sid));
    assert!(sddl.starts_with("D:P("), "{sddl}");
    for well_known in ["WD", "AN", "BU", "S-1-1-0"] {
        assert!(
            !sddl.contains(well_known),
            "{sddl} must not grant Everyone, anonymous or Users"
        );
    }
}

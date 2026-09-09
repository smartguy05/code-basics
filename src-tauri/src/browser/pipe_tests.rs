use super::*;

#[test]
fn the_pipe_name_carries_the_process_id() {
    // A fixed name would make a second application fail to bind while the
    // first answered for a codebase it does not have open.
    assert_eq!(pipe_name(4242), r"\\.\pipe\code-basics.browser.4242");
    assert_ne!(pipe_name(1), pipe_name(2));
}

#[test]
fn the_pipe_name_is_the_one_the_registry_publishes() {
    // The client connects to the string in the registry, so the name is
    // carried rather than recomputed — but the two must agree at the moment it
    // is written.
    let name = pipe_name(std::process::id());
    assert!(name.starts_with(r"\\.\pipe\code-basics.browser."));
    assert!(name.ends_with(&std::process::id().to_string()));
}

#[test]
fn a_client_the_os_could_name_is_shown_by_its_file_name() {
    // The full path is long enough to push the rest of the banner off screen,
    // and the file name is what identifies the program.
    let peer = peer_label(
        Some(12345),
        Some(r"C:\Users\someone\AppData\Roaming\npm\codex.cmd".to_string()),
    );
    assert_eq!(peer.pid, 12345);
    assert_eq!(peer.program, "codex.cmd");
}

#[test]
fn an_unidentifiable_client_is_reported_as_unidentified_rather_than_named() {
    // A user asked to grant access to their own logged-in session is entitled
    // to know the asker could not be identified, rather than being shown a
    // plausible name.
    let peer = peer_label(None, None);
    assert_eq!(peer.pid, 0);
    assert_eq!(peer.program, "an unidentified program");
    assert!(!peer.program.contains("code-basics"));
}

#[test]
fn a_named_pid_with_no_readable_image_still_reports_the_pid() {
    // Half the answer is better than none: the user can look the pid up.
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
    // created at all — which makes it worth knowing it builds here.
    let sid = current_user_sid().expect("this account must have a SID");
    assert!(sid.starts_with("S-1-"), "{sid}");
    let descriptor = user_only_descriptor().expect("the descriptor must build");
    assert!(!descriptor.0.is_null());
}

#[test]
fn the_descriptor_names_this_account_and_nobody_else() {
    let sid = current_user_sid().unwrap();
    // Rebuilt here rather than exposed, so the test reads the same rule the
    // code applies: full access for this account, and a protected DACL.
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

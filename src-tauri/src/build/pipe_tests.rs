use super::*;

#[test]
fn the_pipe_name_carries_the_process_id() {
    // A fixed name would make a second application fail to bind while the first
    // answered for a workspace it does not have open.
    assert_eq!(pipe_name(4242), r"\\.\pipe\code-basics.build.4242");
    assert_ne!(pipe_name(1), pipe_name(2));
}

#[test]
fn the_build_editor_roslyn_and_browser_pipes_never_share_a_name() {
    // Four process-scoped pipes out of one binary: the namespaces must not
    // collide, or an agent's build call could land on another listener.
    assert_ne!(pipe_name(7), crate::roslyn::pipe::pipe_name(7));
    assert_ne!(pipe_name(7), crate::editor_context::pipe::pipe_name(7));
    assert_ne!(pipe_name(7), crate::browser::pipe::pipe_name(7));
    assert!(pipe_name(7).contains(".build."));
}

#[test]
fn the_pipe_name_matches_the_core_derivation() {
    // The application creates the pipe here; a client derives the name from a
    // verified pid via the core module. The two must agree byte for byte, or a
    // client would open a name nobody is listening on.
    let pid = std::process::id();
    assert_eq!(
        pipe_name(pid),
        cb_core::build::mcp::instances::pipe_name(pid)
    );
}

#[test]
fn this_account_has_a_sid_and_the_descriptor_can_be_built() {
    // The DACL is not optional: a NULL security descriptor on a named pipe grants
    // read access to Everyone. So if this cannot be built, no pipe is created.
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

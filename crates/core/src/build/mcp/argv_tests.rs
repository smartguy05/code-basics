use super::*;

fn args(items: &[&str]) -> Vec<String> {
    items.iter().map(|s| (*s).to_string()).collect()
}

#[test]
fn the_subcommand_alone_starts_the_server() {
    assert!(is_mcp_build_invocation(&args(&[
        r"C:\apps\cb-app.exe",
        "mcp-build"
    ])));
}

#[test]
fn the_marker_alone_starts_the_server() {
    // A hand-edited config line that kept only the marker must still work; the
    // entry lives in a file the user shares with their team.
    assert!(is_mcp_build_invocation(&args(&[
        r"C:\apps\cb-app.exe",
        "--code-basics-mcp-build"
    ])));
}

#[test]
fn the_marker_is_read_back_through_the_constant_that_writes_it() {
    let spelled = format!("--{MARKER}");
    assert!(is_mcp_build_invocation(&args(&[
        r"C:\apps\cb-app.exe",
        &spelled
    ])));
}

#[test]
fn an_ordinary_launch_is_not_an_mcp_invocation() {
    assert!(!is_mcp_build_invocation(&args(&[
        r"C:\apps\cb-app.exe",
        "."
    ])));
    assert!(parse_mcp_build_args(&args(&[r"C:\apps\cb-app.exe", "."])).is_none());
}

#[test]
fn the_other_self_dispatch_modes_are_not_this_server() {
    // The modes must not answer for each other: another mode reaching this parser
    // would start a Build server on that client's stdio and hang it.
    for other in [
        "mcp-sql",
        "--code-basics-mcp-sql",
        "mcp-browser",
        "--code-basics-mcp-browser",
        "mcp-tasks",
        "--code-basics-mcp-tasks",
        "mcp-roslyn",
        "--code-basics-mcp-roslyn",
        "mcp-editor",
        "--code-basics-mcp-editor",
        "record-intent",
        "quality-gate",
    ] {
        assert!(
            !is_mcp_build_invocation(&args(&[r"C:\apps\cb-app.exe", other])),
            "{other} must not start the build server"
        );
    }
}

#[test]
fn the_workspace_flag_is_the_boundary_and_is_read() {
    let parsed = parse_mcp_build_args(&args(&[
        "cb-app.exe",
        "mcp-build",
        "--workspace",
        r"C:\code\repo",
    ]))
    .unwrap();
    assert_eq!(parsed.workspace.as_deref(), Some(r"C:\code\repo"));
    assert_eq!(parsed.instance, None);
}

#[test]
fn no_workspace_flag_is_an_unscoped_install() {
    let parsed = parse_mcp_build_args(&args(&["cb-app.exe", "mcp-build"])).unwrap();
    assert_eq!(parsed.workspace, None);
}

#[test]
fn an_empty_workspace_value_is_no_workspace() {
    for empty in ["", "   "] {
        let parsed =
            parse_mcp_build_args(&args(&["cb-app.exe", "mcp-build", "--workspace", empty]))
                .unwrap();
        assert_eq!(parsed.workspace, None, "{empty:?}");
    }
}

#[test]
fn the_instance_flag_is_read() {
    let parsed = parse_mcp_build_args(&args(&[
        "cb-app.exe",
        "mcp-build",
        "--workspace",
        r"C:\code\repo",
        "--instance",
        "4242",
    ]))
    .unwrap();
    assert_eq!(parsed.instance.as_deref(), Some("4242"));
    assert_eq!(parsed.workspace.as_deref(), Some(r"C:\code\repo"));
}

#[test]
fn an_empty_instance_value_is_no_instance() {
    for empty in ["", "   "] {
        let parsed =
            parse_mcp_build_args(&args(&["cb-app.exe", "mcp-build", "--instance", empty])).unwrap();
        assert_eq!(parsed.instance, None, "{empty:?}");
    }
}

#[test]
fn a_trailing_flag_with_no_value_is_none() {
    let parsed = parse_mcp_build_args(&args(&["cb-app.exe", "mcp-build", "--workspace"])).unwrap();
    assert_eq!(parsed.workspace, None);
    let parsed = parse_mcp_build_args(&args(&["cb-app.exe", "mcp-build", "--instance"])).unwrap();
    assert_eq!(parsed.instance, None);
}

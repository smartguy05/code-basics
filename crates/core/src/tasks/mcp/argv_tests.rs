use super::*;

fn args(items: &[&str]) -> Vec<String> {
    items.iter().map(|s| s.to_string()).collect()
}

#[test]
fn the_subcommand_alone_is_an_invocation() {
    assert!(is_mcp_tasks_invocation(&args(&["cb-app.exe", SUBCOMMAND])));
}

#[test]
fn the_marker_alone_is_an_invocation() {
    // A team-shared config line that a hand edit trimmed to the flag must
    // still start the server.
    assert!(is_mcp_tasks_invocation(&args(&[
        "cb-app.exe",
        &format!("--{MARKER}")
    ])));
}

#[test]
fn a_directory_argument_is_not_an_invocation() {
    // `code-basics .` must still open a window.
    assert!(!is_mcp_tasks_invocation(&args(&["cb-app.exe", "."])));
    assert!(!is_mcp_tasks_invocation(&args(&["cb-app.exe"])));
    assert!(!is_mcp_tasks_invocation(&args(&[
        "cb-app.exe",
        "C:/code/mcp-tasks-notes"
    ])));
}

#[test]
fn a_similar_word_is_not_the_subcommand() {
    assert!(!is_mcp_tasks_invocation(&args(&["cb-app.exe", "mcp"])));
    assert!(!is_mcp_tasks_invocation(&args(&[
        "cb-app.exe",
        "mcp-tasks-x"
    ])));
    assert!(!is_mcp_tasks_invocation(&args(&["cb-app.exe", MARKER])));
}

#[test]
fn the_subcommand_is_not_the_sql_subcommand() {
    // The two MCP servers must never answer for each other.
    assert!(!is_mcp_tasks_invocation(&args(&["cb-app.exe", "mcp-sql"])));
    assert_ne!(SUBCOMMAND, crate::mcp::argv::SUBCOMMAND);
    assert_ne!(MARKER, crate::mcp::argv::MARKER);
}

#[test]
fn parse_reads_the_workspace_flag() {
    let parsed = parse_mcp_tasks_args(&args(&[
        "cb-app.exe",
        SUBCOMMAND,
        "--workspace",
        "C:/code/thing",
    ]))
    .unwrap();
    assert_eq!(parsed.workspace.as_deref(), Some("C:/code/thing"));
}

#[test]
fn an_empty_workspace_is_treated_as_unscoped() {
    let parsed =
        parse_mcp_tasks_args(&args(&["cb-app.exe", SUBCOMMAND, "--workspace", ""])).unwrap();
    assert_eq!(parsed.workspace, None);
}

#[test]
fn parse_without_the_flag_is_unscoped() {
    let parsed = parse_mcp_tasks_args(&args(&["cb-app.exe", SUBCOMMAND])).unwrap();
    assert_eq!(parsed.workspace, None);
}

#[test]
fn parse_of_a_non_invocation_is_none() {
    assert!(parse_mcp_tasks_args(&args(&["cb-app.exe", "."])).is_none());
}

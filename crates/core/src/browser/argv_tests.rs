use super::*;

fn args(items: &[&str]) -> Vec<String> {
    items.iter().map(|s| (*s).to_string()).collect()
}

#[test]
fn the_subcommand_alone_starts_the_server() {
    assert!(is_mcp_browser_invocation(&args(&[
        r"C:\apps\cb-app.exe",
        "mcp-browser"
    ])));
}

#[test]
fn the_marker_alone_starts_the_server() {
    // A hand-edited config line that kept only the marker must still work; the
    // entry lives in a file the user shares with their team.
    assert!(is_mcp_browser_invocation(&args(&[
        r"C:\apps\cb-app.exe",
        "--code-basics-mcp-browser"
    ])));
}

#[test]
fn the_marker_is_read_back_through_the_constant_that_writes_it() {
    let spelled = format!("--{MARKER}");
    assert!(is_mcp_browser_invocation(&args(&[
        r"C:\apps\cb-app.exe",
        &spelled
    ])));
}

#[test]
fn an_ordinary_launch_is_not_an_mcp_invocation() {
    assert!(!is_mcp_browser_invocation(&args(&[
        r"C:\apps\cb-app.exe",
        "."
    ])));
    assert!(parse_mcp_browser_args(&args(&[r"C:\apps\cb-app.exe", "."])).is_none());
}

#[test]
fn the_sql_server_is_not_this_server() {
    // The four self-dispatch modes must not answer for each other: `mcp-sql`
    // reaching this parser would start a browser server on a SQL client's
    // stdio and hang it.
    assert!(!is_mcp_browser_invocation(&args(&[
        r"C:\apps\cb-app.exe",
        "mcp-sql"
    ])));
    assert!(!is_mcp_browser_invocation(&args(&[
        r"C:\apps\cb-app.exe",
        "--code-basics-mcp-sql"
    ])));
    assert!(!is_mcp_browser_invocation(&args(&[
        r"C:\apps\cb-app.exe",
        "record-intent"
    ])));
    assert!(!is_mcp_browser_invocation(&args(&[
        r"C:\apps\cb-app.exe",
        "quality-gate"
    ])));
}

#[test]
fn the_instance_flag_is_read() {
    let parsed =
        parse_mcp_browser_args(&args(&["cb-app.exe", "mcp-browser", "--instance", "4242"]))
            .unwrap();
    assert_eq!(parsed.instance.as_deref(), Some("4242"));
}

#[test]
fn no_instance_flag_means_use_the_single_running_application() {
    let parsed = parse_mcp_browser_args(&args(&["cb-app.exe", "mcp-browser"])).unwrap();
    assert_eq!(parsed.instance, None);
}

#[test]
fn an_empty_instance_value_is_no_instance_rather_than_a_pid_of_nothing() {
    for empty in ["", "   "] {
        let parsed =
            parse_mcp_browser_args(&args(&["cb-app.exe", "mcp-browser", "--instance", empty]))
                .unwrap();
        assert_eq!(parsed.instance, None, "{empty:?}");
    }
}

#[test]
fn a_trailing_instance_flag_with_no_value_is_no_instance() {
    // The value is the next argument; there isn't one. Refusing to start over
    // a malformed flag would take the whole server down for a typo, and the
    // ambiguity refusal already handles "which window" correctly.
    let parsed =
        parse_mcp_browser_args(&args(&["cb-app.exe", "mcp-browser", "--instance"])).unwrap();
    assert_eq!(parsed.instance, None);
}

use super::*;

fn args(items: &[&str]) -> Vec<String> {
    items.iter().map(|s| s.to_string()).collect()
}

#[test]
fn the_subcommand_alone_is_an_invocation() {
    assert!(is_mcp_sql_invocation(&args(&["cb-app.exe", SUBCOMMAND])));
}

#[test]
fn the_marker_alone_is_an_invocation() {
    // A team-shared config line that a hand edit trimmed to the flag must
    // still start the server.
    assert!(is_mcp_sql_invocation(&args(&[
        "cb-app.exe",
        &format!("--{MARKER}")
    ])));
}

#[test]
fn a_directory_argument_is_not_an_invocation() {
    // `code-basics .` must still open a window.
    assert!(!is_mcp_sql_invocation(&args(&["cb-app.exe", "."])));
    assert!(!is_mcp_sql_invocation(&args(&["cb-app.exe"])));
    assert!(!is_mcp_sql_invocation(&args(&[
        "cb-app.exe",
        "C:/code/mcp-sql-notes"
    ])));
}

#[test]
fn a_similar_word_is_not_the_subcommand() {
    assert!(!is_mcp_sql_invocation(&args(&["cb-app.exe", "mcp"])));
    assert!(!is_mcp_sql_invocation(&args(&["cb-app.exe", "mcp-sql-x"])));
    assert!(!is_mcp_sql_invocation(&args(&["cb-app.exe", MARKER])));
}

#[test]
fn the_workspace_flag_is_read_and_an_empty_one_is_not_a_workspace() {
    let parsed = parse_mcp_sql_args(&args(&[
        "cb-app.exe",
        SUBCOMMAND,
        "--workspace",
        "C:/code/app",
    ]))
    .expect("an invocation");
    assert_eq!(parsed.workspace.as_deref(), Some("C:/code/app"));

    let empty = parse_mcp_sql_args(&args(&["cb-app.exe", SUBCOMMAND, "--workspace", ""]))
        .expect("an invocation");
    assert_eq!(empty.workspace, None);

    let absent = parse_mcp_sql_args(&args(&["cb-app.exe", SUBCOMMAND])).expect("an invocation");
    assert_eq!(absent.workspace, None);
}

#[test]
fn an_ordinary_launch_parses_to_nothing() {
    assert_eq!(parse_mcp_sql_args(&args(&["cb-app.exe", "."])), None);
}

#[test]
fn the_self_dispatch_markers_are_disjoint() {
    // The drift alarm for a fourth mode. Each of the three self-dispatch modes
    // must be recognised by its own predicate and by neither of the others —
    // this executable decides which mode it is in before it decides anything
    // else, and a collision would start the wrong one silently.
    use crate::intents::hook::is_record_invocation;
    use crate::intents::providers::hooks_json::MARKER as INTENT_MARKER;
    use crate::qgate::install::MARKER as QGATE_MARKER;
    use crate::qgate::{is_quality_gate_invocation, SUBCOMMAND as QGATE_SUBCOMMAND};

    let names = [MARKER, INTENT_MARKER, QGATE_MARKER];
    let subcommands = [SUBCOMMAND, "record-intent", QGATE_SUBCOMMAND];
    for (i, a) in names.iter().enumerate() {
        for (j, b) in names.iter().enumerate() {
            assert!(i == j || a != b, "markers {a} and {b} collide");
        }
    }
    for (i, a) in subcommands.iter().enumerate() {
        for (j, b) in subcommands.iter().enumerate() {
            assert!(i == j || a != b, "subcommands {a} and {b} collide");
        }
    }

    for (mode, line) in [
        ("mcp", args(&["cb-app.exe", SUBCOMMAND])),
        ("mcp", args(&["cb-app.exe", &format!("--{MARKER}")])),
        ("intent", args(&["cb-app.exe", "record-intent"])),
        (
            "intent",
            args(&["cb-app.exe", &format!("--{INTENT_MARKER}")]),
        ),
        ("qgate", args(&["cb-app.exe", QGATE_SUBCOMMAND])),
        ("qgate", args(&["cb-app.exe", &format!("--{QGATE_MARKER}")])),
    ] {
        let mcp = is_mcp_sql_invocation(&line);
        let intent = is_record_invocation(&line);
        let qgate = is_quality_gate_invocation(&line);
        let claimed = [mcp, intent, qgate].iter().filter(|c| **c).count();
        assert_eq!(claimed, 1, "{line:?} was claimed by {claimed} modes");
        assert!(
            match mode {
                "mcp" => mcp,
                "intent" => intent,
                _ => qgate,
            },
            "{line:?} was not claimed by {mode}"
        );
    }
}

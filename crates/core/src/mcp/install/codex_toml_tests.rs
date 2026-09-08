use super::*;

const NAME: &str = "code-basics-sql";

fn args() -> Vec<String> {
    vec![
        "mcp-sql".to_string(),
        "--workspace".to_string(),
        r"C:\work\repo".to_string(),
    ]
}

/// The real shape of `~/.codex/config.toml` on this machine: comments, bare
/// key-values ahead of every table, and single-quoted literal keys holding
/// Windows paths and colons. A `toml::Value` round-trip loses the comments and
/// respells the keys; this is the test that would fail under one.
const REAL_SHAPE: &str = r#"# Codex configuration
model = "gpt-5"
approval_policy = "on-request"

[projects.'c:\users\me\documents\code\stonks']
trust_level = "trusted"

[projects.'C:\Users\me\Documents\Code\code-basics']
trust_level = "trusted"

[hooks.state.'C:\Users\me\.codex\hooks.json:stop:1:0']
last_run = 17

[mcp_servers.some-other-server]
command = "npx"
args = ["-y", "@example/server"]
"#;

#[test]
fn project_trust_tables_with_quoted_windows_path_keys_survive_verbatim() {
    let out = merged_into_text(REAL_SHAPE, NAME, r"C:\apps\cb-app.exe", &args()).expect("merges");

    assert!(
        out.contains(r"[projects.'c:\users\me\documents\code\stonks']"),
        "the literal lower-case key was respelled:\n{out}"
    );
    assert!(
        out.contains(r"[projects.'C:\Users\me\Documents\Code\code-basics']"),
        "the literal mixed-case key was respelled:\n{out}"
    );
    assert!(
        out.contains(r"[hooks.state.'C:\Users\me\.codex\hooks.json:stop:1:0']"),
        "a literal key holding colons was respelled:\n{out}"
    );
    assert!(
        out.contains("# Codex configuration"),
        "comments lost:\n{out}"
    );
    assert!(
        out.contains("[mcp_servers.some-other-server]"),
        "another server was dropped:\n{out}"
    );
    // Everything the file already said is still there, line for line.
    for line in REAL_SHAPE.lines().filter(|l| !l.trim().is_empty()) {
        assert!(out.contains(line), "lost line {line:?}:\n{out}");
    }
}

#[test]
fn the_entry_is_a_command_and_an_args_array() {
    let out = merged_into_text("", NAME, r"C:\apps\cb-app.exe", &args()).expect("merges");
    let parsed: toml::Value = toml::from_str(&out).expect("valid TOML");
    let entry = &parsed["mcp_servers"][NAME];

    assert_eq!(entry["command"].as_str(), Some(r"C:\apps\cb-app.exe"));
    assert_eq!(
        entry["args"]
            .as_array()
            .expect("array")
            .iter()
            .map(|v| v.as_str().expect("string").to_string())
            .collect::<Vec<_>>(),
        args()
    );
}

#[test]
fn reinstalling_refreshes_in_place_rather_than_duplicating() {
    let once = merged_into_text(REAL_SHAPE, NAME, r"C:\apps\cb-app.exe", &args()).expect("merges");
    let moved = merged_into_text(&once, NAME, r"D:\moved\cb-app.exe", &args()).expect("merges");

    assert_eq!(
        moved.matches(&format!("[mcp_servers.{NAME}]")).count(),
        1,
        "duplicated:\n{moved}"
    );
    let parsed: toml::Value = toml::from_str(&moved).expect("valid TOML");
    assert_eq!(
        parsed["mcp_servers"][NAME]["command"].as_str(),
        Some(r"D:\moved\cb-app.exe")
    );
    assert!(parsed["mcp_servers"]["some-other-server"].is_table());
}

#[test]
fn a_file_that_does_not_parse_is_never_written() {
    assert!(merged_into_text("[[[ not toml", NAME, "x", &args()).is_err());
}

#[test]
fn mcp_servers_holding_something_other_than_a_table_bails() {
    assert!(merged_into_text("mcp_servers = 7\n", NAME, "x", &args()).is_err());
    assert!(merged_into_text("mcp_servers = \"a\"\n", NAME, "x", &args()).is_err());
}

#[test]
fn removal_takes_only_our_entry_and_reports_nothing_when_absent() {
    let with_ours =
        merged_into_text(REAL_SHAPE, NAME, r"C:\apps\cb-app.exe", &args()).expect("merges");

    let removed = removal_text(&with_ours, NAME)
        .expect("removes")
        .expect("something to remove");
    assert!(
        !removed.contains(&format!("[mcp_servers.{NAME}]")),
        "{removed}"
    );
    assert!(
        removed.contains("[mcp_servers.some-other-server]"),
        "{removed}"
    );
    assert!(removed.contains("# Codex configuration"), "{removed}");

    assert!(removal_text(&removed, NAME).expect("ok").is_none());
}

#[test]
fn is_installed_in_reads_back_what_the_merge_wrote() {
    assert!(!is_installed_in(REAL_SHAPE, NAME));
    let out = merged_into_text(REAL_SHAPE, NAME, "x", &args()).expect("merges");
    assert!(is_installed_in(&out, NAME));
    assert!(!is_installed_in(&out, "a-server-that-is-absent"));
    assert!(!is_installed_in("[[[ broken", NAME));
}

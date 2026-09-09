use super::*;

use tempfile::TempDir;

/// A provider home under a tempdir. **Every** test passes one: with `None`,
/// `status_for` reads the developer's real `~/.claude.json`, which is what
/// makes two of `mcp::install::plan`'s own tests fail on a machine where the
/// feature has been used. Not repeating that mistake here.
fn provider_home(dir: &TempDir) -> std::path::PathBuf {
    dir.path().join(".claude")
}

// ---------------------------------------------------------------------------
// The entry
// ---------------------------------------------------------------------------

#[test]
fn the_entry_is_a_command_field_and_an_args_array() {
    let entry = browser_mcp_entry(r"C:\Program Files\code basics\cb-app.exe");
    assert_eq!(
        entry["command"],
        json!(r"C:\Program Files\code basics\cb-app.exe")
    );
    assert_eq!(entry["args"], json!(["mcp-browser"]));
}

#[test]
fn the_exe_path_is_never_quoted_into_a_command_string() {
    // A path with a space, a quote and an ampersand in it: nothing downstream
    // re-splits these, and that is only true while they stay separate fields.
    let hostile = r#"C:\a b\c"d&e\cb-app.exe"#;
    let entry = browser_mcp_entry(hostile);
    assert_eq!(entry["command"].as_str().unwrap(), hostile);
    assert!(entry["args"]
        .as_array()
        .unwrap()
        .iter()
        .all(|arg| !arg.as_str().unwrap().contains("cb-app.exe")));
}

#[test]
fn the_subcommand_is_read_back_through_the_constant_the_parser_uses() {
    let entry = browser_mcp_entry("cb-app.exe");
    let args: Vec<String> = entry["args"]
        .as_array()
        .unwrap()
        .iter()
        .map(|a| a.as_str().unwrap().to_string())
        .collect();
    assert_eq!(args, vec![argv::SUBCOMMAND.to_string()]);
    // And the line it writes really does start the server.
    let mut command_line = vec!["cb-app.exe".to_string()];
    command_line.extend(args);
    assert!(argv::is_mcp_browser_invocation(&command_line));
}

#[test]
fn no_scope_writes_a_workspace_argument_because_there_is_nothing_to_scope() {
    // A browser panel belongs to a window, not a repository. A --workspace flag
    // here would read like a boundary and would not be one.
    let dir = TempDir::new().unwrap();
    for (provider, scope) in [
        (ProviderId::ClaudeCode, InstallScope::Project),
        (ProviderId::ClaudeCode, InstallScope::User),
        (ProviderId::Codex, InstallScope::User),
    ] {
        let plan =
            install_plan_for(provider, dir.path(), scope, Some(&provider_home(&dir))).unwrap();
        let written = &plan.writes[0].content;
        assert!(
            !written.contains("--workspace"),
            "{provider:?}/{scope:?}: {written}"
        );
    }
}

// ---------------------------------------------------------------------------
// Where it writes — reusing `mcp::install`, not re-deciding
// ---------------------------------------------------------------------------

#[test]
fn a_project_install_writes_dot_mcp_json_at_the_root() {
    let dir = TempDir::new().unwrap();
    let plan = install_plan_for(
        ProviderId::ClaudeCode,
        dir.path(),
        InstallScope::Project,
        Some(&provider_home(&dir)),
    )
    .unwrap();
    assert_eq!(plan.writes.len(), 1);
    assert_eq!(plan.writes[0].path, dir.path().join(".mcp.json"));
    assert!(!plan.writes[0].merges_existing);
    let parsed: Value = serde_json::from_str(&plan.writes[0].content).unwrap();
    assert_eq!(
        parsed["mcpServers"]["code-basics-browser"]["args"][0],
        "mcp-browser"
    );
}

#[test]
fn a_codex_install_writes_config_toml_and_a_project_scope_is_refused() {
    let dir = TempDir::new().unwrap();
    let plan = install_plan_for(
        ProviderId::Codex,
        dir.path(),
        InstallScope::User,
        Some(dir.path()),
    )
    .unwrap();
    assert_eq!(plan.writes[0].path, dir.path().join("config.toml"));
    assert!(plan.writes[0]
        .content
        .contains("[mcp_servers.code-basics-browser]"));

    // Codex has no project scope, and inventing <root>/.codex/config.toml
    // would look installed and never be read.
    let refused = install_plan_for(
        ProviderId::Codex,
        dir.path(),
        InstallScope::Project,
        Some(dir.path()),
    )
    .unwrap_err();
    assert!(format!("{refused:#}").contains("no per-repository equivalent"));
}

#[test]
fn installing_beside_the_sql_server_keeps_both() {
    // The two servers come out of one executable and a user may well install
    // both. The reused merge is what makes this work; this test is the alarm
    // for anybody who replaces it with a whole-file write.
    let dir = TempDir::new().unwrap();
    let path = dir.path().join(".mcp.json");
    let sql = crate::mcp::install::plan::install_plan_for(
        ProviderId::ClaudeCode,
        dir.path(),
        InstallScope::Project,
        Some(&provider_home(&dir)),
    )
    .unwrap();
    std::fs::write(&path, &sql.writes[0].content).unwrap();

    let browser = install_plan_for(
        ProviderId::ClaudeCode,
        dir.path(),
        InstallScope::Project,
        Some(&provider_home(&dir)),
    )
    .unwrap();
    assert!(
        browser.writes[0].merges_existing,
        "an existing config must be merged into, and backed up"
    );
    let parsed: Value = serde_json::from_str(&browser.writes[0].content).unwrap();
    let servers = parsed["mcpServers"].as_object().unwrap();
    assert!(servers.contains_key("code-basics-sql"), "{servers:?}");
    assert!(servers.contains_key("code-basics-browser"), "{servers:?}");
}

#[test]
fn a_status_read_finds_what_an_install_wrote_and_nothing_else() {
    let dir = TempDir::new().unwrap();
    let home = provider_home(&dir);
    assert_eq!(
        status_for(ProviderId::ClaudeCode, dir.path(), Some(&home)),
        None
    );

    let plan = install_plan_for(
        ProviderId::ClaudeCode,
        dir.path(),
        InstallScope::Project,
        Some(&home),
    )
    .unwrap();
    std::fs::write(&plan.writes[0].path, &plan.writes[0].content).unwrap();
    assert_eq!(
        status_for(ProviderId::ClaudeCode, dir.path(), Some(&home)),
        Some(InstallScope::Project)
    );
    // Codex is a different file; installing for Claude Code must not report it.
    assert_eq!(
        status_for(ProviderId::Codex, dir.path(), Some(dir.path())),
        None
    );
}

#[test]
fn the_sql_server_being_installed_is_not_this_server_being_installed() {
    let dir = TempDir::new().unwrap();
    let home = provider_home(&dir);
    let sql = crate::mcp::install::plan::install_plan_for(
        ProviderId::ClaudeCode,
        dir.path(),
        InstallScope::Project,
        Some(&home),
    )
    .unwrap();
    std::fs::write(&sql.writes[0].path, &sql.writes[0].content).unwrap();
    assert_eq!(
        status_for(ProviderId::ClaudeCode, dir.path(), Some(&home)),
        None,
        "the two servers have different names and must be reported separately"
    );
}

// ---------------------------------------------------------------------------
// Uninstall
// ---------------------------------------------------------------------------

#[test]
fn uninstalling_removes_only_our_entry() {
    let dir = TempDir::new().unwrap();
    let home = provider_home(&dir);
    let path = dir.path().join(".mcp.json");
    std::fs::write(
        &path,
        serde_json::to_string_pretty(&json!({
            "mcpServers": {
                "code-basics-browser": { "command": "x", "args": ["mcp-browser"] },
                "somebody-elses": { "command": "y", "args": [] }
            }
        }))
        .unwrap(),
    )
    .unwrap();

    let plan = uninstall_plan_for(
        ProviderId::ClaudeCode,
        dir.path(),
        InstallScope::Project,
        Some(&home),
    )
    .unwrap();
    assert_eq!(plan.writes.len(), 1);
    let parsed: Value = serde_json::from_str(&plan.writes[0].content).unwrap();
    let servers = parsed["mcpServers"].as_object().unwrap();
    assert!(!servers.contains_key("code-basics-browser"));
    assert!(servers.contains_key("somebody-elses"));
}

#[test]
fn uninstalling_a_file_holding_no_entry_of_ours_produces_no_write() {
    // So the UI can say "nothing to remove" rather than rewrite an unchanged
    // file — which for ~/.claude.json is a 122 KB rewrite for nothing.
    let dir = TempDir::new().unwrap();
    let plan = uninstall_plan_for(
        ProviderId::ClaudeCode,
        dir.path(),
        InstallScope::Project,
        Some(&provider_home(&dir)),
    )
    .unwrap();
    assert!(plan.writes.is_empty());
}

// ---------------------------------------------------------------------------
// The caveats
// ---------------------------------------------------------------------------

#[test]
fn every_scope_states_what_an_agent_gains_access_to() {
    for (provider, scope) in [
        (ProviderId::ClaudeCode, InstallScope::Project),
        (ProviderId::ClaudeCode, InstallScope::User),
        (ProviderId::Codex, InstallScope::User),
    ] {
        let caveats = caveats_for(provider, scope);
        assert!(
            caveats.iter().any(|c| c == PAGE_ACCESS_NOTE),
            "{provider:?}/{scope:?} must state it verbatim"
        );
    }
}

#[test]
fn the_page_access_note_names_the_session_rather_than_advising_care() {
    // "Be careful" is not a caveat. What is on the other side of a permission
    // click is the user's own authenticated session.
    assert!(PAGE_ACCESS_NOTE.contains("logged-in session"));
    assert!(PAGE_ACCESS_NOTE.contains("authenticated"));
    assert!(PAGE_ACCESS_NOTE.contains("not remembered"));
    assert!(!PAGE_ACCESS_NOTE.contains("be careful"));
}

#[test]
fn the_caveats_say_the_pipe_keeps_other_users_out_and_nothing_else() {
    // Describing it as isolation would be a false claim about a real exposure.
    let caveats = caveats_for(ProviderId::ClaudeCode, InstallScope::User);
    let pipe = caveats
        .iter()
        .find(|c| c.contains("named pipe"))
        .expect("the pipe must be described");
    assert!(pipe.contains("nothing else"), "{pipe}");
    assert!(pipe.contains("running as you"), "{pipe}");
    assert!(pipe.contains("names the program that asked"), "{pipe}");
}

#[test]
fn the_caveats_say_installing_grants_nothing() {
    let caveats = caveats_for(ProviderId::Codex, InstallScope::User);
    assert!(caveats
        .iter()
        .any(|c| c.contains("grants nothing by itself")));
}

#[test]
fn the_caveats_say_there_is_no_javascript_evaluation() {
    let caveats = caveats_for(ProviderId::Codex, InstallScope::User);
    assert!(caveats
        .iter()
        .any(|c| c.contains("no tool that runs JavaScript")));
}

#[test]
fn the_project_scope_warns_that_the_file_is_shared_and_carries_a_local_path() {
    let caveats = caveats_for(ProviderId::ClaudeCode, InstallScope::Project);
    assert!(caveats.iter().any(|c| c.contains("committed and shared")));
    assert!(caveats.iter().any(|c| c.contains("same path")));
    assert!(caveats
        .iter()
        .any(|c| c.contains("approve a project .mcp.json")));
}

#[test]
fn these_are_not_the_sql_caveats_with_a_word_changed() {
    // The SQL note is about a database login; sharing its words here would be
    // a warning about the wrong feature.
    let ours = caveats_for(ProviderId::ClaudeCode, InstallScope::User);
    for caveat in &ours {
        assert!(!caveat.contains("read access to your database"), "{caveat}");
        assert!(!caveat.contains("expose to agents"), "{caveat}");
    }
    assert_ne!(
        ours,
        crate::mcp::install::plan::caveats_for(ProviderId::ClaudeCode, InstallScope::User)
    );
}

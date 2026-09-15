use super::*;

use tempfile::TempDir;

/// A provider home under a tempdir. **Every** test passes one: with `None`,
/// `status_for` reads the developer's real `~/.claude.json`.
fn provider_home(dir: &TempDir) -> std::path::PathBuf {
    dir.path().join(".claude")
}

// ---------------------------------------------------------------------------
// The entry
// ---------------------------------------------------------------------------

#[test]
fn the_entry_is_a_command_field_and_an_args_array() {
    let entry = editor_mcp_entry(
        r"C:\Program Files\code basics\cb-app.exe",
        Some(Path::new(r"C:\code\repo")),
    );
    assert_eq!(
        entry["command"],
        json!(r"C:\Program Files\code basics\cb-app.exe")
    );
    assert_eq!(
        entry["args"],
        json!(["mcp-editor", "--workspace", r"C:\code\repo"])
    );
}

#[test]
fn the_exe_path_is_never_quoted_into_a_command_string() {
    let hostile = r#"C:\a b\c"d&e\cb-app.exe"#;
    let entry = editor_mcp_entry(hostile, None);
    assert_eq!(entry["command"].as_str().unwrap(), hostile);
    assert!(entry["args"]
        .as_array()
        .unwrap()
        .iter()
        .all(|arg| !arg.as_str().unwrap().contains("cb-app.exe")));
}

#[test]
fn the_workspace_is_a_separate_arg_and_never_quoted_into_one() {
    // A workspace path with a space must stay its own argv element.
    let entry = editor_mcp_entry("cb-app.exe", Some(Path::new(r"C:\my repo\proj")));
    let args: Vec<String> = entry["args"]
        .as_array()
        .unwrap()
        .iter()
        .map(|a| a.as_str().unwrap().to_string())
        .collect();
    assert_eq!(
        args,
        vec![
            "mcp-editor".to_string(),
            "--workspace".to_string(),
            r"C:\my repo\proj".to_string()
        ]
    );
    // And the line it writes really does parse back with that workspace.
    let mut command_line = vec!["cb-app.exe".to_string()];
    command_line.extend(args);
    let parsed = argv::parse_mcp_editor_args(&command_line).unwrap();
    assert_eq!(parsed.workspace.as_deref(), Some(r"C:\my repo\proj"));
}

#[test]
fn a_user_scope_entry_writes_no_workspace() {
    let entry = editor_mcp_entry("cb-app.exe", None);
    assert_eq!(entry["args"], json!(["mcp-editor"]));
}

// ---------------------------------------------------------------------------
// Where it writes — reusing `mcp::install`, not re-deciding
// ---------------------------------------------------------------------------

#[test]
fn a_project_install_bakes_in_the_workspace() {
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
    let parsed: Value = serde_json::from_str(&plan.writes[0].content).unwrap();
    let args = parsed["mcpServers"]["code-basics-editor"]["args"]
        .as_array()
        .unwrap();
    assert_eq!(args[0], "mcp-editor");
    assert_eq!(args[1], "--workspace");
    assert_eq!(args[2], json!(dir.path().display().to_string()));
}

#[test]
fn a_user_install_writes_no_workspace_argument() {
    // A user-scope entry fires everywhere and names no repository; it answers
    // NoWorkspace until scoped.
    let dir = TempDir::new().unwrap();
    let plan = install_plan_for(
        ProviderId::ClaudeCode,
        dir.path(),
        InstallScope::User,
        Some(&provider_home(&dir)),
    )
    .unwrap();
    assert!(
        !plan.writes[0].content.contains("--workspace"),
        "{}",
        plan.writes[0].content
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
        .contains("[mcp_servers.code-basics-editor]"));

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
fn installing_beside_the_other_servers_keeps_them_all() {
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

    let editor = install_plan_for(
        ProviderId::ClaudeCode,
        dir.path(),
        InstallScope::Project,
        Some(&provider_home(&dir)),
    )
    .unwrap();
    assert!(
        editor.writes[0].merges_existing,
        "an existing config must be merged into, and backed up"
    );
    let parsed: Value = serde_json::from_str(&editor.writes[0].content).unwrap();
    let servers = parsed["mcpServers"].as_object().unwrap();
    assert!(servers.contains_key("code-basics-sql"), "{servers:?}");
    assert!(servers.contains_key("code-basics-editor"), "{servers:?}");
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
    assert_eq!(
        status_for(ProviderId::Codex, dir.path(), Some(dir.path())),
        None
    );
}

#[test]
fn the_roslyn_server_being_installed_is_not_this_server_being_installed() {
    let dir = TempDir::new().unwrap();
    let home = provider_home(&dir);
    let roslyn = crate::roslyn::install::install_plan_for(
        ProviderId::ClaudeCode,
        dir.path(),
        InstallScope::Project,
        Some(&home),
    )
    .unwrap();
    std::fs::write(&roslyn.writes[0].path, &roslyn.writes[0].content).unwrap();
    assert_eq!(
        status_for(ProviderId::ClaudeCode, dir.path(), Some(&home)),
        None,
        "the servers have different names and must be reported separately"
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
                "code-basics-editor": { "command": "x", "args": ["mcp-editor"] },
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
    assert!(!servers.contains_key("code-basics-editor"));
    assert!(servers.contains_key("somebody-elses"));
}

#[test]
fn uninstalling_a_file_holding_no_entry_of_ours_produces_no_write() {
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
fn every_scope_states_the_read_only_access_verbatim() {
    for (provider, scope) in [
        (ProviderId::ClaudeCode, InstallScope::Project),
        (ProviderId::ClaudeCode, InstallScope::User),
        (ProviderId::Codex, InstallScope::User),
    ] {
        let caveats = caveats_for(provider, scope);
        assert!(
            caveats.iter().any(|c| c == ACCESS_NOTE),
            "{provider:?}/{scope:?} must state it verbatim"
        );
    }
}

#[test]
fn the_access_note_says_read_only_and_names_the_selection_and_paths_it_shares() {
    assert!(ACCESS_NOTE.contains("read-only"));
    assert!(ACCESS_NOTE.contains("never changes a file"));
    assert!(ACCESS_NOTE.contains("selected"));
    assert!(ACCESS_NOTE.contains("path"));
    assert!(!ACCESS_NOTE.contains("be careful"));
}

#[test]
fn these_are_not_the_roslyn_caveats_verbatim() {
    let ours = caveats_for(ProviderId::ClaudeCode, InstallScope::User);
    assert_ne!(
        ours,
        crate::roslyn::install::caveats_for(ProviderId::ClaudeCode, InstallScope::User)
    );
    // The access note in particular is about editor state, not code intelligence.
    assert_ne!(ACCESS_NOTE, crate::roslyn::install::ACCESS_NOTE);
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

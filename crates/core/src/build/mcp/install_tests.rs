use super::*;

use tempfile::TempDir;

fn provider_home(dir: &TempDir) -> std::path::PathBuf {
    dir.path().join(".claude")
}

#[test]
fn the_entry_is_a_command_field_and_an_args_array() {
    let entry = build_mcp_entry(
        r"C:\Program Files\code basics\cb-app.exe",
        Some(Path::new(r"C:\code\repo")),
    );
    assert_eq!(
        entry["command"],
        json!(r"C:\Program Files\code basics\cb-app.exe")
    );
    assert_eq!(
        entry["args"],
        json!(["mcp-build", "--workspace", r"C:\code\repo"])
    );
}

#[test]
fn the_workspace_is_a_separate_arg_that_parses_back() {
    let entry = build_mcp_entry("cb-app.exe", Some(Path::new(r"C:\my repo\proj")));
    let args: Vec<String> = entry["args"]
        .as_array()
        .unwrap()
        .iter()
        .map(|a| a.as_str().unwrap().to_string())
        .collect();
    let mut command_line = vec!["cb-app.exe".to_string()];
    command_line.extend(args);
    let parsed = argv::parse_mcp_build_args(&command_line).unwrap();
    assert_eq!(parsed.workspace.as_deref(), Some(r"C:\my repo\proj"));
}

#[test]
fn a_user_scope_entry_writes_no_workspace() {
    let entry = build_mcp_entry("cb-app.exe", None);
    assert_eq!(entry["args"], json!(["mcp-build"]));
}

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
    let args = parsed["mcpServers"]["code-basics-build"]["args"]
        .as_array()
        .unwrap();
    assert_eq!(args[0], "mcp-build");
    assert_eq!(args[1], "--workspace");
    assert_eq!(args[2], json!(dir.path().display().to_string()));
}

#[test]
fn a_user_install_writes_no_workspace_argument() {
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

    let build = install_plan_for(
        ProviderId::ClaudeCode,
        dir.path(),
        InstallScope::Project,
        Some(&provider_home(&dir)),
    )
    .unwrap();
    assert!(build.writes[0].merges_existing);
    let parsed: Value = serde_json::from_str(&build.writes[0].content).unwrap();
    let servers = parsed["mcpServers"].as_object().unwrap();
    assert!(servers.contains_key("code-basics-sql"), "{servers:?}");
    assert!(servers.contains_key("code-basics-build"), "{servers:?}");
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
}

#[test]
fn uninstalling_removes_only_our_entry() {
    let dir = TempDir::new().unwrap();
    let home = provider_home(&dir);
    let path = dir.path().join(".mcp.json");
    std::fs::write(
        &path,
        serde_json::to_string_pretty(&json!({
            "mcpServers": {
                "code-basics-build": { "command": "x", "args": ["mcp-build"] },
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
    assert!(!servers.contains_key("code-basics-build"));
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

#[test]
fn every_scope_states_the_access_note_verbatim() {
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
fn the_access_note_is_honest_that_a_build_writes_output_but_not_source() {
    assert!(ACCESS_NOTE.contains("changes no source file"));
    assert!(!ACCESS_NOTE.contains("be careful"));
}

#[test]
fn the_project_scope_warns_that_the_file_is_shared() {
    let caveats = caveats_for(ProviderId::ClaudeCode, InstallScope::Project);
    assert!(caveats.iter().any(|c| c.contains("committed and shared")));
    assert!(caveats
        .iter()
        .any(|c| c.contains("approve a project .mcp.json")));
}

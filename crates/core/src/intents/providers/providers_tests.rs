//! Tests for provider detection and hook installation.
//! Included by `providers/mod.rs` under `#[cfg(test)]`.

use super::*;
use serde_json::{json, Value};

fn workspace() -> tempfile::TempDir {
    tempfile::tempdir().unwrap()
}

fn read(path: &Path) -> Value {
    serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap()
}

/// A `hooks.json` in the shape found on a real machine: several unrelated
/// hooks, on every event, that must survive an install untouched.
fn existing_dashboard_hooks() -> String {
    let handler = json!({
        "type": "command",
        "command": "\"C:\\\\Program Files\\\\USB LCD Dashboard\\\\pythonw.exe\" -m usb_lcd_dashboard emit",
        "timeout": 2,
    });

    let mut hooks = serde_json::Map::new();
    for event in [
        "SessionStart",
        "UserPromptSubmit",
        "PreToolUse",
        "PermissionRequest",
        "PostToolUse",
        "Stop",
        "SessionEnd",
    ] {
        hooks.insert(event.to_string(), json!([ { "hooks": [handler.clone()] } ]));
    }

    serde_json::to_string_pretty(&json!({ "hooks": hooks })).unwrap()
}

// -- merging into a file the user already owns ------------------------------

/// The reason this module exists. The development machine's global Codex
/// config drove a physical LCD dashboard from all seven events; an install
/// that replaced the file would have silently broken it.
#[test]
fn installing_preserves_every_existing_hook_entry() {
    let dir = workspace();
    let path = dir.path().join("hooks.json");
    std::fs::write(&path, existing_dashboard_hooks()).unwrap();

    let (merged, merges_existing) = hooks_json::plan_merge(&path, dir.path()).unwrap();
    std::fs::write(&path, merged).unwrap();

    assert!(merges_existing);
    let value = read(&path);
    let hooks = value.get("hooks").unwrap().as_object().unwrap();

    // Every original event is still there.
    for event in [
        "SessionStart",
        "UserPromptSubmit",
        "PreToolUse",
        "PermissionRequest",
        "PostToolUse",
        "Stop",
        "SessionEnd",
    ] {
        assert!(hooks.contains_key(event), "{event} was dropped");
    }

    // And the dashboard command survives on the two events we also write to.
    for event in hooks_json::EVENTS {
        let text = serde_json::to_string(hooks.get(*event).unwrap()).unwrap();
        assert!(
            text.contains("usb_lcd_dashboard"),
            "{event} lost its existing hook: {text}"
        );
        assert!(
            text.contains(hooks_json::MARKER),
            "{event} did not gain ours"
        );
    }
}

#[test]
fn installing_into_a_missing_file_creates_it() {
    let dir = workspace();
    let path = dir.path().join(".codex").join("hooks.json");

    let (content, merges_existing) = hooks_json::plan_merge(&path, dir.path()).unwrap();

    assert!(!merges_existing);
    let value: Value = serde_json::from_str(&content).unwrap();
    assert!(value.get("hooks").unwrap().get("PostToolUse").is_some());
    assert!(value.get("hooks").unwrap().get("Stop").is_some());
}

#[test]
fn installing_twice_does_not_duplicate_the_entry() {
    let dir = workspace();
    let path = dir.path().join("hooks.json");

    let (first, _) = hooks_json::plan_merge(&path, dir.path()).unwrap();
    std::fs::write(&path, first).unwrap();
    let (second, _) = hooks_json::plan_merge(&path, dir.path()).unwrap();
    std::fs::write(&path, second).unwrap();

    let value = read(&path);
    let entries = value["hooks"]["PostToolUse"].as_array().unwrap();
    let ours = entries
        .iter()
        .filter(|e| {
            serde_json::to_string(e)
                .unwrap()
                .contains(hooks_json::MARKER)
        })
        .count();

    assert_eq!(ours, 1, "installed twice: {entries:?}");
}

/// Unrelated settings sharing the file — the Claude Code case, where hooks
/// live inside `settings.json` next to everything else.
#[test]
fn unrelated_settings_in_the_same_file_are_preserved() {
    let dir = workspace();
    let path = dir.path().join("settings.json");
    std::fs::write(
        &path,
        r#"{ "model": "opus", "env": { "FOO": "bar" }, "permissions": { "allow": ["Bash"] } }"#,
    )
    .unwrap();

    let (merged, _) = hooks_json::plan_merge(&path, dir.path()).unwrap();
    let value: Value = serde_json::from_str(&merged).unwrap();

    assert_eq!(value["model"], "opus");
    assert_eq!(value["env"]["FOO"], "bar");
    assert_eq!(value["permissions"]["allow"][0], "Bash");
    assert!(value["hooks"]["Stop"].is_array());
}

#[test]
fn a_malformed_file_is_reported_and_left_untouched() {
    let dir = workspace();
    let path = dir.path().join("hooks.json");
    std::fs::write(&path, "{ not json at all").unwrap();

    let error = hooks_json::plan_merge(&path, dir.path())
        .unwrap_err()
        .to_string();

    assert!(error.contains("left untouched"), "got: {error}");
    assert_eq!(std::fs::read_to_string(&path).unwrap(), "{ not json at all");
}

#[test]
fn an_empty_file_is_treated_as_no_configuration() {
    let dir = workspace();
    let path = dir.path().join("hooks.json");
    std::fs::write(&path, "   \n").unwrap();

    let (content, _) = hooks_json::plan_merge(&path, dir.path()).unwrap();

    assert!(serde_json::from_str::<Value>(&content).unwrap()["hooks"]["Stop"].is_array());
}

#[test]
fn a_hooks_key_of_an_unexpected_type_is_not_destroyed() {
    let dir = workspace();
    let path = dir.path().join("hooks.json");
    std::fs::write(&path, r#"{ "hooks": "somewhere/else.json" }"#).unwrap();

    let (content, _) = hooks_json::plan_merge(&path, dir.path()).unwrap();
    let value: Value = serde_json::from_str(&content).unwrap();

    assert_eq!(value["hooks"], "somewhere/else.json");
}

#[test]
fn a_file_that_is_not_an_object_is_refused() {
    let dir = workspace();
    let path = dir.path().join("hooks.json");
    std::fs::write(&path, "[1, 2, 3]").unwrap();

    assert!(hooks_json::plan_merge(&path, dir.path()).is_err());
}

// -- recognising our own installation ---------------------------------------

#[test]
fn an_untouched_file_is_not_reported_as_installed() {
    let dir = workspace();
    let path = dir.path().join("hooks.json");
    std::fs::write(&path, existing_dashboard_hooks()).unwrap();

    assert!(!hooks_json::is_installed(&path));
}

#[test]
fn a_merged_file_is_reported_as_installed() {
    let dir = workspace();
    let path = dir.path().join("hooks.json");
    let (content, _) = hooks_json::plan_merge(&path, dir.path()).unwrap();
    std::fs::write(&path, content).unwrap();

    assert!(hooks_json::is_installed(&path));
}

#[test]
fn a_missing_file_is_not_reported_as_installed() {
    let dir = workspace();

    assert!(!hooks_json::is_installed(&dir.path().join("nothing.json")));
}

// -- removal ----------------------------------------------------------------

#[test]
fn removing_our_hooks_leaves_the_users_own_alone() {
    let dir = workspace();
    let path = dir.path().join("hooks.json");
    std::fs::write(&path, existing_dashboard_hooks()).unwrap();
    let (merged, _) = hooks_json::plan_merge(&path, dir.path()).unwrap();
    std::fs::write(&path, merged).unwrap();

    let removed = hooks_json::plan_removal(&path).unwrap().unwrap();
    std::fs::write(&path, removed).unwrap();

    let text = std::fs::read_to_string(&path).unwrap();
    assert!(!text.contains(hooks_json::MARKER));
    assert!(text.contains("usb_lcd_dashboard"));
    assert!(!hooks_json::is_installed(&path));
}

// -- applying a plan --------------------------------------------------------

#[test]
fn applying_a_plan_backs_up_the_file_it_replaces() {
    let dir = workspace();
    let path = dir.path().join("hooks.json");
    std::fs::write(&path, existing_dashboard_hooks()).unwrap();

    let plan = InstallPlan {
        provider: ProviderId::Codex,
        scope: InstallScope::User,
        writes: vec![PlannedWrite {
            path: path.clone(),
            content: "{}\n".to_string(),
            merges_existing: true,
        }],
        caveats: Vec::new(),
    };
    apply_plan(&plan).unwrap();

    let backup = dir.path().join("hooks.json.bak");
    assert!(backup.exists(), "no backup was written");
    assert!(std::fs::read_to_string(backup)
        .unwrap()
        .contains("usb_lcd_dashboard"));
}

#[test]
fn applying_a_plan_creates_missing_directories() {
    let dir = workspace();
    let path = dir.path().join(".codex").join("nested").join("hooks.json");

    let plan = InstallPlan {
        provider: ProviderId::Codex,
        scope: InstallScope::Project,
        writes: vec![PlannedWrite {
            path: path.clone(),
            content: "{}\n".to_string(),
            merges_existing: false,
        }],
        caveats: Vec::new(),
    };
    apply_plan(&plan).unwrap();

    assert!(path.exists());
}

// -- the hook command itself ------------------------------------------------

/// A user-level hook fires for every repository, so the command has to say
/// which workspace it is for or it cannot know when to do nothing.
#[test]
fn the_hook_command_names_its_workspace_and_carries_the_marker() {
    let dir = workspace();
    let commands = hooks_json::commands_for(dir.path(), "codex");

    let text = serde_json::to_string(&commands).unwrap();

    assert!(text.contains(hooks_json::MARKER));
    assert!(text.contains("--provider codex"));
    assert!(text.contains("--event PostToolUse"));
    assert!(text.contains("--event Stop"));
}

/// Codex accepts Claude Code's tool names as aliases for `apply_patch`, so one
/// matcher is correct for both agents.
#[test]
fn the_edit_matcher_covers_both_agents_tool_names() {
    assert!(EDIT_TOOL_MATCHER.contains("apply_patch"));
    assert!(EDIT_TOOL_MATCHER.contains("Edit"));
    assert!(EDIT_TOOL_MATCHER.contains("Write"));
}

#[test]
fn only_the_edit_event_carries_a_tool_matcher() {
    let dir = workspace();
    let commands = hooks_json::commands_for(dir.path(), "claudeCode");

    assert_eq!(commands["PostToolUse"][0]["matcher"], EDIT_TOOL_MATCHER);
    assert_eq!(commands["Stop"][0]["matcher"], "");
}

// -- install plans ----------------------------------------------------------

#[test]
fn a_project_install_targets_a_file_inside_the_workspace() {
    let dir = workspace();

    let plan = codex::Codex::new()
        .install_plan(dir.path(), InstallScope::Project)
        .unwrap();

    assert!(plan.writes[0].path.starts_with(dir.path()));
    assert!(plan.writes[0].path.ends_with("hooks.json"));
}

/// Hooks alone only get the fallback label — the first sentence of the closing
/// message. The request for a real one goes in the agent's instruction file.
#[test]
fn an_install_also_asks_the_agent_for_a_label() {
    let dir = workspace();

    let claude = claude_code::ClaudeCode::new()
        .install_plan(dir.path(), InstallScope::Project)
        .unwrap();
    let codex = codex::Codex::new()
        .install_plan(dir.path(), InstallScope::Project)
        .unwrap();

    assert!(claude.writes.iter().any(|w| w.path.ends_with("CLAUDE.md")));
    assert!(codex.writes.iter().any(|w| w.path.ends_with("AGENTS.md")));
}

/// Every file an install touches has to be visible before it is written —
/// including one the user wrote themselves.
#[test]
fn the_instruction_file_appears_in_the_plan_with_its_full_contents() {
    let dir = workspace();
    std::fs::write(dir.path().join("AGENTS.md"), "# House rules\n").unwrap();

    let plan = codex::Codex::new()
        .install_plan(dir.path(), InstallScope::Project)
        .unwrap();

    let write = plan
        .writes
        .iter()
        .find(|w| w.path.ends_with("AGENTS.md"))
        .expect("the instruction file");

    assert!(write.merges_existing);
    assert!(write.content.starts_with("# House rules"));
    assert!(write.content.contains("Intent:"));
    assert!(
        plan.caveats.iter().any(|c| c.contains("AGENTS.md")),
        "the user should be told: {:?}",
        plan.caveats
    );
}

#[test]
fn a_second_install_does_not_append_the_request_again() {
    let dir = workspace();
    let plan = codex::Codex::new()
        .install_plan(dir.path(), InstallScope::Project)
        .unwrap();
    apply_plan(&plan).unwrap();

    let again = codex::Codex::new()
        .install_plan(dir.path(), InstallScope::Project)
        .unwrap();

    assert!(
        !again.writes.iter().any(|w| w.path.ends_with("AGENTS.md")),
        "the request is already there"
    );
}

#[test]
fn a_claude_code_project_install_warns_that_the_file_is_shared() {
    let dir = workspace();

    let plan = claude_code::ClaudeCode::new()
        .install_plan(dir.path(), InstallScope::Project)
        .unwrap();

    assert!(
        plan.caveats.iter().any(|c| c.contains("committed")),
        "got: {:?}",
        plan.caveats
    );
}

/// A plan is for showing to the user; computing one must not touch disk.
#[test]
fn computing_a_plan_writes_nothing() {
    let dir = workspace();
    let plan = codex::Codex::new()
        .install_plan(dir.path(), InstallScope::Project)
        .unwrap();

    assert!(!plan.writes[0].path.exists());
}

// -- the IPC contract -------------------------------------------------------

/// `src/ipc/types.ts` mirrors these by hand; a rename must fail here rather
/// than surfacing as an undefined value in the UI.
fn keys(value: &serde_json::Value) -> Vec<String> {
    let mut keys: Vec<String> = value
        .as_object()
        .expect("expected a JSON object")
        .keys()
        .cloned()
        .collect();
    keys.sort();
    keys
}

#[test]
fn provider_status_serialises_with_the_keys_the_ui_reads() {
    let status = ProviderStatus {
        provider: ProviderId::Codex,
        detected: true,
        capture: Some(InstallScope::Project),
        sessions: 3,
        caveats: vec!["something to know".into()],
    };

    assert_eq!(
        keys(&serde_json::to_value(&status).unwrap()),
        ["capture", "caveats", "detected", "provider", "sessions"]
    );
}

#[test]
fn empty_caveats_stay_out_of_the_payload() {
    let status = ProviderStatus::absent(ProviderId::ClaudeCode);

    assert!(!keys(&serde_json::to_value(&status).unwrap()).contains(&"caveats".to_string()));
}

#[test]
fn an_install_plan_serialises_with_the_keys_the_ui_reads() {
    let plan = InstallPlan {
        provider: ProviderId::ClaudeCode,
        scope: InstallScope::User,
        writes: vec![PlannedWrite {
            path: PathBuf::from("a.json"),
            content: "{}".into(),
            merges_existing: true,
        }],
        caveats: Vec::new(),
    };

    let value = serde_json::to_value(&plan).unwrap();

    assert_eq!(keys(&value), ["provider", "scope", "writes"]);
    assert_eq!(
        keys(&value["writes"][0]),
        ["content", "mergesExisting", "path"]
    );
}

#[test]
fn provider_ids_and_scopes_serialise_in_camel_case() {
    assert_eq!(
        serde_json::to_string(&[ProviderId::ClaudeCode, ProviderId::Codex]).unwrap(),
        r#"["claudeCode","codex"]"#
    );
    assert_eq!(
        serde_json::to_string(&[InstallScope::Project, InstallScope::User]).unwrap(),
        r#"["project","user"]"#
    );
}

// -- Claude Code project directory naming -----------------------------------

/// Matched all fifteen projects on the development machine.
#[test]
fn a_workspace_path_encodes_to_its_transcript_directory_name() {
    let encoded =
        claude_code::encode_project_dir(Path::new(r"C:\Users\Someone\Documents\Code\code-basics"));

    assert_eq!(encoded, "C--Users-Someone-Documents-Code-code-basics");
}

#[test]
fn dots_and_underscores_are_also_replaced_when_encoding() {
    let encoded = claude_code::encode_project_dir(Path::new(r"C:\code\my_app.v2"));

    assert_eq!(encoded, "C--code-my-app-v2");
}

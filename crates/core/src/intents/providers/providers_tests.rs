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
        "SubagentStop",
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

    let (merged, merges_existing) = hooks_json::plan_merge(&path, Some(dir.path())).unwrap();
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
        "SubagentStop",
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

    let (content, merges_existing) = hooks_json::plan_merge(&path, Some(dir.path())).unwrap();

    assert!(!merges_existing);
    let value: Value = serde_json::from_str(&content).unwrap();
    assert!(value.get("hooks").unwrap().get("PostToolUse").is_some());
    assert!(value.get("hooks").unwrap().get("Stop").is_some());
}

#[test]
fn installing_twice_does_not_duplicate_the_entry() {
    let dir = workspace();
    let path = dir.path().join("hooks.json");

    let (first, _) = hooks_json::plan_merge(&path, Some(dir.path())).unwrap();
    std::fs::write(&path, first).unwrap();
    let (second, _) = hooks_json::plan_merge(&path, Some(dir.path())).unwrap();
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

    let (merged, _) = hooks_json::plan_merge(&path, Some(dir.path())).unwrap();
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

    let error = hooks_json::plan_merge(&path, Some(dir.path()))
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

    let (content, _) = hooks_json::plan_merge(&path, Some(dir.path())).unwrap();

    assert!(serde_json::from_str::<Value>(&content).unwrap()["hooks"]["Stop"].is_array());
}

#[test]
fn a_hooks_key_of_an_unexpected_type_is_not_destroyed() {
    let dir = workspace();
    let path = dir.path().join("hooks.json");
    std::fs::write(&path, r#"{ "hooks": "somewhere/else.json" }"#).unwrap();

    let (content, _) = hooks_json::plan_merge(&path, Some(dir.path())).unwrap();
    let value: Value = serde_json::from_str(&content).unwrap();

    assert_eq!(value["hooks"], "somewhere/else.json");
}

#[test]
fn a_file_that_is_not_an_object_is_refused() {
    let dir = workspace();
    let path = dir.path().join("hooks.json");
    std::fs::write(&path, "[1, 2, 3]").unwrap();

    assert!(hooks_json::plan_merge(&path, Some(dir.path())).is_err());
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
    let (content, _) = hooks_json::plan_merge(&path, Some(dir.path())).unwrap();
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
    let (merged, _) = hooks_json::plan_merge(&path, Some(dir.path())).unwrap();
    std::fs::write(&path, merged).unwrap();

    let removed = hooks_json::plan_removal(&path).unwrap().unwrap();
    std::fs::write(&path, removed).unwrap();

    let text = std::fs::read_to_string(&path).unwrap();
    assert!(!text.contains(hooks_json::MARKER));
    assert!(text.contains("usb_lcd_dashboard"));
    assert!(!hooks_json::is_installed(&path));
}

/// Disabling one agent's capture must remove only that agent's own hook-config
/// entries, never the other agent's. The two live in separate files, and a
/// disable that reached into the wrong one would silently stop the agent the
/// user did not touch.
#[test]
fn disable_removes_only_that_providers_entries() {
    let dir = workspace();
    let claude_path = dir.path().join(".claude").join("settings.json");
    let codex_path = dir.path().join(".codex").join("hooks.json");

    // Both agents' recorders installed into their own project hook files.
    for path in [&claude_path, &codex_path] {
        let (content, _) = hooks_json::plan_merge(path, Some(dir.path())).unwrap();
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, content).unwrap();
    }
    assert!(hooks_json::is_installed(&claude_path));
    assert!(hooks_json::is_installed(&codex_path));

    // Disable only Claude Code.
    let plan = uninstall_plan(ProviderId::ClaudeCode, dir.path(), InstallScope::Project).unwrap();

    // The plan rewrites Claude's file, removing its marker, and never names
    // Codex's file at all.
    let claude_write = plan
        .writes
        .iter()
        .find(|w| w.path == claude_path)
        .expect("Claude's settings.json is in the plan");
    assert!(!claude_write.content.contains(hooks_json::MARKER));
    assert!(
        plan.writes.iter().all(|w| w.path != codex_path),
        "disabling Claude must not touch Codex's hooks.json: {:?}",
        plan.writes.iter().map(|w| &w.path).collect::<Vec<_>>()
    );

    // Applying it leaves Codex's recorder installed and Claude's gone.
    apply_writes(&plan.writes).unwrap();
    assert!(!hooks_json::is_installed(&claude_path));
    assert!(hooks_json::is_installed(&codex_path));
}

// -- shared repo-level hooks: gated on whether another agent still captures --

/// A fresh git repository, so the shared `pre-commit`/`post-commit` hooks have
/// somewhere to live — `guard::hook_path`/`whyhook::hook_path` resolve to `None`
/// outside a repository.
fn git_workspace() -> tempfile::TempDir {
    let dir = workspace();
    git2::Repository::init(dir.path()).unwrap();
    dir
}

/// The pre-commit guard and post-commit why-hook are seeded by *both* providers'
/// installs, so disabling one agent must take them with it only when no *other*
/// agent is still capturing here. This pins that decision against the real
/// `status(root).capture` check the orchestration uses, in whichever direction
/// this machine's installed agents put it: shared-hook removal appears in the
/// plan exactly when nothing else still captures.
#[test]
fn shared_hooks_are_removed_only_when_no_other_provider_still_captures() {
    let dir = git_workspace();
    let root = dir.path();

    // Installing Claude Code intent capture also seeds the two shared hooks.
    let plan = claude_code::ClaudeCode::new()
        .install_plan(root, InstallScope::Project)
        .unwrap();
    apply_writes(&plan.writes).unwrap();

    let guard_hook = crate::intents::guard::hook_path(root).expect("a pre-commit path in a repo");
    let why_hook = crate::intents::whyhook::hook_path(root).expect("a post-commit path in a repo");
    assert!(
        crate::intents::guard::is_installed(&guard_hook),
        "the guard was seeded"
    );
    assert!(why_hook.exists(), "the why-hook was seeded");

    // The exact signal the orchestration gates on: any *other* provider capturing.
    let another_captures = all()
        .iter()
        .filter(|p| p.id() != ProviderId::ClaudeCode)
        .any(|p| p.status(root).capture.is_some());

    let plan = uninstall_plan(ProviderId::ClaudeCode, root, InstallScope::Project).unwrap();
    let removes_guard = plan.writes.iter().any(|w| w.path == guard_hook);
    let removes_why = plan.writes.iter().any(|w| w.path == why_hook);

    assert_eq!(
        removes_guard, !another_captures,
        "guard removal must track 'no other provider captures' (another={another_captures})"
    );
    assert_eq!(
        removes_why, !another_captures,
        "why-hook removal must track 'no other provider captures' (another={another_captures})"
    );
}

/// The part (b) case made concrete: with a second agent (Codex) also capturing
/// at project scope, disabling Claude Code must leave the shared hooks in place.
/// Codex reports capture only where it is installed on this machine, so — like
/// the sibling `statuses` test — the leave-in-place assertion is made where
/// Codex is present and the last-agent fallback is checked where it is not.
#[test]
fn a_second_capturing_agent_keeps_the_shared_hooks_in_place() {
    let dir = git_workspace();
    let root = dir.path();

    // Both agents' capture installed at project scope; either install seeds the
    // shared hooks (the second finds the block current and adds nothing).
    for provider in all() {
        let plan = provider.install_plan(root, InstallScope::Project).unwrap();
        apply_writes(&plan.writes).unwrap();
    }

    let guard_hook = crate::intents::guard::hook_path(root).unwrap();
    let why_hook = crate::intents::whyhook::hook_path(root).unwrap();

    // Exactly the "another provider" the gate weighs when Claude Code is disabled.
    let codex_still_captures = codex::Codex::new().status(root).capture.is_some();

    let plan = uninstall_plan(ProviderId::ClaudeCode, root, InstallScope::Project).unwrap();
    let removes_shared = plan
        .writes
        .iter()
        .any(|w| w.path == guard_hook || w.path == why_hook);

    if codex_still_captures {
        assert!(
            !removes_shared,
            "Codex still captures, so the shared hooks must be left in place: {:?}",
            plan.writes.iter().map(|w| &w.path).collect::<Vec<_>>()
        );
    } else {
        assert!(
            removes_shared,
            "no other agent captures, so disabling the last one takes the shared hooks"
        );
    }
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

/// A project-scope hook belongs to exactly one workspace, so it says so.
#[test]
fn the_hook_command_names_its_workspace_and_carries_the_marker() {
    let dir = workspace();
    let commands = hooks_json::commands_for(Some(dir.path()), "codex");

    let text = serde_json::to_string(&commands).unwrap();

    assert!(text.contains(hooks_json::MARKER));
    assert!(text.contains("--provider codex"));
    assert!(text.contains("--event PostToolUse"));
    assert!(text.contains("--event Stop"));
    assert!(text.contains("--workspace"));
}

/// A user-level hook fires for every repository, so pinning it to the one that
/// happened to be open at install time is what made it record nowhere else.
/// With no workspace named, the payload's `cwd` decides.
#[test]
fn a_user_scope_hook_command_omits_the_workspace_flag() {
    let commands = hooks_json::commands_for(None, "codex");

    let text = serde_json::to_string(&commands).unwrap();

    assert!(text.contains(hooks_json::MARKER));
    assert!(text.contains("--provider codex"));
    assert!(
        !text.contains("--workspace"),
        "a user-scope hook must not be pinned: {text}"
    );
}

#[test]
fn a_user_scope_install_plan_does_not_pin_a_workspace() {
    let dir = workspace();
    let home = workspace();

    let claude = claude_code::ClaudeCode::with_home(home.path())
        .install_plan(dir.path(), InstallScope::User)
        .unwrap();
    let codex = codex::Codex::new()
        .install_plan_in(Some(home.path()), dir.path(), InstallScope::User)
        .unwrap();

    for plan in [claude, codex] {
        let settings = plan
            .writes
            .iter()
            .find(|w| w.content.contains(hooks_json::MARKER))
            .expect("the hook file");
        assert!(
            !settings.content.contains("--workspace"),
            "{:?} pinned a workspace: {}",
            plan.provider,
            settings.content
        );
    }
}

/// Re-running setup is how an instruction section written by an older version
/// gets its wording updated: the plan must carry the rewrite, not skip the
/// file because the marker is already there.
#[test]
fn re_enabling_refreshes_a_stale_instruction_section() {
    let dir = workspace();
    let home = workspace();
    std::fs::write(
        dir.path().join("CLAUDE.md"),
        format!(
            "# Mine\n\n{}\nStale request wording.\n{}\n",
            instructions::MARKER,
            instructions::END_MARKER
        ),
    )
    .unwrap();

    let plan = claude_code::ClaudeCode::with_home(home.path())
        .install_plan(dir.path(), InstallScope::User)
        .unwrap();

    let write = plan
        .writes
        .iter()
        .find(|w| w.path.ends_with("CLAUDE.md"))
        .expect("the instruction rewrite");
    assert!(write.content.starts_with("# Mine"));
    assert!(write.content.contains("Intent: "));
    assert!(!write.content.contains("Stale request wording."));
}

#[test]
fn a_project_scope_install_plan_still_names_the_workspace() {
    let dir = workspace();

    let plan = codex::Codex::new()
        .install_plan(dir.path(), InstallScope::Project)
        .unwrap();

    assert!(plan.writes[0].content.contains("--workspace"));
}

// -- repairing an install pinned to another workspace ------------------------

/// The shape a pinned install left behind on the development machine: a
/// user-level hook naming a repository the user is no longer in.
fn pinned_user_settings(path: &Path, pinned_to: &Path) {
    let (content, _) = hooks_json::plan_merge(path, Some(pinned_to)).unwrap();
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, content).unwrap();
}

#[test]
fn pinned_workspace_reads_the_path_out_of_a_pinned_hook() {
    let dir = workspace();
    let path = dir.path().join("settings.json");
    let other = Path::new(r"C:\Users\Someone\Code\ONEflight");
    pinned_user_settings(&path, other);

    assert_eq!(
        hooks_json::pinned_workspace(&path).as_deref(),
        Some(r"C:\Users\Someone\Code\ONEflight")
    );
}

#[test]
fn pinned_workspace_is_none_for_the_unpinned_form() {
    let dir = workspace();
    let path = dir.path().join("settings.json");
    let (content, _) = hooks_json::plan_merge(&path, None).unwrap();
    std::fs::write(&path, content).unwrap();

    assert_eq!(hooks_json::pinned_workspace(&path), None);
    assert_eq!(
        hooks_json::pinned_workspace(&dir.path().join("nothing.json")),
        None
    );
}

#[test]
fn a_user_hook_pinned_to_another_workspace_reports_not_capturing_with_a_caveat() {
    let dir = workspace();
    let home = workspace();
    let other = workspace();
    pinned_user_settings(&home.path().join("settings.json"), other.path());
    pinned_user_settings(&home.path().join("hooks.json"), other.path());

    let claude = claude_code::ClaudeCode::with_home(home.path()).status(dir.path());
    let codex = codex::Codex::new().status_in(Some(home.path()), dir.path());

    for status in [claude, codex] {
        assert_eq!(
            status.capture, None,
            "{:?} reported capture from a pinned hook",
            status.provider
        );
        assert!(
            status
                .caveats
                .iter()
                .any(|c| c.contains("pinned to") && c.contains("Enable capture again")),
            "{:?} said nothing about the pin: {:?}",
            status.provider,
            status.caveats
        );
    }
}

/// The repair is just installing again: the marker entry is replaced in place.
#[test]
fn re_enabling_user_capture_replaces_the_pinned_hook_entry() {
    let dir = workspace();
    let path = dir.path().join("settings.json");
    pinned_user_settings(&path, Path::new(r"C:\Users\Someone\Code\ONEflight"));

    let (content, _) = hooks_json::plan_merge(&path, None).unwrap();
    std::fs::write(&path, content).unwrap();

    let value = read(&path);
    for event in hooks_json::EVENTS {
        let entries = value["hooks"][*event].as_array().unwrap();
        let ours = entries
            .iter()
            .filter(|e| {
                serde_json::to_string(e)
                    .unwrap()
                    .contains(hooks_json::MARKER)
            })
            .count();
        assert_eq!(ours, 1, "{event} kept a duplicate: {entries:?}");
    }
    assert_eq!(hooks_json::pinned_workspace(&path), None);
    assert!(hooks_json::is_installed(&path));
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
    let commands = hooks_json::commands_for(Some(dir.path()), "claudeCode");

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

// -- the provider registry --------------------------------------------------

/// Both agents are always offered, installed or not — the UI needs a row for
/// one that is missing just as much as for one that is present.
#[test]
fn every_known_provider_is_listed_whether_or_not_it_is_installed() {
    let ids: Vec<ProviderId> = all().iter().map(|p| p.id()).collect();

    assert_eq!(ids, vec![ProviderId::ClaudeCode, ProviderId::Codex]);
}

#[test]
fn statuses_reports_one_row_per_provider_in_registry_order() {
    let dir = workspace();

    let statuses = statuses(dir.path());

    assert_eq!(
        statuses.iter().map(|s| s.provider).collect::<Vec<_>>(),
        vec![ProviderId::ClaudeCode, ProviderId::Codex]
    );
}

/// A workspace no agent has ever been run in has no sessions, whatever the
/// machine's own home directories hold.
///
/// Capture is deliberately not asserted here: a user-level install on the
/// developer's own machine legitimately shows as `Some(User)` for every
/// workspace, which is the point of a user-level install.
#[test]
fn a_fresh_workspace_has_no_sessions_for_any_provider() {
    let dir = workspace();

    for status in statuses(dir.path()) {
        assert_eq!(status.sessions, 0, "{:?}: {status:?}", status.provider);
    }
}

/// Installing hooks in the workspace is visible through the registry, not just
/// through the provider itself.
///
/// **Conditional on Codex being installed on this machine**, and it says so
/// rather than skipping quietly. `statuses` resolves the real Codex home, and
/// `status_in` reports a provider whose home directory does not exist as
/// `absent` — correctly, since hooks written into a workspace do nothing for an
/// agent that is not there. So on a machine without Codex the row is `detected:
/// false, capture: None`, which is the right answer and not the one this test is
/// about.
///
/// It was written on a machine that has Codex and asserted the installed branch
/// unconditionally, so it failed the first time the suite was run in a container
/// — `left: None, right: Some(Project)` — with nothing in the failure to
/// suggest the machine rather than the code. Both branches are asserted now, so
/// the test says something wherever it runs.
#[test]
fn a_project_install_shows_up_in_the_statuses_row_where_codex_is_installed() {
    let dir = workspace();
    let plan = codex::Codex::new()
        .install_plan(dir.path(), InstallScope::Project)
        .unwrap();
    apply_plan(&plan).unwrap();

    let row = statuses(dir.path())
        .into_iter()
        .find(|s| s.provider == ProviderId::Codex)
        .expect("a Codex row");

    if row.detected {
        assert_eq!(row.capture, Some(InstallScope::Project));
    } else {
        assert_eq!(
            row.capture, None,
            "Codex is not installed here, so hooks written into the workspace \
             configure nothing and the row must not claim capture is on"
        );
    }
}

#[test]
fn a_workspace_no_agent_has_run_in_has_no_merged_history() {
    let dir = workspace();

    let HistoryMined {
        records, labels, ..
    } = history(dir.path());

    assert!(records.is_empty(), "got: {records:?}");
    assert!(labels.is_empty(), "got: {labels:?}");
}

/// Records from the two providers interleave by sequence number; the merge has
/// to leave them ordered or the intent cards read out of order.
#[test]
fn merged_history_comes_back_sorted_by_sequence() {
    let dir = workspace();

    let HistoryMined { records, .. } = history(dir.path());

    let seqs: Vec<u64> = records.iter().map(|r| r.seq).collect();
    let mut sorted = seqs.clone();
    sorted.sort_unstable();
    assert_eq!(seqs, sorted);
}

// -- the absent status ------------------------------------------------------

#[test]
fn an_absent_provider_reports_nothing_available_and_keeps_its_id() {
    let status = ProviderStatus::absent(ProviderId::ClaudeCode);

    assert_eq!(status.provider, ProviderId::ClaudeCode);
    assert!(!status.detected);
    assert_eq!(status.capture, None);
    assert_eq!(status.sessions, 0);
    assert!(status.caveats.is_empty());
}

// -- backups ----------------------------------------------------------------

/// The backup keeps the original extension so it is obviously a copy of that
/// file and not of a different one.
#[test]
fn a_backup_keeps_the_extension_of_the_file_it_copies() {
    assert_eq!(backup_extension(Path::new("hooks.json")), "json.bak");
    assert_eq!(backup_extension(Path::new("CLAUDE.md")), "md.bak");
    assert_eq!(backup_extension(Path::new("settings")), "bak");
}

/// A file being created for the first time has nothing to back up.
#[test]
fn applying_a_plan_that_creates_a_file_writes_no_backup() {
    let dir = workspace();
    let path = dir.path().join("hooks.json");

    apply_plan(&InstallPlan {
        provider: ProviderId::Codex,
        scope: InstallScope::User,
        writes: vec![PlannedWrite {
            path: path.clone(),
            content: "{}\n".to_string(),
            merges_existing: false,
        }],
        caveats: Vec::new(),
    })
    .unwrap();

    assert!(path.exists());
    assert!(!dir.path().join("hooks.json.bak").exists());
}

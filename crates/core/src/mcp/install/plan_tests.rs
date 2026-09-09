use super::*;
use crate::intents::providers::{apply_writes, claude_code::ClaudeCode, Provider};
use std::fs;
use tempfile::tempdir;

/// An isolated stand-in for the user's home directory.
///
/// **Mandatory, not tidiness.** `status_for` checks project scope and then
/// **user** scope, and user scope is the real `~/.claude.json`. Passing `None`
/// for `home` therefore asks a question about the machine the test is running
/// on: these two tests passed for months and began failing the moment someone
/// actually installed the server for themselves, because `status_for` correctly
/// answered `Some(User)` where the test had hard-coded `None`.
///
/// A test that reads the developer's own configuration is not testing this code.
/// `config_target` takes `home: Option<&Path>` precisely so it never has to.
fn isolated_home(dir: &tempfile::TempDir) -> std::path::PathBuf {
    let home = dir.path().join("home");
    fs::create_dir_all(&home).expect("an empty home to look in");
    home
}
fn apply(plan: &InstallPlan) {
    apply_writes(&plan.writes).expect("writes apply");
}

#[test]
fn a_project_install_writes_dot_mcp_json_and_reads_back_as_installed() {
    let root = tempdir().unwrap();
    let home = isolated_home(&root);
    let home = Some(home.as_path());

    assert_eq!(status_for(ProviderId::ClaudeCode, root.path(), home), None);

    let plan = install_plan_for(
        ProviderId::ClaudeCode,
        root.path(),
        InstallScope::Project,
        None,
    )
    .unwrap();
    assert_eq!(plan.writes.len(), 1);
    assert_eq!(plan.writes[0].path, root.path().join(".mcp.json"));
    assert!(!plan.writes[0].merges_existing, "a fresh file is created");
    apply(&plan);

    assert_eq!(
        status_for(ProviderId::ClaudeCode, root.path(), home),
        Some(InstallScope::Project)
    );

    let value: Value =
        serde_json::from_str(&fs::read_to_string(root.path().join(".mcp.json")).unwrap()).unwrap();
    assert_eq!(
        value["mcpServers"][SERVER_NAME]["args"][0],
        Value::String(crate::mcp::argv::SUBCOMMAND.to_string())
    );
}

/// The gate bakes its exe path into a single command *string* and quotes it by
/// hand. An MCP entry has a `command` field and a separate `args` array, so no
/// quoting is ever needed — and a path containing a space or a quote can never
/// be re-split. Pinned so nobody "simplifies" it into one string.
#[test]
fn the_exe_path_is_a_field_and_never_quoted_into_an_args_string() {
    let root = tempdir().unwrap();
    let plan = install_plan_for(
        ProviderId::ClaudeCode,
        root.path(),
        InstallScope::Project,
        None,
    )
    .unwrap();
    let value: Value = serde_json::from_str(&plan.writes[0].content).unwrap();
    let entry = &value["mcpServers"][SERVER_NAME];

    let command = entry["command"].as_str().expect("a command string");
    assert!(
        !command.starts_with('"'),
        "the exe is not quoted: {command}"
    );
    assert!(!command.ends_with('"'), "the exe is not quoted: {command}");

    for arg in entry["args"].as_array().expect("an args array") {
        let arg = arg.as_str().expect("string argument");
        assert!(!arg.contains('"'), "no argument carries a quote: {arg:?}");
        assert!(
            !arg.contains(command),
            "the exe path is never spliced into an argument: {arg:?}"
        );
    }
}

/// `--workspace` scopes the listing only. A project install names its root; a
/// user install must not, or it would list one repository's connections
/// forever.
#[test]
fn a_user_scope_entry_names_no_workspace() {
    let root = tempdir().unwrap();
    let home = tempdir().unwrap();
    let claude_home = home.path().join(".claude");
    fs::create_dir_all(&claude_home).unwrap();

    let plan = install_plan_for(
        ProviderId::ClaudeCode,
        root.path(),
        InstallScope::User,
        Some(&claude_home),
    )
    .unwrap();
    assert_eq!(plan.writes[0].path, home.path().join(".claude.json"));

    let value: Value = serde_json::from_str(&plan.writes[0].content).unwrap();
    let args = value["mcpServers"][SERVER_NAME]["args"]
        .as_array()
        .expect("args");
    assert!(
        !args.iter().any(|a| a == "--workspace"),
        "a user-scope entry is unscoped: {args:?}"
    );

    let project = install_plan_for(
        ProviderId::ClaudeCode,
        root.path(),
        InstallScope::Project,
        None,
    )
    .unwrap();
    let project_value: Value = serde_json::from_str(&project.writes[0].content).unwrap();
    let project_args = project_value["mcpServers"][SERVER_NAME]["args"]
        .as_array()
        .expect("args");
    assert!(project_args.iter().any(|a| a == "--workspace"));
}

#[test]
fn a_codex_install_writes_config_toml_and_reads_back_as_installed() {
    let root = tempdir().unwrap();
    let home = tempdir().unwrap();
    fs::write(
        home.path().join("config.toml"),
        "model = \"gpt-5\"\n\n[projects.'c:\\work\\x']\ntrust_level = \"trusted\"\n",
    )
    .unwrap();

    let plan = install_plan_for(
        ProviderId::Codex,
        root.path(),
        InstallScope::User,
        Some(home.path()),
    )
    .unwrap();
    assert_eq!(plan.writes[0].path, home.path().join("config.toml"));
    assert!(plan.writes[0].merges_existing);
    apply(&plan);

    assert_eq!(
        status_for(ProviderId::Codex, root.path(), Some(home.path())),
        Some(InstallScope::User)
    );
    let text = fs::read_to_string(home.path().join("config.toml")).unwrap();
    assert!(text.contains("[projects.'c:\\work\\x']"), "{text}");
}

#[test]
fn uninstalling_a_file_that_holds_no_entry_of_ours_produces_no_write() {
    let root = tempdir().unwrap();
    let home = isolated_home(&root);
    let home = Some(home.as_path());

    let plan = uninstall_plan_for(
        ProviderId::ClaudeCode,
        root.path(),
        InstallScope::Project,
        None,
    )
    .unwrap();
    assert!(plan.writes.is_empty(), "nothing to remove");

    apply(
        &install_plan_for(
            ProviderId::ClaudeCode,
            root.path(),
            InstallScope::Project,
            None,
        )
        .unwrap(),
    );
    let plan = uninstall_plan_for(
        ProviderId::ClaudeCode,
        root.path(),
        InstallScope::Project,
        None,
    )
    .unwrap();
    assert_eq!(plan.writes.len(), 1);
    apply(&plan);
    assert_eq!(status_for(ProviderId::ClaudeCode, root.path(), home), None);
}

/// The sentence that is this feature's real security boundary. It must appear
/// wherever the user is asked to install, in every scope and for every
/// provider, and must not be softened.
#[test]
fn every_scope_states_the_read_access_boundary() {
    let cases = [
        (ProviderId::ClaudeCode, InstallScope::Project),
        (ProviderId::ClaudeCode, InstallScope::User),
        (ProviderId::Codex, InstallScope::User),
    ];
    for (provider, scope) in cases {
        let caveats = caveats_for(provider, scope);
        assert!(
            caveats.iter().any(|c| c == READ_ACCESS_NOTE),
            "{provider:?}/{scope:?} does not state the boundary verbatim"
        );
    }
    assert!(READ_ACCESS_NOTE.contains("credentials the database itself stores"));
    assert!(READ_ACCESS_NOTE.contains(
        "Expose only connections whose login you would give a colleague read access to."
    ));
}

/// Correction 3: a project `.mcp.json` is correct-but-inert until the user
/// accepts Claude Code's approval prompt. Stating it is the difference between
/// "installed and waiting for you" and "installed and mysteriously silent".
#[test]
fn the_claude_project_scope_states_the_approval_prompt_and_the_shared_exe_path() {
    let caveats = caveats_for(ProviderId::ClaudeCode, InstallScope::Project);
    let joined = caveats.join(" ");

    assert!(
        joined.contains("approve") || joined.contains("approval"),
        "{joined}"
    );
    assert!(joined.contains(".mcp.json"), "{joined}");

    // The exe-path hazard must be stated — a teammate without this build at the
    // same path gets a broken install — but stated *for this feature*. The gate's
    // own constant says "The hook command … will silently get no gate", which is
    // a true sentence about a different thing, so it must NOT appear here.
    assert!(
        joined.contains("full path to this copy of code-basics"),
        "the exe-path hazard has to be stated: {joined}"
    );
    assert!(
        !joined.contains("no gate") && !joined.contains("hook"),
        "an MCP caveat must not describe a hook or a gate: {joined}"
    );
    assert!(
        !caveats
            .iter()
            .any(|c| c == crate::qgate::install::SHARED_EXE_PATH_NOTE),
        "the gate's wording names the wrong feature and must not be reused verbatim: {joined}"
    );
}

/// `~/.claude.json` is rewritten continuously by a running Claude Code, so the
/// user is told to close it — and the note about a *repository* being trusted
/// belongs to `.codex/`, not to the global config this writes.
#[test]
fn the_user_scope_warns_about_the_live_file_and_codex_borrows_no_trust_caveat() {
    let claude = caveats_for(ProviderId::ClaudeCode, InstallScope::User).join(" ");
    assert!(claude.contains("Close Claude Code"), "{claude}");

    let codex = caveats_for(ProviderId::Codex, InstallScope::User).join(" ");
    assert!(
        !codex.to_lowercase().contains("trust"),
        "the .codex/ trust caveat gates a repository; this writes the global config: {codex}"
    );
}

/// Correction 1 held as an invariant rather than an assumption. If a future
/// Claude Code moves `mcpServers` into `.claude/settings.json`, this fails and
/// points whoever moved it at `setup.rs`'s chaining logic — because two
/// independent plans computing full file contents for one path make the second
/// clobber the first.
#[test]
fn the_setup_plan_writes_no_path_the_mcp_installer_also_writes() {
    let root = tempdir().unwrap();
    let home = tempdir().unwrap();
    let claude_home = home.path().join(".claude");
    fs::create_dir_all(&claude_home).unwrap();
    let codex_home = home.path().join(".codex");
    fs::create_dir_all(&codex_home).unwrap();

    for scope in [InstallScope::Project, InstallScope::User] {
        let providers: Vec<Box<dyn Provider>> =
            vec![Box::new(ClaudeCode::with_home(claude_home.clone()))];
        let setup = crate::setup::setup_plan(root.path(), scope, &providers, Some(&claude_home))
            .expect("a setup plan");
        let setup_paths: Vec<_> = setup.writes.iter().map(|w| w.path.clone()).collect();
        assert!(!setup_paths.is_empty(), "the setup plan writes something");

        let mut mcp_paths = Vec::new();
        for provider in [ProviderId::ClaudeCode, ProviderId::Codex] {
            let provider_home = match provider {
                ProviderId::Codex => codex_home.clone(),
                _ => claude_home.clone(),
            };
            if let Ok(plan) = install_plan_for(provider, root.path(), scope, Some(&provider_home)) {
                mcp_paths.extend(plan.writes.into_iter().map(|w| w.path));
            }
        }
        assert!(!mcp_paths.is_empty(), "the MCP installer writes something");

        for path in &mcp_paths {
            assert!(
                !setup_paths.contains(path),
                "{} is written by BOTH setup_plan and the MCP installer. \
                 Two plans computing full file contents for one path make the second \
                 clobber the first — chain them in setup.rs as the quality gate is chained.",
                path.display()
            );
        }
    }
}

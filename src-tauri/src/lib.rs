//! The Tauri shell.
//!
//! Deliberately thin: every decision lives in `cb-core`, which has no Tauri
//! dependency and is unit tested without a windowing system. What remains here
//! is state management, dispatching a configuration to the right adapter, and
//! bridging streamed process output onto an IPC channel.

mod invocation;
mod state;

mod commands {
    pub mod changelists;
    pub mod files;
    pub mod git;
    pub mod intents;
    pub mod run;
    pub mod secrets;
    pub mod workspace;
}

mod recorder;

use state::AppState;

/// Open the workspace named on the command line, so `code-basics .` works the
/// way every other developer tool does.
///
/// A bad path is not fatal: the app still starts and shows its welcome screen,
/// which is more useful than refusing to launch.
fn workspace_from_args(state: &AppState) {
    let Some(arg) = std::env::args().nth(1) else {
        return;
    };
    let path = std::path::PathBuf::from(arg);
    if !path.is_dir() {
        eprintln!("code-basics: {} is not a directory", path.display());
        return;
    }

    match cb_core::workspace::scan(&path) {
        Ok(mut workspace) => {
            if let Ok(saved) = cb_core::config::load(&workspace.root) {
                cb_core::config::apply(&mut workspace, saved);
            }
            let _ = state.set_workspace(workspace);
        }
        Err(e) => eprintln!("code-basics: could not open {}: {e:#}", path.display()),
    }
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    // Invoked by an agent's hook rather than by a user: record and exit
    // without ever creating a window.
    if recorder::is_record_invocation() {
        recorder::run();
        return;
    }

    let state = AppState::default();
    workspace_from_args(&state);

    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_opener::init())
        .manage(state)
        .invoke_handler(tauri::generate_handler![
            commands::workspace::open_workspace,
            commands::workspace::current_workspace,
            commands::workspace::rescan_workspace,
            commands::workspace::save_config,
            commands::workspace::delete_config,
            commands::workspace::preview_rider_import,
            commands::workspace::apply_rider_import,
            commands::workspace::launch_profiles,
            commands::workspace::set_favorite,
            commands::workspace::set_config_order,
            commands::files::fs_list_dir,
            commands::files::fs_read_file,
            commands::files::fs_write_file,
            commands::secrets::read_project_secrets,
            commands::secrets::write_project_secrets,
            commands::run::start_run,
            commands::run::build_project,
            commands::run::cancel_run,
            commands::run::running_ids,
            commands::run::run_tests,
            commands::run::last_test_run,
            commands::git::git_status,
            commands::git::git_file_diff,
            commands::git::git_file_contents,
            commands::git::git_write_file,
            commands::git::git_stage_file,
            commands::git::git_unstage_file,
            commands::git::git_stage_lines,
            commands::git::git_unstage_lines,
            commands::git::git_revert_lines,
            commands::git::git_discard_file,
            commands::git::git_commit,
            commands::git::git_branches,
            commands::git::git_create_branch,
            commands::git::git_checkout_branch,
            commands::git::git_checkout_remote_branch,
            commands::git::git_delete_branch,
            commands::git::git_merge_branch,
            commands::git::git_abort_merge,
            commands::changelists::git_changelists,
            commands::changelists::git_create_changelist,
            commands::changelists::git_delete_changelist,
            commands::changelists::git_rename_changelist,
            commands::changelists::git_assign_to_changelist,
            commands::git::git_history,
            commands::git::git_commit_diff,
            commands::git::git_stash_save,
            commands::git::git_stash_pop,
            commands::git::git_network,
            commands::intents::intent_groups,
            commands::intents::stage_intent_group,
            commands::intents::revert_intent_group,
            commands::intents::intent_capture_status,
            commands::intents::intent_install_plan,
            commands::intents::enable_intent_capture,
            commands::intents::import_intent_history,
            commands::intents::clear_intent_history,
        ])
        .run(tauri::generate_context!())
        .expect("failed to start code-basics");
}

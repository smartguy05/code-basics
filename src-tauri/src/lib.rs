//! The Tauri shell.
//!
//! Deliberately thin: every decision lives in `cb-core`, which has no Tauri
//! dependency and is unit tested without a windowing system. What remains here
//! is state management, dispatching a configuration to the right adapter, and
//! bridging streamed process output onto an IPC channel.

mod invocation;
mod state;

mod commands {
    pub mod git;
    pub mod run;
    pub mod workspace;
}

use state::AppState;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .manage(AppState::default())
        .invoke_handler(tauri::generate_handler![
            commands::workspace::open_workspace,
            commands::workspace::current_workspace,
            commands::workspace::rescan_workspace,
            commands::workspace::save_config,
            commands::workspace::delete_config,
            commands::workspace::preview_rider_import,
            commands::workspace::apply_rider_import,
            commands::run::start_run,
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
            commands::git::git_delete_branch,
            commands::git::git_history,
            commands::git::git_commit_diff,
            commands::git::git_stash_save,
            commands::git::git_stash_pop,
            commands::git::git_network,
        ])
        .run(tauri::generate_context!())
        .expect("failed to start code-basics");
}

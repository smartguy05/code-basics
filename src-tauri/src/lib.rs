//! The Tauri shell.
//!
//! Deliberately thin: every decision lives in `cb-core`, which has no Tauri
//! dependency and is unit tested without a windowing system. What remains here
//! is state management, the `#[tauri::command]` surface, and bridging streamed
//! process output onto an IPC channel.

mod state;

mod commands {
    pub mod architecture;
    pub mod behavioral;
    pub mod changelists;
    pub mod enhancements;
    pub mod erosion;
    pub mod files;
    pub mod git;
    pub mod inspect;
    pub mod intents;
    pub mod lsp;
    pub mod review;
    pub mod run;
    pub mod secrets;
    pub mod symbols;
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

    match cb_core::workspace::workspace_from_dir(&path) {
        Ok(workspace) => {
            let _ = state.set_workspace(workspace);
        }
        Err(e) => eprintln!("code-basics: {e}"),
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
        // A workspace named on the command line is open before there is an app
        // to hang a background thread off, so its index — and its language-server
        // session — are started here instead of in `open_workspace`. Without
        // this, `code-basics .` would leave the palette empty until the user
        // happened to trigger a rescan, which is the one entry point where
        // nothing else would ever kick the build off.
        .setup(|app| {
            use tauri::Manager;
            if let Ok(workspace) = app.state::<AppState>().workspace() {
                commands::symbols::spawn_build(
                    app.handle().clone(),
                    workspace,
                    commands::symbols::Rebuild::Cached,
                );
                // Spawned rather than called: `session::start` needs a Tokio
                // runtime for its actor, and `setup` is synchronous.
                commands::lsp::spawn_session(app.handle().clone());
            }
            Ok(())
        })
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
            commands::enhancements::list_enhancements,
            commands::enhancements::add_enhancement,
            commands::enhancements::remove_enhancement,
            commands::enhancements::list_prompts,
            commands::enhancements::agent_runs,
            commands::enhancements::mark_agent_run,
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
            commands::review::start_review,
            commands::review::cancel_review,
            commands::review::review_agents,
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
            commands::git::git_commit_file_contents,
            commands::git::git_commit_file_why,
            commands::git::git_stash_save,
            commands::git::git_stash_list,
            commands::git::git_stash_pop,
            commands::git::git_stash_apply,
            commands::git::git_stash_drop,
            commands::git::git_stash_clear,
            commands::git::git_network,
            commands::intents::intent_groups,
            commands::intents::stage_intent_group,
            commands::intents::revert_intent_group,
            commands::intents::reject_intent_group,
            commands::intents::intent_capture_status,
            commands::intents::intent_install_plan,
            commands::intents::enable_intent_capture,
            commands::intents::import_intent_history,
            commands::intents::clear_intent_history,
            commands::behavioral::behavioral_diff,
            commands::behavioral::behavioral_clear,
            commands::erosion::erosion_scan,
            commands::inspect::inspect_status,
            commands::inspect::inspect_attachable,
            commands::inspect::inspect_run_dump,
            commands::inspect::inspect_capture,
            commands::inspect::inspect_last,
            commands::inspect::inspect_clear,
            commands::architecture::arch_project_graph,
            commands::architecture::arch_render_graph,
            commands::architecture::arch_component_graph,
            commands::architecture::arch_render_component_graph,
            commands::architecture::arch_list_diagrams,
            commands::architecture::arch_read_diagram,
            commands::architecture::arch_write_diagram,
            commands::architecture::arch_validate,
            commands::symbols::search_everywhere,
            commands::symbols::symbol_index_status,
            commands::symbols::rebuild_symbol_index,
            commands::lsp::lsp_status,
            commands::lsp::lsp_open_document,
            commands::lsp::lsp_change_document,
            commands::lsp::lsp_close_document,
            commands::lsp::lsp_find_usages,
            commands::lsp::lsp_goto_definition,
            commands::lsp::lsp_declaration_anchors,
        ])
        .run(tauri::generate_context!())
        .expect("failed to start code-basics");
}

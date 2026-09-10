//! The Tauri shell.
//!
//! Deliberately thin: every decision lives in `cb-core`, which has no Tauri
//! dependency and is unit tested without a windowing system. What remains here
//! is state management, the `#[tauri::command]` surface, and bridging streamed
//! process output onto an IPC channel.

// `pub` rather than private, unlike `state`: the browser host is the surface the
// Phase 4c MCP server — a fourth self-dispatch mode of this same binary, beside
// `record-intent`, `quality-gate` and `mcp-sql` — reads the panel through. The
// consent decision it exists for (`browser::shared::state_for` feeding
// `cb_core::browser::consent::decide`) has no in-window caller by design: the
// user driving their own panel needs no consent from themselves.
pub mod browser;
mod roslyn;
mod state;

mod commands {
    pub mod about;
    pub mod architecture;
    pub mod behavioral;
    pub mod browser;
    pub mod browser_mcp;
    pub mod changelists;
    pub mod debug;
    pub mod enhancements;
    pub mod erosion;
    pub mod features;
    pub mod files;
    pub mod git;
    pub mod inspect;
    pub mod intents;
    pub mod launcher;
    pub mod lsp;
    pub mod mcp;
    pub mod notes;
    pub mod qgate;
    pub mod review;
    pub mod roslyn_mcp;
    pub mod rules;
    pub mod run;
    pub mod running;
    pub mod secrets;
    pub mod setup;
    pub mod sql;
    pub mod symbols;
    pub mod tasks;
    pub mod terminal;
    pub mod workspace;
}

mod mcp_browser;
mod mcp_roslyn;
mod mcp_sql;
mod mcp_tasks;
mod qgate_run;
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

    // The quality-gate Stop hook likewise re-invokes this binary rather than a
    // shipped script; it runs its checks and exits without creating a window.
    if qgate_run::is_quality_gate_invocation() {
        qgate_run::run();
        return;
    }

    // The third self-dispatch mode: an MCP client (Claude Code, Codex) starts
    // this executable as a stdio server rather than as an application. Like the
    // other two it must never create a window — and unlike them it must never
    // write to stdout either, which is the transport.
    if mcp_sql::is_mcp_sql_invocation() {
        mcp_sql::run();
    }

    // The fifth mode, and the second MCP server out of this one executable:
    // the browser tools. Unlike `mcp-sql` it answers nothing itself - the page
    // it reports on lives in a window in another process - so it forwards over
    // a named pipe and hands back that application's own words. Same two rules
    // as the SQL server: never a window, and never a byte on stdout that is not
    // an MCP frame.
    if mcp_browser::is_mcp_browser_invocation() {
        mcp_browser::run();
    }

    // The fifth self-dispatch mode, and the third MCP server out of this one
    // executable: the Tasks server. Unlike `mcp-sql` it is write-capable by
    // design — creating and updating tasks is the point — and unlike the
    // browser server it answers directly out of the per-workspace task store
    // rather than forwarding to a window. Same two rules as the others: never a
    // window, and never a byte on stdout that is not an MCP frame.
    if mcp_tasks::is_mcp_tasks_invocation() {
        mcp_tasks::run();
    }

    // The sixth self-dispatch mode, and the fourth MCP server out of this one
    // executable: the Roslyn/LSP server. Like the browser server it answers
    // nothing itself — the warm semantic model lives in a window in another
    // process — so it forwards over a named pipe and hands back that
    // application's own words. The one difference is the boundary: it resolves
    // the `--workspace` it was installed for rather than the active window. Same
    // two rules as the others: never a window, and never a byte on stdout that is
    // not an MCP frame.
    if mcp_roslyn::is_mcp_roslyn_invocation() {
        mcp_roslyn::run();
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
            // Bundle icons decorate the executable and installer; Windows does
            // not reliably copy that resource onto a WebView window. Apply the
            // same icon explicitly so the installed app also owns its taskbar
            // and title-bar identity.
            if let (Some(window), Some(icon)) =
                (app.get_webview_window("main"), app.default_window_icon())
            {
                window.set_icon(icon.clone())?;
            }
            // Detect crash-orphans once, before anything is spawned: reload the
            // previous session's recorded processes, keep the ones still alive
            // whose identity matches, and rewrite the file to the survivors.
            app.state::<AppState>()
                .running
                .load_orphans(cb_core::running::probe::probe);
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
            // Publish this application in the browser instance registry with no
            // listener: an absent entry means *no application is running*, and
            // an entry carrying `browserFeature` and no listener means *running,
            // and no browser panel open*. Those are different answers with
            // different fixes, and a client has to be able to tell them apart
            // without connecting to anything - which it can only do if the
            // entry exists before any panel does.
            browser::registry::announce_startup();
            // Open the process-global Roslyn control pipe and publish this
            // application in the roslyn instance registry with the workspaces it
            // already has open. Spawned rather than called: `pipe::start` uses
            // `tokio::spawn` for its accept loop and so must run inside the async
            // runtime, which `setup` is not. Unlike the browser pipe (tied to a
            // panel), this one lives for the whole process.
            {
                let handle = app.handle().clone();
                tauri::async_runtime::spawn(async move {
                    let state = handle.state::<AppState>();
                    roslyn::start_listener(state.inner(), &handle);
                });
            }
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::workspace::open_workspace,
            commands::workspace::current_workspace,
            commands::workspace::list_open_workspaces,
            commands::workspace::set_active_workspace,
            commands::workspace::close_workspace,
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
            commands::enhancements::save_note_as_instruction,
            commands::launcher::list_launchables,
            commands::launcher::launch_command,
            commands::launcher::stop_command,
            commands::launcher::save_launchable,
            commands::launcher::delete_launchable,
            commands::features::list_features,
            commands::features::set_feature,
            commands::notes::read_notes,
            commands::notes::write_notes,
            commands::tasks::read_tasks,
            commands::tasks::create_task,
            commands::tasks::update_task,
            commands::tasks::assign_task,
            commands::tasks::complete_task,
            commands::tasks::delete_task,
            commands::tasks::tasks_mcp_status,
            commands::tasks::tasks_mcp_install_plan,
            commands::tasks::install_tasks_mcp_server,
            commands::tasks::tasks_mcp_uninstall_plan,
            commands::tasks::uninstall_tasks_mcp_server,
            commands::about::about_info,
            commands::files::fs_list_dir,
            commands::files::fs_read_file,
            commands::files::fs_write_file,
            commands::files::fs_create_file,
            commands::files::fs_create_dir,
            commands::files::fs_rename,
            commands::files::fs_delete,
            commands::secrets::read_project_secrets,
            commands::secrets::write_project_secrets,
            commands::run::start_run,
            commands::run::build_project,
            commands::run::cancel_run,
            commands::run::running_ids,
            commands::debug::start_debug,
            commands::debug::stop_debug,
            commands::debug::debug_ids,
            commands::run::run_tests,
            commands::run::last_test_run,
            commands::run::coverage_of_change,
            commands::review::start_review,
            commands::review::cancel_review,
            commands::review::review_agents,
            commands::review::agent_interactive_command,
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
            commands::git::git_add_worktree,
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
            commands::git::git_stash_paths,
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
            commands::intents::intent_uninstall_plan,
            commands::intents::disable_intent_capture,
            commands::intents::import_intent_history,
            commands::intents::intent_prune_preview,
            commands::intents::prune_intent_history,
            commands::intents::clear_intent_history,
            commands::intents::set_card_intent,
            commands::intents::clear_card_intent,
            commands::intents::move_card_edits,
            commands::mcp::mcp_server_status,
            commands::mcp::mcp_server_install_plan,
            commands::mcp::install_mcp_server,
            commands::mcp::mcp_server_uninstall_plan,
            commands::mcp::uninstall_mcp_server,
            commands::browser_mcp::browser_mcp_status,
            commands::browser_mcp::browser_mcp_install_plan,
            commands::browser_mcp::install_browser_mcp,
            commands::browser_mcp::browser_mcp_uninstall_plan,
            commands::browser_mcp::uninstall_browser_mcp,
            commands::roslyn_mcp::roslyn_mcp_server_status,
            commands::roslyn_mcp::roslyn_mcp_server_install_plan,
            commands::roslyn_mcp::install_roslyn_mcp_server,
            commands::roslyn_mcp::roslyn_mcp_server_uninstall_plan,
            commands::roslyn_mcp::uninstall_roslyn_mcp_server,
            commands::qgate::quality_gate_status,
            commands::qgate::quality_gate_install_plan,
            commands::qgate::install_quality_gate,
            commands::qgate::quality_gate_uninstall_plan,
            commands::qgate::uninstall_quality_gate,
            commands::setup::setup_install_plan,
            commands::setup::install_setup,
            commands::behavioral::behavioral_diff,
            commands::behavioral::behavioral_clear,
            commands::erosion::erosion_scan,
            commands::rules::list_rules,
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
            commands::lsp::lsp_restart,
            commands::lsp::lsp_open_document,
            commands::lsp::lsp_change_document,
            commands::lsp::lsp_close_document,
            commands::lsp::lsp_find_usages,
            commands::lsp::lsp_goto_definition,
            commands::lsp::lsp_declaration_anchors,
            commands::lsp::lsp_prepare_rename,
            commands::lsp::lsp_rename,
            commands::browser::browser_open,
            commands::browser::browser_close,
            commands::browser::browser_set_bounds,
            commands::browser::browser_set_visible,
            commands::browser::browser_navigate,
            commands::browser::browser_back,
            commands::browser::browser_forward,
            commands::browser::browser_reload,
            commands::browser::browser_state,
            commands::browser::browser_console,
            commands::browser::browser_network,
            commands::browser::browser_page_text,
            commands::browser::browser_set_automation_consent,
            commands::terminal::terminal_open,
            commands::terminal::terminal_write,
            commands::terminal::terminal_resize,
            commands::terminal::terminal_close,
            commands::terminal::terminal_list,
            commands::terminal::terminal_set_label,
            commands::terminal::list_shells,
            commands::running::list_running,
            commands::running::kill_running,
            commands::sql::sql_list_connections,
            commands::sql::sql_discover,
            commands::sql::sql_save_connection,
            commands::sql::sql_delete_connection,
            commands::sql::sql_rename_connection,
            commands::sql::sql_set_allow_writes,
            commands::sql::sql_set_expose_to_agents,
            commands::sql::sql_test_connection,
            commands::sql::sql_test_connection_string,
            commands::sql::sql_list_objects,
            commands::sql::sql_list_columns,
            commands::sql::sql_execute,
            commands::sql::sql_cancel,
        ])
        .build(tauri::generate_context!())
        .expect("failed to start code-basics")
        .run(|app_handle, event| {
            // When the app is quitting (last window closed, or an explicit
            // exit), tree-kill everything it started so nothing is orphaned.
            // Every spawning handle — per-workspace supervisors, the global
            // supervisor and the PTY manager — records into the one shared
            // registry, so its live set is the complete set of live pids. The
            // one exception is the language servers: they are spawned straight
            // through `lsp::transport` and never touch the running store, so they
            // are reaped separately just below. A true crash (panic = abort)
            // cannot run any of this; that case stays covered by the next-launch
            // orphan detection in `.setup`.
            if let tauri::RunEvent::ExitRequested { .. } | tauri::RunEvent::Exit = event {
                use tauri::Manager;
                let state = app_handle.state::<AppState>();
                for record in state.running.live() {
                    cb_core::process::kill_tree(record.pid);
                }
                // The LSP trees (Roslyn + its `BuildHost` children, node, …). The
                // handle's own async teardown cannot run here — the runtime is
                // being abandoned — so the pids are read synchronously off the
                // shared snapshot and killed the same way the running set is.
                for handle in state.all_lsp_handles() {
                    for pid in handle.server_pids() {
                        cb_core::process::kill_tree(pid);
                    }
                }
                // And take this application out of the browser instance
                // registry. Not load-bearing - liveness is re-probed, so a
                // leftover entry for a dead pid reads as `NoneRunning` - but a
                // stale entry makes a second application's entry look like an
                // ambiguity, which is a refusal the user would have to resolve
                // for no reason.
                let _ = browser::registry::withdraw(std::process::id());
                // And out of the roslyn instance registry, for the same reason.
                let _ = roslyn::registry::withdraw(std::process::id());
            }
        });
}

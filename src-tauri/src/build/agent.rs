//! Answering an agent's Build tool call: check the feature, run or read the build,
//! phrase the result.
//!
//! This is the application's half of [`cb_core::build::mcp::wire`]. Unlike the
//! Roslyn host — which needs a live language server and so cannot be reached from a
//! test — the **read** half here is a pure function over plain data ([`read_answer`],
//! tested in `agent_tests.rs`), and only the **build** half ([`build_solution`])
//! touches the world.
//!
//! # What is decided here, and what is not
//!
//! * **whether the feature is on** is [`answer`]'s `enabled` argument, read fresh
//!   by the pipe host per call — feature-off is [`BuildRefusal::Disabled`], and the
//!   frontend never gates this because a build must not run while the user has the
//!   feature switched off;
//! * **which report a read tool phrases** is the cached
//!   [`cb_core::build::BuildReport`] for the request's `--workspace`, turned into
//!   the distinct [`BuildStatus::NeverBuilt`] answer when there is none (via
//!   [`crate::state::build_report_or_never_built`]) rather than an empty success;
//! * **what every answer says** is [`cb_core::build::mcp::render`].
//!
//! # Why the feature is re-checked here, per call
//!
//! Exactly the editor-context precedent: the frontend cannot gate a build the way
//! it gates its own pushes, so the pipe host reads
//! [`cb_core::features::FeatureId::BuildMcp`] every call and hands the answer its
//! value — see [`crate::build::pipe`].

use std::path::Path;

use cb_core::adapters::dotnet::{self, BuildAction};
use cb_core::build::mcp::answer::BuildRefusal;
use cb_core::build::mcp::render;
use cb_core::build::mcp::tools::BuildToolCall;
use cb_core::build::mcp::wire::ToolAnswer;
use cb_core::build::{parse_report, BuildReport, BuildStatus};
use tauri::{AppHandle, Manager};

use crate::state::{build_report_or_never_built, AppState};

/// The reserved cache key the Build MCP server records its own builds under.
///
/// `get_*` read this key and `build_solution` writes it, so the workspace-level
/// tools — which take no configuration argument — agree on one report without a
/// second store. It is deliberately its own key (the editor's Build button does
/// not record here), so "the most recent build" this server reports is
/// unambiguously the most recent build *it* ran.
pub const WORKSPACE_BUILD_KEY: &str = "mcp-build:solution";

/// Which read-only tool is being answered. `build_solution` is not here — it runs a
/// build and so cannot be part of the pure decision.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ReadTool {
    Errors,
    Warnings,
    Status,
}

/// The refusal for a [`BuildRefusal`], as a [`ToolAnswer`].
fn refusal(reason: BuildRefusal) -> ToolAnswer {
    ToolAnswer::refused(reason.code(), reason.sentence())
}

/// Phrase a read-only build tool call from the cached report — **pure and tested**.
///
/// `enabled` is the feature flag; `cached` is whatever `last_build` held for the
/// request's workspace under [`WORKSPACE_BUILD_KEY`]. The abstain rule lives in
/// [`build_report_or_never_built`] (absent ⇒ [`BuildStatus::NeverBuilt`], never an
/// empty clean success) and in [`render`]; this function only routes.
fn read_answer(enabled: bool, cached: Option<BuildReport>, tool: ReadTool) -> ToolAnswer {
    if !enabled {
        return refusal(BuildRefusal::Disabled);
    }
    let report = build_report_or_never_built(cached);
    match tool {
        ReadTool::Errors => render::errors(&report),
        ReadTool::Warnings => render::warnings(&report),
        ReadTool::Status => render::status(&report),
    }
}

/// Run one Build tool call against the application and phrase its answer.
///
/// `enabled` is read fresh by the pipe host. `workspace` is the request's
/// `--workspace` root — the boundary the shim was installed with — resolved here to
/// the slot that owns the build, exactly as the Roslyn host resolves a session.
pub async fn answer(
    app: &AppHandle,
    workspace: &str,
    enabled: bool,
    call: BuildToolCall,
) -> ToolAnswer {
    if !enabled {
        return refusal(BuildRefusal::Disabled);
    }
    match call {
        BuildToolCall::BuildSolution { configuration } => {
            build_solution(app, workspace, configuration).await
        }
        BuildToolCall::GetErrors => read_answer(true, cached(app, workspace), ReadTool::Errors),
        BuildToolCall::GetWarnings => read_answer(true, cached(app, workspace), ReadTool::Warnings),
        BuildToolCall::GetBuildStatus => {
            read_answer(true, cached(app, workspace), ReadTool::Status)
        }
    }
}

/// The cached report for `workspace` under the reserved key, or [`None`].
fn cached(app: &AppHandle, workspace: &str) -> Option<BuildReport> {
    let state = app.state::<AppState>();
    state.previous_build_for(Path::new(workspace), WORKSPACE_BUILD_KEY)
}

/// Run `dotnet build` for the workspace, parse the file-logger artifacts, cache the
/// report and phrase the outcome.
///
/// A single `dotnet build` at the workspace root (no project argument) — the
/// literal "build the solution", the same command a developer types in the
/// repository root, which resolves the `.sln` or single project there. Its two
/// MSBuild file-logger artifacts (errors-only and warnings-only, under
/// `.code-basics/build/`) are read back through [`dotnet::build_log_paths`] and
/// parsed by [`parse_report`], which owns the status decision.
///
/// The distinct non-parse states are supplied here, never guessed: a spawn failure
/// or an undirectory-able build dir is [`BuildStatus::CouldNotStart`], which
/// [`render::build_outcome`] refuses rather than reporting as an empty success.
async fn build_solution(
    app: &AppHandle,
    workspace: &str,
    configuration: Option<String>,
) -> ToolAnswer {
    // Resolve the slot, then drop the `State` guard before any await — the slot is
    // an owned `Arc`, so nothing here holds a lock across the build.
    let (slot, root) = {
        let state = app.state::<AppState>();
        let Some(slot) = state.slot_for_root(Path::new(workspace)) else {
            // The shim chose this application because it published this workspace,
            // so reaching here means it closed between that choice and now. A
            // generic refusal that leaks no path.
            return ToolAnswer::refused(
                "workspace_not_open",
                "This workspace is no longer open in the application, so its build could not be \
                 run. Reopen it in code-basics and try again.",
            );
        };
        let root = slot.root.clone();
        (slot, root)
    };

    // The MSBuild file logger does not create its own directory; create it first,
    // exactly as the editor's `build_project` command does.
    let build_dir = cb_core::config::build_dir(&root);
    if std::fs::create_dir_all(&build_dir).is_err() {
        return record_and_render(
            app,
            &root,
            BuildReport::of_status(BuildStatus::CouldNotStart),
        );
    }

    let mut config = cb_core::model::RunConfig::new(
        WORKSPACE_BUILD_KEY,
        "Build solution",
        cb_core::model::RunKind::App,
        "dotnet",
        cb_core::model::ConfigSource::Detected,
    );
    if let Some(configuration) = configuration.filter(|c| !c.trim().is_empty()) {
        config.build_configuration = Some(configuration);
    }
    let invocation = dotnet::build_action_invocation(&config, BuildAction::Build, &root);

    let (tx, mut rx) = tokio::sync::mpsc::channel(512);
    // The console output is not shown to the agent, but the supervisor's output
    // pump blocks once its channel fills, so it must have a consumer or the build
    // never finishes. Drain and discard.
    tokio::spawn(async move { while rx.recv().await.is_some() {} });

    // Publish the in-progress state before awaiting the build, so a concurrent
    // `get_build_status` from another tool call reports `Building` — the sixth
    // distinct answer — rather than the previous report or `NeverBuilt`. The parsed
    // report below overwrites it on completion; if the build never returns, the
    // truthful last word to any reader is that it is still running.
    {
        let state = app.state::<AppState>();
        state.record_build(
            &root,
            WORKSPACE_BUILD_KEY,
            BuildReport::of_status(BuildStatus::Building),
        );
    }

    let outcome = slot
        .supervisor
        .run_tracked(
            WORKSPACE_BUILD_KEY,
            &invocation,
            tx,
            cb_core::running::RunMeta {
                root: root.display().to_string(),
                label: "Build solution (agent)".to_string(),
                kind: cb_core::running::RunKind::Build,
            },
        )
        .await;

    let report = match outcome {
        // The process could not be spawned at all — distinct from a build that ran
        // and failed. `render::build_outcome` refuses this rather than reporting it
        // as data.
        Err(_) => BuildReport::of_status(BuildStatus::CouldNotStart),
        Ok(exit) => {
            let (errors_path, warnings_path) = dotnet::build_log_paths(&root, WORKSPACE_BUILD_KEY);
            let errors = std::fs::read_to_string(&errors_path).unwrap_or_default();
            let warnings = std::fs::read_to_string(&warnings_path).unwrap_or_default();
            // A `None` exit code is a signalled or cancelled build; it is not a
            // success, so `exit_success` is false and any captured diagnostics
            // still surface.
            parse_report(&errors, &warnings, exit == Some(0))
        }
    };

    record_and_render(app, &root, report)
}

/// Cache a freshly produced report and phrase it as the `build_solution` outcome.
fn record_and_render(app: &AppHandle, root: &Path, report: BuildReport) -> ToolAnswer {
    let state = app.state::<AppState>();
    state.record_build(root, WORKSPACE_BUILD_KEY, report.clone());
    render::build_outcome(&report)
}

#[cfg(test)]
#[path = "agent_tests.rs"]
mod tests;

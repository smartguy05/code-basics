/** Typed wrappers over the Tauri command surface. */

import { Channel, invoke } from "@tauri-apps/api/core";
import type {
  AttachableList,
  Branch,
  BuildAction,
  Changelists,
  Commit,
  ComparisonMode,
  DirEntry,
  ElidedReason,
  FileContents,
  FileDiff,
  InspectGraph,
  InspectStatus,
  InspectTarget,
  InstallPlan,
  InstallScope,
  IntentGroup,
  LaunchProfile,
  MergeReport,
  NetworkKind,
  ProcessEvent,
  ProjectSecrets,
  ProviderId,
  ProviderStatus,
  RejectSummary,
  RiderImportPreview,
  RootSpec,
  RunConfig,
  RunDump,
  TestRunOutcome,
  WorkingStatus,
  Workspace,
} from "./types";

// ---------------------------------------------------------------------------
// Workspace
// ---------------------------------------------------------------------------

export const openWorkspace = (path: string) =>
  invoke<Workspace>("open_workspace", { path });

export const currentWorkspace = () =>
  invoke<Workspace | null>("current_workspace");

export const rescanWorkspace = () => invoke<Workspace>("rescan_workspace");

export const saveConfig = (config: RunConfig) =>
  invoke<Workspace>("save_config", { config });

export const deleteConfig = (id: string) =>
  invoke<Workspace>("delete_config", { id });

/**
 * Launch profiles a .NET project defines, including the hosting profiles
 * `dotnet run` cannot apply — those come back with `launchable: false`.
 */
export const launchProfiles = (project: string) =>
  invoke<LaunchProfile[]>("launch_profiles", { project });

export const setFavorite = (id: string, favorite: boolean) =>
  invoke<Workspace>("set_favorite", { id, favorite });

export const setConfigOrder = (order: string[]) =>
  invoke<Workspace>("set_config_order", { order });

/** `project` is the workspace-relative path from `RunConfig.project`. */
export const readProjectSecrets = (project: string) =>
  invoke<ProjectSecrets>("read_project_secrets", { project });

export const writeProjectSecrets = (project: string, content: string) =>
  invoke<ProjectSecrets>("write_project_secrets", { project, content });

export const previewRiderImport = () =>
  invoke<RiderImportPreview>("preview_rider_import");

export const applyRiderImport = (configs: RunConfig[]) =>
  invoke<Workspace>("apply_rider_import", { configs });

// ---------------------------------------------------------------------------
// Workspace files (directory tree and file editor)
// ---------------------------------------------------------------------------

/** List one directory of the workspace, filtered like the project scan. */
export const fsListDir = (path: string) =>
  invoke<DirEntry[]>("fs_list_dir", { path });

export const fsReadFile = (path: string) =>
  invoke<string>("fs_read_file", { path });

export const fsWriteFile = (path: string, content: string) =>
  invoke<void>("fs_write_file", { path, content });

// ---------------------------------------------------------------------------
// Running
// ---------------------------------------------------------------------------

/**
 * Start a configuration, streaming its output to `onEvent`.
 *
 * The returned promise resolves when the process exits, so callers that want
 * to keep the UI responsive should not await it before rendering.
 */
export function startRun(
  configId: string,
  onEvent: (event: ProcessEvent) => void,
  /** Environment variables layered over the config's own, for this run only. */
  env?: Record<string, string>,
): Promise<void> {
  const channel = new Channel<ProcessEvent>();
  channel.onmessage = onEvent;
  return invoke<void>("start_run", { configId, channel, env });
}

/** Build / rebuild / clean the project behind a .NET configuration. */
export function buildProject(
  configId: string,
  action: BuildAction,
  onEvent: (event: ProcessEvent) => void,
): Promise<void> {
  const channel = new Channel<ProcessEvent>();
  channel.onmessage = onEvent;
  return invoke<void>("build_project", { configId, action, channel });
}

export const cancelRun = (configId: string) =>
  invoke<boolean>("cancel_run", { configId });

export const runningIds = () => invoke<string[]>("running_ids");

export function runTests(
  configId: string,
  onlyFailed: boolean,
  onEvent: (event: ProcessEvent) => void,
): Promise<TestRunOutcome> {
  const channel = new Channel<ProcessEvent>();
  channel.onmessage = onEvent;
  return invoke<TestRunOutcome>("run_tests", { configId, onlyFailed, channel });
}

export const lastTestRun = (configId: string) =>
  invoke<TestRunOutcome | null>("last_test_run", { configId });

// ---------------------------------------------------------------------------
// Git
// ---------------------------------------------------------------------------

export const gitStatus = () => invoke<WorkingStatus>("git_status");

export const gitFileDiff = (path: string, mode: ComparisonMode) =>
  invoke<FileDiff>("git_file_diff", { path, mode });

export const gitFileContents = (path: string, mode: ComparisonMode) =>
  invoke<FileContents>("git_file_contents", { path, mode });

export const gitWriteFile = (path: string, content: string) =>
  invoke<void>("git_write_file", { path, content });

export const gitStageFile = (path: string) =>
  invoke<void>("git_stage_file", { path });

export const gitUnstageFile = (path: string) =>
  invoke<void>("git_unstage_file", { path });

export const gitStageLines = (path: string, lines: number[]) =>
  invoke<boolean>("git_stage_lines", { path, lines });

export const gitUnstageLines = (path: string, lines: number[]) =>
  invoke<boolean>("git_unstage_lines", { path, lines });

export const gitRevertLines = (
  path: string,
  mode: ComparisonMode,
  lines: number[],
) => invoke<boolean>("git_revert_lines", { path, mode, lines });

export const gitDiscardFile = (path: string) =>
  invoke<void>("git_discard_file", { path });

export const gitCommit = (message: string, amend: boolean) =>
  invoke<string>("git_commit", { message, amend });

export const gitBranches = () => invoke<Branch[]>("git_branches");

/** `from` names the revision to branch from; absent means HEAD. */
export const gitCreateBranch = (name: string, checkout: boolean, from?: string) =>
  invoke<void>("git_create_branch", { name, checkout, from });

export const gitCheckoutBranch = (name: string) =>
  invoke<void>("git_checkout_branch", { name });

/** Check out `origin/x` like `git switch x`: local tracking branch + switch. */
export const gitCheckoutRemoteBranch = (name: string) =>
  invoke<void>("git_checkout_remote_branch", { name });

export const gitDeleteBranch = (name: string) =>
  invoke<void>("git_delete_branch", { name });

/**
 * Merge a branch into the current one.
 *
 * Conflicts do not throw: the merge is left in progress and reported with
 * `outcome: "conflicted"`, to be resolved in the Changes tab or backed out
 * with `gitAbortMerge`.
 */
export const gitMergeBranch = (name: string) =>
  invoke<MergeReport>("git_merge_branch", { name });

/** Undo an in-progress merge, returning to the pre-merge commit. */
export const gitAbortMerge = () => invoke<void>("git_abort_merge");

// ---------------------------------------------------------------------------
// Change groups
//
// Local bookkeeping, not git state. Every mutation returns the full set so the
// Changes tab re-renders from one round trip.
// ---------------------------------------------------------------------------

export const gitChangelists = () => invoke<Changelists>("git_changelists");

export const gitCreateChangelist = (name: string) =>
  invoke<Changelists>("git_create_changelist", { name });

/** Delete a group; its files become ungrouped rather than disappearing. */
export const gitDeleteChangelist = (name: string) =>
  invoke<Changelists>("git_delete_changelist", { name });

export const gitRenameChangelist = (from: string, to: string) =>
  invoke<Changelists>("git_rename_changelist", { from, to });

/** Move files into a group, or out of every group when `group` is null. */
export const gitAssignToChangelist = (paths: string[], group: string | null) =>
  invoke<Changelists>("git_assign_to_changelist", { paths, group });

export const gitHistory = (limit: number) =>
  invoke<Commit[]>("git_history", { limit });

export const gitCommitDiff = (id: string) =>
  invoke<FileDiff[]>("git_commit_diff", { id });

export const gitStashSave = (message: string) =>
  invoke<void>("git_stash_save", { message });

export const gitStashPop = () => invoke<void>("git_stash_pop");

export function gitNetwork(
  kind: NetworkKind,
  onEvent: (event: ProcessEvent) => void,
): Promise<number | null> {
  const channel = new Channel<ProcessEvent>();
  channel.onmessage = onEvent;
  return invoke<number | null>("git_network", { kind, channel });
}

// ---------------------------------------------------------------------------
// Agent intent
// ---------------------------------------------------------------------------

/** The intent cards for the whole working tree, recomputed on every call. */
export const intentGroups = (mode: ComparisonMode) =>
  invoke<IntentGroup[]>("intent_groups", { mode });

/**
 * Stage everything in one group — or one file's share of it — returning how
 * many files changed.
 *
 * The group is named rather than its lines sent back: line indices are only
 * valid for one comparison mode, and staging uses a different one from
 * whatever the user is looking at.
 */
export const stageIntentGroup = (group: string, path?: string) =>
  invoke<number>("stage_intent_group", { group, path });

/**
 * Revert one group — or one file's share of it — in the mode currently
 * displayed.
 */
export const revertIntentGroup = (group: string, mode: ComparisonMode, path?: string) =>
  invoke<number>("revert_intent_group", { group, mode, path });

/**
 * Reject one group — or one file's share of it: revert it, and leave the reason
 * as a comment where the code was, for the agent to find and act on.
 *
 * Rejects only in the working-tree modes; the staged view is refused by Rust
 * rather than silently writing a note the reviewer cannot see.
 */
export const rejectIntentGroup = (
  group: string,
  mode: ComparisonMode,
  reason: string,
  path?: string,
) => invoke<RejectSummary>("reject_intent_group", { group, mode, path, reason });

/** What each agent can currently do for this workspace. */
export const intentCaptureStatus = () =>
  invoke<ProviderStatus[]>("intent_capture_status");

/** Exactly what enabling capture would write. Touches nothing. */
export const intentInstallPlan = (provider: ProviderId, scope: InstallScope) =>
  invoke<InstallPlan>("intent_install_plan", { provider, scope });

/** Perform an install the user has confirmed. */
export const enableIntentCapture = (provider: ProviderId, scope: InstallScope) =>
  invoke<ProviderStatus[]>("enable_intent_capture", { provider, scope });

/** Read what the agents already recorded, with no setup. Returns the total. */
export const importIntentHistory = () =>
  invoke<number>("import_intent_history");

export const clearIntentHistory = () => invoke<void>("clear_intent_history");

// ---------------------------------------------------------------------------
// Object inspection
// ---------------------------------------------------------------------------

/** Whether the inspector can run here, and which dumps are on disk. */
export const inspectStatus = () => invoke<InspectStatus>("inspect_status");

/**
 * Capture an object graph, streaming the inspector's own output to `onEvent`.
 *
 * Expanding past a cap is this same call with an `address` root: it is a fresh
 * read of the target, and the graph it returns carries a new `snapshotId` for
 * exactly that reason.
 *
 * `widen` names the cap that stopped the previous read of that branch, and the
 * backend raises it for this one. Passing `null` re-reads under the same
 * limits, which for a capped branch returns the identical truncation — so an
 * expand always passes the reason it is expanding.
 */
export function inspectCapture(
  target: InspectTarget,
  root: RootSpec,
  widen: ElidedReason | null,
  onEvent: (event: ProcessEvent) => void,
): Promise<InspectGraph> {
  const channel = new Channel<ProcessEvent>();
  channel.onmessage = onEvent;
  return invoke<InspectGraph>("inspect_capture", {
    target,
    root,
    widen,
    channel,
  });
}

/**
 * Every .NET process on this machine that can be attached to, each labelled
 * with how it was linked to a run configuration.
 *
 * An empty `processes` is a normal answer, not an error. A rejection means the
 * list could not be read at all, which is a different thing and must be shown
 * as one; `warnings` sits between the two — a real list that is missing the
 * evidence attribution depends on.
 */
export const inspectAttachable = () => invoke<AttachableList>("inspect_attachable");

/**
 * The dump a finished run may have written, and whether it is certainly that
 * run's.
 *
 * The attribution rule lives in the backend deliberately: with two
 * configurations up, the newest dump since a run started belongs to whichever
 * of them crashed last, so only a matching pid is evidence. `certain: false`
 * means the dump may be offered but must not be called this run's crash.
 */
export const inspectRunDump = (pid: number | null, startedAt: number) =>
  invoke<RunDump | null>("inspect_run_dump", { pid, startedAt });

/** The most recent capture, so a tab switch does not discard it. */
export const inspectLast = () => invoke<InspectGraph | null>("inspect_last");

export const inspectClear = () => invoke<void>("inspect_clear");

/** Tauri returns command errors as plain strings. */
export function errorMessage(error: unknown): string {
  if (typeof error === "string") return error;
  if (error instanceof Error) return error.message;
  return String(error);
}

/** Typed wrappers over the Tauri command surface. */

import { Channel, invoke } from "@tauri-apps/api/core";
import type {
  Branch,
  BuildAction,
  Commit,
  ComparisonMode,
  DirEntry,
  FileContents,
  FileDiff,
  NetworkKind,
  ProcessEvent,
  ProjectSecrets,
  RiderImportPreview,
  RunConfig,
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

/** Launch profile names a .NET project defines (`Project` profiles only). */
export const launchProfiles = (project: string) =>
  invoke<string[]>("launch_profiles", { project });

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

/** Tauri returns command errors as plain strings. */
export function errorMessage(error: unknown): string {
  if (typeof error === "string") return error;
  if (error instanceof Error) return error.message;
  return String(error);
}

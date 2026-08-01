/** Typed wrappers over the Tauri command surface. */

import { Channel, invoke } from "@tauri-apps/api/core";
import type {
  Branch,
  Commit,
  ComparisonMode,
  FileContents,
  FileDiff,
  NetworkKind,
  ProcessEvent,
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

export const previewRiderImport = () =>
  invoke<RiderImportPreview>("preview_rider_import");

export const applyRiderImport = (configs: RunConfig[]) =>
  invoke<Workspace>("apply_rider_import", { configs });

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
): Promise<void> {
  const channel = new Channel<ProcessEvent>();
  channel.onmessage = onEvent;
  return invoke<void>("start_run", { configId, channel });
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

export const gitCreateBranch = (name: string, checkout: boolean) =>
  invoke<void>("git_create_branch", { name, checkout });

export const gitCheckoutBranch = (name: string) =>
  invoke<void>("git_checkout_branch", { name });

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

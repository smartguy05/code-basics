/** Typed wrappers over the Tauri command surface. */

import { Channel, invoke } from "@tauri-apps/api/core";
import type {
  AnchorResult,
  ArchGraph,
  AttachableList,
  BehavioralReport,
  Branch,
  BuildAction,
  ChangeCoverage,
  Changelists,
  Commit,
  ComparisonMode,
  DefinitionResult,
  DiagramFile,
  DirEntry,
  ElidedReason,
  EnhancementInfo,
  ErosionReport,
  FileContents,
  FileDiff,
  InspectGraph,
  InspectStatus,
  InspectTarget,
  InstallPlan,
  InstallScope,
  IntentReview,
  LaunchedApp,
  LauncherFile,
  LauncherGroups,
  LaunchProfile,
  LineIntent,
  LspStatus,
  MergeReport,
  NetworkKind,
  NotesFile,
  ProcessEvent,
  ProjectSecrets,
  PromptInfo,
  PromptRuns,
  ProviderId,
  ProviderStatus,
  RejectSummary,
  RetireSummary,
  ReviewAgentInfo,
  RiderImportPreview,
  RootSpec,
  RulesReport,
  ProcessKind,
  RunConfig,
  RunDump,
  RunningReport,
  SearchHit,
  SearchScope,
  StashEntry,
  SymbolIndexStatus,
  TerminalEvent,
  TestRunOutcome,
  UsageResult,
  ValidationError,
  WorkingStatus,
  Workspace,
} from "./types";

// ---------------------------------------------------------------------------
// Workspace
// ---------------------------------------------------------------------------

/**
 * Open a codebase. With multiple workspaces the backend ADDS this root to its
 * open set (keyed by canonical root) and makes it the active one — it no longer
 * evicts whatever was open before. Opening an already-open root focuses and
 * rescans it. The returned `Workspace` is the freshly opened one.
 */
export const openWorkspace = (path: string) =>
  invoke<Workspace>("open_workspace", { path });

/** The currently ACTIVE workspace (the foreground tab), or null if none is open. */
export const currentWorkspace = () =>
  invoke<Workspace | null>("current_workspace");

export const rescanWorkspace = () => invoke<Workspace>("rescan_workspace");

/**
 * Every open workspace, in no particular order — the frontend orders its own tab
 * strip. Used to rebuild the tab bar after a reload, since there is no event
 * channel; identity is `Workspace.root`.
 */
export const listOpenWorkspaces = () =>
  invoke<Workspace[]>("list_open_workspaces");

/**
 * Make `root` the active workspace that the argument-free commands resolve
 * against. A cheap pointer move that tears nothing down — background workspaces
 * keep running. Must be awaited BEFORE the newly-active views issue their
 * commands, or they would query the previous workspace.
 */
export const setActiveWorkspace = (root: string) =>
  invoke<void>("set_active_workspace", { root });

/**
 * Close an open workspace: removes its slot, tears down its language server and
 * cancels its running processes, and repoints the active workspace to another
 * open one (or none). Returns the new active root, or null when nothing is left.
 */
export const closeWorkspace = (root: string) =>
  invoke<string | null>("close_workspace", { root });

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
// Enhancements (instruction templates for CLAUDE.md / AGENTS.md)
// ---------------------------------------------------------------------------

/** Every instruction template, flagged with whether it is installed here. */
export const listEnhancements = () =>
  invoke<EnhancementInfo[]>("list_enhancements");

/** Add a template's section to both agent files; returns the refreshed list. */
export const addEnhancement = (id: string) =>
  invoke<EnhancementInfo[]>("add_enhancement", { id });

/** Remove a template's section from both agent files; returns the refreshed list. */
export const removeEnhancement = (id: string) =>
  invoke<EnhancementInfo[]>("remove_enhancement", { id });

/** Every prompt template, each carrying the body to run as an agent. */
export const listPrompts = () => invoke<PromptInfo[]>("list_prompts");

/** The run-once record for the current workspace, keyed by prompt id. */
export const agentRuns = () => invoke<PromptRuns>("agent_runs");

/** Record a successful run of a run-once prompt in the current workspace. */
export const markAgentRun = (promptId: string) =>
  invoke<void>("mark_agent_run", { promptId });

/** Save a Notes-panel note into the instruction library as a `.md` template. */
export const saveNoteAsInstruction = (title: string, body: string) =>
  invoke<void>("save_note_as_instruction", { title, body });

// ---------------------------------------------------------------------------
// Notes / scratchpad (user-global, not per-workspace)
// ---------------------------------------------------------------------------

/** Read the global notes file. Missing/unreadable yields an empty set. */
export const readNotes = () => invoke<NotesFile>("read_notes");

/** Write the global notes file, creating its directory if absent. */
export const writeNotes = (file: NotesFile) =>
  invoke<void>("write_notes", { file });

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
  /**
   * Collect code coverage and map it onto the current diff. Off by default so an
   * ordinary run's command line is unchanged; when set, the mapped result is
   * cached for {@link coverageOfChange}. Trailing so existing calls are
   * unaffected.
   */
  withCoverage = false,
): Promise<TestRunOutcome> {
  const channel = new Channel<ProcessEvent>();
  channel.onmessage = onEvent;
  return invoke<TestRunOutcome>("run_tests", {
    configId,
    onlyFailed,
    withCoverage,
    channel,
  });
}

export const lastTestRun = (configId: string) =>
  invoke<TestRunOutcome | null>("last_test_run", { configId });

/**
 * The last coverage-of-change map for the active workspace: which changed lines
 * the most recent coverage-enabled test run never executed. Non-streaming, like
 * {@link erosionScan}. Returns an empty map carrying a warning when no coverage
 * has been collected yet.
 */
export const coverageOfChange = (mode: ComparisonMode) =>
  invoke<ChangeCoverage>("coverage_of_change", { mode });

// ---------------------------------------------------------------------------
// Agent runs (adversarial review + Run Agent)
// ---------------------------------------------------------------------------

/** The posture an agent runs under: read-only, or allowed to edit files. */
export type AgentMode = "read-only" | "edit";

/**
 * Run a chosen prompt against the open workspace with `claude`/`codex`,
 * streaming its output to `onEvent`. Serves both the adversarial review and the
 * Enhancements "Run Agent" action; `mode` picks the read-only/edit posture.
 *
 * Mirrors {@link startRun}: the promise resolves when the agent process exits,
 * so callers should not await it before rendering the console.
 */
export function startReview(
  promptId: string | undefined,
  agentId: string,
  model: string | undefined,
  mode: AgentMode,
  onEvent: (event: ProcessEvent) => void,
  /**
   * Injected context — evidence, business-rule docs — prepended to the prompt so
   * the agent reads it before the instruction. Blank/absent leaves the prompt
   * unchanged. Trailing so existing five-argument calls are unaffected.
   */
  context?: string,
  /**
   * An inline prompt body — a note's text sent straight to the agent — used in
   * place of a library prompt. When present it wins over `promptId`; when absent
   * the run falls back to the library prompt named by `promptId`.
   */
  promptBody?: string,
): Promise<void> {
  const channel = new Channel<ProcessEvent>();
  channel.onmessage = onEvent;
  return invoke<void>("start_review", {
    promptId,
    promptBody,
    agentId,
    model,
    mode,
    context,
    channel,
  });
}

export const cancelReview = () => invoke<boolean>("cancel_review");

/** The review agents whose CLI is installed, in preference order. */
export const reviewAgents = () => invoke<ReviewAgentInfo[]>("review_agents");

// ---------------------------------------------------------------------------
// Interactive terminals
// ---------------------------------------------------------------------------

/**
 * Open an interactive terminal, streaming its raw output to `onEvent`, and
 * resolve to the session id used by {@link terminalWrite}/{@link terminalResize}
 * /{@link terminalClose}. Output is one merged stream written straight to xterm
 * — no post-processing — because an interactive TUI (Claude Code's included)
 * redraws its own screen.
 *
 * `cols`/`rows` are the initial size; `cwd` defaults to the open workspace when
 * omitted.
 */
export function terminalOpen(
  cols: number,
  rows: number,
  onEvent: (event: TerminalEvent) => void,
  cwd?: string,
  label?: string,
): Promise<string> {
  const channel = new Channel<TerminalEvent>();
  channel.onmessage = onEvent;
  return invoke<string>("terminal_open", { cwd, cols, rows, label, channel });
}

/**
 * Update a terminal's label in the Running panel after the user renames it.
 * `root` is the terminal's cwd (the record key beside the session id).
 */
export const terminalSetLabel = (id: string, root: string, label: string) =>
  invoke<void>("terminal_set_label", { id, root, label });

/** Send keystrokes (or any bytes) to a terminal. */
export const terminalWrite = (id: string, data: string) =>
  invoke<void>("terminal_write", { id, data });

/** Tell a terminal its viewport changed size. */
export const terminalResize = (id: string, cols: number, rows: number) =>
  invoke<void>("terminal_resize", { id, cols, rows });

/** Close a terminal, killing its process tree. Resolves whether one was open. */
export const terminalClose = (id: string) =>
  invoke<boolean>("terminal_close", { id });

/** The ids of every open terminal. */
export const terminalList = () => invoke<string[]>("terminal_list");

// ---------------------------------------------------------------------------
// The app launcher
// ---------------------------------------------------------------------------

/**
 * The remembered command lines, grouped for the picker: the open codebase's
 * first, then everything the user has run anywhere.
 */
export const listLaunchables = () =>
  invoke<LauncherGroups>("list_launchables");

/**
 * Run a command line, streaming its output to `onEvent`.
 *
 * Unlike {@link startRun} this resolves as soon as the process is spawned — not
 * when it exits — because a launched app is typically long-lived and the picker
 * closes immediately. Watch `onEvent` for the exit. `cwd` defaults to the open
 * workspace; `shell` hands the whole line to the default shell, which is
 * required for anything using `|`, `>` or `&&` (an unquoted metacharacter is
 * otherwise refused rather than passed through as an argument).
 */
export function launchCommand(
  spec: {
    command: string;
    cwd?: string;
    shell: boolean;
    label?: string;
    /**
     * The key to address this launch by. Minted by the caller (not the backend)
     * because output starts arriving the moment the process spawns — before this
     * promise resolves — so the console needs its destination up front.
     */
    key: string;
  },
  onEvent: (event: ProcessEvent) => void,
): Promise<LaunchedApp> {
  const channel = new Channel<ProcessEvent>();
  channel.onmessage = onEvent;
  return invoke<LaunchedApp>("launch_command", { ...spec, channel });
}

/** Stop a launched app by the key {@link launchCommand} returned. */
export const stopCommand = (key: string) =>
  invoke<boolean>("stop_command", { key });

/** Pin/unpin or rename a remembered command; resolves to the updated file. */
export const saveLaunchable = (
  id: string,
  changes: { label?: string; pinned?: boolean },
) => invoke<LauncherFile>("save_launchable", { id, ...changes });

/** Forget a remembered command; resolves to the updated file. */
export const deleteLaunchable = (id: string) =>
  invoke<LauncherFile>("delete_launchable", { id });

// ---------------------------------------------------------------------------
// Running processes (the Running panel)
// ---------------------------------------------------------------------------

/** Everything running now across all open codebases, plus crash-orphans. */
export const listRunning = () => invoke<RunningReport>("list_running");

/**
 * Kill one process from the Running panel. `orphan` picks the safe path (kill by
 * pid after an identity re-check) vs. stopping a live process through its owning
 * subsystem. Resolves whether something was actually terminated.
 */
export const killRunning = (entry: {
  pid: number;
  kind: ProcessKind;
  root: string;
  key: string;
  orphan: boolean;
}) => invoke<boolean>("kill_running", entry);

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

/** Both sides of one file as a commit changed it, for the History diff. */
export const gitCommitFileContents = (id: string, path: string) =>
  invoke<FileContents>("git_commit_file_contents", { id, path });

/** The recorded reason behind each line of a file, as a past commit left it. */
export const gitCommitFileWhy = (id: string, path: string) =>
  invoke<LineIntent[]>("git_commit_file_why", { id, path });

export const gitStashSave = (message: string) =>
  invoke<void>("git_stash_save", { message });

export const gitStashList = () => invoke<StashEntry[]>("git_stash_list");

export const gitStashPop = (index = 0) => invoke<void>("git_stash_pop", { index });

export const gitStashApply = (index: number) =>
  invoke<void>("git_stash_apply", { index });

export const gitStashDrop = (index: number) =>
  invoke<void>("git_stash_drop", { index });

export const gitStashClear = () => invoke<void>("git_stash_clear");

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

/**
 * The intent review for the whole working tree, recomputed on every call:
 * the grouped cards, the unfulfilled claims, and the per-turn scorecard.
 */
export const intentGroups = (mode: ComparisonMode) =>
  invoke<IntentReview>("intent_groups", { mode });

/**
 * The erosion scan for the whole working tree — changes that quietly weaken the
 * codebase — recomputed on every call.
 */
export const erosionScan = (mode: ComparisonMode) =>
  invoke<ErosionReport>("erosion_scan", { mode });

/**
 * Every business-rule doc authored in the workspace's `.code-basics/rules/`.
 *
 * These carry no pattern and match nothing on their own — they are prose the
 * team wrote down, handed to a review as `context` so the agent judges the diff
 * against the stated invariants. `warnings` lists any file that would not read.
 */
export const listRules = () => invoke<RulesReport>("list_rules");

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

/**
 * Exactly what disabling a provider's capture would remove. Touches nothing.
 * An empty `writes` means there was nothing installed for that agent.
 */
export const intentUninstallPlan = (provider: ProviderId, scope: InstallScope) =>
  invoke<InstallPlan>("intent_uninstall_plan", { provider, scope });

/** Perform a disable the user has confirmed; returns the refreshed statuses. */
export const disableIntentCapture = (provider: ProviderId, scope: InstallScope) =>
  invoke<ProviderStatus[]>("disable_intent_capture", { provider, scope });

/** Read what the agents already recorded, with no setup. Returns the total. */
export const importIntentHistory = () =>
  invoke<number>("import_intent_history");

/**
 * How much recorded history a prune would retire, changing nothing. The dry run
 * shown before the archive action is confirmed.
 */
export const intentPrunePreview = () => invoke<RetireSummary>("intent_prune_preview");

/**
 * Archive every intent this workspace's HEAD has already absorbed. The only way
 * to clear a backlog recorded before pruning existed: the automatic prune needs
 * a baseline to notice HEAD moving against, so it never touches what was there
 * already. Retired records are archived and tombstoned, never destroyed.
 */
export const pruneIntentHistory = () => invoke<RetireSummary>("prune_intent_history");

export const clearIntentHistory = () => invoke<void>("clear_intent_history");

/**
 * Write (or overwrite) the user's own intent for one card. The note is stored
 * as the card's changed-line content, so it rebinds by content on the next
 * refresh and titles the card, overriding any agent reason there.
 */
export const setCardIntent = (group: string, label: string, mode: ComparisonMode) =>
  invoke<void>("set_card_intent", { group, label, mode });

/** Remove the user's note from one card. Returns whether one was found. */
export const clearCardIntent = (group: string, mode: ComparisonMode) =>
  invoke<boolean>("clear_card_intent", { group, mode });

// ---------------------------------------------------------------------------
// Quality-gate Stop hook (`qgate/`) — installed the same way the intent hooks
// are: preview a plan, then apply it.
// ---------------------------------------------------------------------------

/** Where the quality gate is installed for this workspace and provider, if anywhere. */
export const qualityGateStatus = (provider: ProviderId) =>
  invoke<InstallScope | null>("quality_gate_status", { provider });

/** Exactly what installing the quality gate for a provider would write. Touches nothing. */
export const qualityGateInstallPlan = (provider: ProviderId, scope: InstallScope) =>
  invoke<InstallPlan>("quality_gate_install_plan", { provider, scope });

/** Perform an install the user has confirmed; returns the new status. */
export const installQualityGate = (provider: ProviderId, scope: InstallScope) =>
  invoke<InstallScope | null>("install_quality_gate", { provider, scope });

/**
 * Exactly what turning the quality gate off for a provider would remove.
 * Touches nothing. An empty `writes` means there was nothing installed.
 */
export const qualityGateUninstallPlan = (provider: ProviderId, scope: InstallScope) =>
  invoke<InstallPlan>("quality_gate_uninstall_plan", { provider, scope });

/** Perform an uninstall the user has confirmed; returns the new status. */
export const uninstallQualityGate = (provider: ProviderId, scope: InstallScope) =>
  invoke<InstallScope | null>("uninstall_quality_gate", { provider, scope });

/** First-open setup: exactly what installing every hook at `scope` would write. */
export const setupInstallPlan = (scope: InstallScope) =>
  invoke<InstallPlan>("setup_install_plan", { scope });

/** Apply a confirmed first-open setup (intent capture + quality gate together). */
export const installSetup = (scope: InstallScope) =>
  invoke<void>("install_setup", { scope });

// ---------------------------------------------------------------------------
// Behavioral before/after testing (`behavioral/`)
// ---------------------------------------------------------------------------

/**
 * Run a configuration against both git HEAD and the working tree, then diff the
 * observable outcomes — test results, console output, and `.http` responses —
 * as evidence a change did what its intent claimed.
 *
 * The inspector-style output of both runs is streamed to `onEvent`; the promise
 * resolves with the assembled `BehavioralReport` once both sides have finished
 * and been compared.
 *
 * `httpFiles` names the `.http` scenarios to replay, or `null` to let the
 * backend discover them.
 */
export function behavioralDiff(
  configId: string,
  httpFiles: string[] | null,
  onEvent: (event: ProcessEvent) => void,
): Promise<BehavioralReport> {
  const channel = new Channel<ProcessEvent>();
  channel.onmessage = onEvent;
  return invoke<BehavioralReport>("behavioral_diff", { configId, httpFiles, channel });
}

/** Discard the cached baseline worktrees; returns any teardown warnings. */
export const behavioralClear = () => invoke<string[]>("behavioral_clear");

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

// ---------------------------------------------------------------------------
// Search everywhere
// ---------------------------------------------------------------------------

/**
 * Rank everything the query could mean, best first.
 *
 * `query` is passed through exactly as the user typed it, trailing `:123` and
 * all: the line suffix is parsed in `cb-core`, and re-deriving it here would be
 * a second implementation of a rule that decides where the editor jumps. Read
 * the line off `SearchHit.line` instead.
 *
 * A query that matches nothing resolves to an empty array — "nothing is called
 * that" is an answer, not an error. `limit` is optional and the backend
 * chooses when it is omitted.
 */
export const searchEverywhere = (
  query: string,
  scope: SearchScope,
  limit?: number,
) => invoke<SearchHit[]>("search_everywhere", { query, scope, limit });

/** What the index holds, and whether a build is in flight over it. */
export const symbolIndexStatus = () =>
  invoke<SymbolIndexStatus>("symbol_index_status");

/** Discard the index and walk the workspace again. */
export const rebuildSymbolIndex = () => invoke<void>("rebuild_symbol_index");

// ---------------------------------------------------------------------------
// Architecture diagrams
// ---------------------------------------------------------------------------

/**
 * The project graph, derived from the manifests as they are on disk right now.
 *
 * Nothing is cached on either side of the IPC boundary: the inputs are files
 * the user edits while the workspace stays open, and a stale arrow asserts a
 * dependency that may since have been deleted. Call it again rather than
 * holding one.
 *
 * A non-empty `warnings` is normal and must be surfaced — it lists every
 * reference that could not be turned into an edge, which is the only way a
 * reader can tell a complete diagram from one that merely looks complete.
 */
export const archProjectGraph = () => invoke<ArchGraph>("arch_project_graph");

/** The same graph, rendered to Mermaid source. Renders only; stores nothing. */
export const archRenderGraph = () => invoke<string>("arch_render_graph");

/**
 * The component map: the services this workspace runs and the data stores they
 * declare they speak to.
 *
 * A **different question** from `archProjectGraph`, and presenting one as the
 * other is the worst thing a caller can do with either: the project map is
 * what is in the repository, this is what the system consists of at run time.
 * An empty result is a real answer — a repository of class libraries has no
 * components — and the backend deliberately does not fall back to the project
 * map to avoid returning one. Label the view accordingly.
 *
 * `warnings` matters more here than anywhere else and must be surfaced: it is
 * where every candidate that was seen and refused ends up, including the
 * cross-project HTTP calls that were read but may not be drawn as arrows. It
 * also carries a note when the symbol index was not ready, which costs route
 * details and nothing else — no box and no arrow comes from a route, so the
 * map is smaller then, never wrong.
 */
export const archComponentGraph = () => invoke<ArchGraph>("arch_component_graph");

/**
 * The component map as Mermaid source. Renders only; stores nothing.
 *
 * Mermaid source is nodes and edges, so the warnings do not survive it. Call
 * `archComponentGraph` alongside this if you draw the picture, or the reader
 * has no way to tell what was left off it.
 */
export const archRenderComponentGraph = () =>
  invoke<string>("arch_render_component_graph");

/**
 * Every stored diagram, committed ones first, each group alphabetical.
 *
 * The order is part of the contract, so a list cannot reshuffle under the
 * user's cursor between calls.
 */
export const archListDiagrams = () =>
  invoke<DiagramFile[]>("arch_list_diagrams");

/** One diagram exactly as it is on disk, front matter included. */
export const archReadDiagram = (name: string) =>
  invoke<string>("arch_read_diagram", { name });

/**
 * Save an edit. Resolves with the problem the saved text carries, or `null`.
 *
 * **The file is written either way.** A resolved `ValidationError` means saved
 * *and* broken — show it beside the editor, do not treat it as a failed save.
 * Mermaid passes through invalid states on the way to every valid one, so a
 * save that refused them would be a save the user cannot use while they are
 * still drawing. Only a rejection means nothing was written.
 *
 * Re-list afterwards rather than reusing the path you had: editing a derived
 * diagram promotes it out of the gitignored regenerated directory, so a save
 * can move the file. Provenance is taken from the copy already on disk and
 * never from the text being saved, so typing `derivation: derived` into the
 * editor cannot pass a drawing off as a fact read out of the manifests.
 */
export const archWriteDiagram = (name: string, contents: string) =>
  invoke<ValidationError | null>("arch_write_diagram", { name, contents });

/**
 * Check Mermaid source without storing it: `null` means it will render.
 *
 * Invalid source resolves rather than rejects — a diagram someone is midway
 * through typing is an ordinary editing state, not a failed command.
 */
export const archValidate = (source: string) =>
  invoke<ValidationError | null>("arch_validate", { source });

// ---------------------------------------------------------------------------
// Language servers (`crates/core/src/lsp/`)
// ---------------------------------------------------------------------------

/**
 * What every configured server is doing right now.
 *
 * Cheap and synchronous behind the scenes — a read of a shared snapshot, not a
 * round trip to any server — so it is safe to poll for a status row. A language
 * that has never been asked anything is **absent** from `servers` rather than
 * listed as starting; only a language that was started, or one that could not be
 * resolved at all, appears.
 */
export const lspStatus = () => invoke<LspStatus>("lsp_status");

/** Tear down this workspace's language-server session and start a fresh one. */
export const lspRestart = () => invoke<LspStatus>("lsp_restart");

/**
 * Tell the servers the editor now holds `text` for `path`.
 *
 * `path` is workspace-relative, as everywhere else in this file. Resolves once
 * the notification is enqueued; there is nothing to wait for, because a
 * notification has no reply. Send this before asking anything about a file the
 * user is editing, or the server answers about what is on disk.
 */
export const lspOpenDocument = (path: string, text: string) =>
  invoke<void>("lsp_open_document", { path, text });

/** The document's contents changed. Whole text, not a delta. */
export const lspChangeDocument = (path: string, text: string) =>
  invoke<void>("lsp_change_document", { path, text });

/** The editor closed the document, so the servers go back to disk. */
export const lspCloseDocument = (path: string) =>
  invoke<void>("lsp_close_document", { path });

/**
 * Every use site of the symbol at `line`/`character`.
 *
 * **`line` is 1-based** (the editor gutter, `SearchHit.line`,
 * `DeclarationAnchor.selectionLine`) and **`character` is 0-based UTF-16 code
 * units** (what CodeMirror hands over). The asymmetry is the IPC contract in both
 * directions; see `Target.character` in `types.ts`.
 *
 * **Never rejects for a missing answer.** A server that is absent, still
 * starting, still loading, dead or without the capability comes back as a
 * resolved `UsageResult` whose `outcome` says which of those it was and whose
 * `total` is `null` — five distinct reasons, none of them an empty list. Only
 * `outcome: "ready"` licenses showing a count, and `total: 0` under it is the
 * genuine "no usages". A rejection means the command itself failed.
 */
export const lspFindUsages = (path: string, line: number, character: number) =>
  invoke<UsageResult>("lsp_find_usages", { path, line, character });

/**
 * Where the symbol at `line`/`character` is declared, implemented and typed.
 *
 * Same position convention and same abstain rule as `lspFindUsages`. The three
 * lists answer three different questions and a symbol may appear in more than
 * one; an empty list is "none" only when `outcome` is `"ready"`.
 */
export const lspGotoDefinition = (
  path: string,
  line: number,
  character: number,
) =>
  invoke<DefinitionResult>("lsp_goto_definition", { path, line, character });

/**
 * Which declarations in `path` deserve an inline "N usages" row.
 *
 * Aim the follow-up `lspFindUsages` at each anchor's `selectionLine` and
 * `character` — not at `line`, which is where the row is *drawn* and can sit
 * above the identifier when attributes or a wrapped signature intervene.
 */
export const lspDeclarationAnchors = (path: string) =>
  invoke<AnchorResult>("lsp_declaration_anchors", { path });

/** Tauri returns command errors as plain strings. */
export function errorMessage(error: unknown): string {
  if (typeof error === "string") return error;
  if (error instanceof Error) return error.message;
  return String(error);
}

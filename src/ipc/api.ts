/** Typed wrappers over the Tauri command surface. */

import { Channel, invoke } from "@tauri-apps/api/core";
import {
  isDebugTerminal,
  isProcessTerminal,
  isSqlTerminal,
  isTerminalStreamTerminal,
} from "./channelLogic";

/** A handler that captures nothing, used to release a spent channel's closure. */
const NOOP = () => {};

/**
 * Deliver a streamed channel's events to `onEvent`, then release the handler
 * once the stream ends.
 *
 * Tauri registers `onmessage` in the webview's `__TAURI_INTERNALS__` registry
 * for the life of the page and never drops it, so a per-call `onEvent` (which
 * pins React state) leaks for every run/build/test/terminal/query. Swapping in a
 * no-op on the terminal event releases that closure while leaving live streams
 * untouched. Not `null` — the `onmessage` type forbids it. See {@link
 * ./channelLogic} for why each `isTerminal` names a single unambiguous end
 * marker.
 */
function streaming<T>(
  channel: Channel<T>,
  onEvent: (event: T) => void,
  isTerminal: (event: T) => boolean,
): void {
  channel.onmessage = (event) => {
    onEvent(event);
    if (isTerminal(event)) channel.onmessage = NOOP;
  };
}
import type {
  BrowserConsoleBatch,
  BrowserNetworkBatch,
  BrowserPageText,
  BrowserRect,
  BrowserSnapshot,
  AboutInfo,
  AgentCommand,
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
  DebugEvent,
  DetectedShells,
  DiagramFile,
  DirEntry,
  EditorContext,
  ElidedReason,
  EnhancementInfo,
  ErosionReport,
  FeatureInfo,
  McpServerToolsInfo,
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
  PrepareRenameResult,
  ProviderId,
  ProviderStatus,
  RejectSummary,
  RenameResult,
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
  SqlConnectionProfile,
  SqlConnectionView,
  RedisConnectionView,
  RedisDiscovery,
  RedisScanPage,
  RedisValue,
  RedisKeyInfo,
  RedisStatusKind,
  SqlColumnView,
  SqlObjectView,
  SqlDiscovery,
  SqlEngine,
  SqlEvent,
  SqlStopOutcome,
  SqlTestOutcome,
  StashEntry,
  SymbolIndexStatus,
  TaskOwner,
  TaskStatus,
  TasksFile,
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

/** Create an empty file, and any parent directories it needs. Never overwrites. */
export const fsCreateFile = (path: string) =>
  invoke<void>("fs_create_file", { path });

/** Create a directory, and any parent directories it needs. */
export const fsCreateDir = (path: string) =>
  invoke<void>("fs_create_dir", { path });

/** Rename or move a file or directory. Refuses an occupied destination. */
export const fsRename = (from: string, to: string) =>
  invoke<void>("fs_rename", { from, to });

/** Delete a file, or a directory and everything under it. Permanent. */
export const fsDelete = (path: string) =>
  invoke<void>("fs_delete", { path });

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

/**
 * Every optional feature with its current state. The first call also adopts an
 * installer seed, if one is present and the user has no store yet.
 */
export const listFeatures = () => invoke<FeatureInfo[]>("list_features");

/**
 * Switch one feature on or off. Returns the whole list as persisted, so the
 * caller re-renders from what was written rather than what it hoped to write.
 */
export const setFeature = (id: string, enabled: boolean) =>
  invoke<FeatureInfo[]>("set_feature", { id, enabled });

/** Every MCP server with its tools and their enabled state. */
export const listMcpTools = () =>
  invoke<McpServerToolsInfo[]>("list_mcp_tools");

/**
 * Switch one MCP tool on or off. Returns the whole list as persisted, so the
 * caller re-renders from what was actually written, not what it hoped to write.
 */
export const setMcpTool = (server: string, tool: string, enabled: boolean) =>
  invoke<McpServerToolsInfo[]>("set_mcp_tool", { server, tool, enabled });

/** Read the global notes file. Missing/unreadable yields an empty set. */
export const readNotes = () => invoke<NotesFile>("read_notes");

/** Write the global notes file, creating its directory if absent. */
export const writeNotes = (file: NotesFile) =>
  invoke<void>("write_notes", { file });

// ---------------------------------------------------------------------------
// Tasks (`cb_core::tasks`) — the per-workspace, gitignored task list. Every
// command takes an explicit `root`: tasks are per-repository and several
// codebases can be open at once, so the panel names its own workspace rather
// than reading the active one. Each mutation returns the whole updated
// `TasksFile`, so the panel re-renders from the persisted truth.
// ---------------------------------------------------------------------------

/** Read this workspace's tasks. A missing or unreadable file is an empty list. */
export const readTasks = (root: string) =>
  invoke<TasksFile>("read_tasks", { root });

/** Create a new open task owned by the user; returns the updated list. */
export const createTask = (root: string, title: string, body: string) =>
  invoke<TasksFile>("create_task", { root, title, body });

/** Overwrite a task's title and body; returns the updated list. */
export const updateTask = (root: string, id: string, title: string, body: string) =>
  invoke<TasksFile>("update_task", { root, id, title, body });

/**
 * Assign a task to the user or the agent; returns the updated list. Launching
 * the agent when the owner becomes the AI is the frontend's job — this only
 * records the owner.
 */
export const assignTask = (root: string, id: string, owner: TaskOwner) =>
  invoke<TasksFile>("assign_task", { root, id, owner });

/** Set a task's status (done, or back to open); returns the updated list. */
export const completeTask = (root: string, id: string, status: TaskStatus) =>
  invoke<TasksFile>("complete_task", { root, id, status });

/** Remove a task; returns the updated list. */
export const deleteTask = (root: string, id: string) =>
  invoke<TasksFile>("delete_task", { root, id });

// ---------------------------------------------------------------------------
// About (Help -> About)
// ---------------------------------------------------------------------------

/**
 * What build is running: host platform, and the commit and instant `build.rs`
 * stamped in. Cannot fail — anything it could not establish comes back as the
 * literal `"unknown"` rather than as an error or a blank.
 *
 * The application and Tauri versions are *not* here; read those from
 * `@tauri-apps/api/app`, which the bundle already carries.
 */
export const aboutInfo = () => invoke<AboutInfo>("about_info");

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
  /**
   * Debug / Release / whatever the project declares, for this run only. Omit
   * to keep the configuration's own default.
   */
  buildConfiguration?: string,
): Promise<void> {
  const channel = new Channel<ProcessEvent>();
  streaming(channel, onEvent, isProcessTerminal);
  return invoke<void>("start_run", { configId, channel, env, buildConfiguration });
}

/** Build / rebuild / clean the project behind a .NET configuration. */
export function buildProject(
  configId: string,
  action: BuildAction,
  onEvent: (event: ProcessEvent) => void,
  /** The toolbar's build configuration, so a build matches the next run. */
  buildConfiguration?: string,
): Promise<void> {
  const channel = new Channel<ProcessEvent>();
  streaming(channel, onEvent, isProcessTerminal);
  return invoke<void>("build_project", { configId, action, channel, buildConfiguration });
}

export const cancelRun = (configId: string) =>
  invoke<boolean>("cancel_run", { configId });

export const runningIds = () => invoke<string[]>("running_ids");

/** Launch a configuration under its ecosystem's debug adapter. */
export function startDebug(
  configId: string,
  onEvent: (event: DebugEvent) => void,
  env?: Record<string, string>,
  buildConfiguration?: string,
): Promise<void> {
  const channel = new Channel<DebugEvent>();
  streaming(channel, onEvent, isDebugTerminal);
  return invoke<void>("start_debug", { configId, channel, env, buildConfiguration });
}

export const stopDebug = (configId: string) =>
  invoke<boolean>("stop_debug", { configId });

export const debugIds = () => invoke<string[]>("debug_ids");

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
  streaming(channel, onEvent, isProcessTerminal);
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
  streaming(channel, onEvent, isProcessTerminal);
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

/**
 * The program and argv that start an interactive agent session already asked
 * `prompt` — the command line behind "Ask the codebase", handed straight to
 * {@link terminalOpen}.
 *
 * Built in `cb_core::review` rather than here on purpose: the argument order,
 * the model validation and the refusals are one decision and live in one place,
 * so the frontend never assembles a command line of its own.
 */
export const agentInteractiveCommand = (agentId: string, model: string | undefined, prompt: string) =>
  invoke<AgentCommand>("agent_interactive_command", { agentId, model, prompt });

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
 *
 * `program`/`args` run something other than the default shell — an interactive
 * agent seeded with a question, for "Ask the codebase". They are **appended**
 * and optional so every existing caller (a plain terminal) is untouched. The
 * arguments reach `CommandBuilder` directly — nothing joins or re-splits them —
 * so through a real executable a question containing a quote, a newline or a
 * `&` crosses verbatim as one argv entry. Through a Windows `.cmd`/`.bat` shim
 * it does not: `cmd.exe` re-parses the command line, so an argument carrying
 * `&`, `|`, `<`, `>`, `^`, `"` or `%` is **rejected** by the backend before the
 * spawn (this promise rejects with a reason naming the character) rather than
 * running as something else. Args given without a program are dropped by the
 * backend rather than handed to the shell.
 */
export function terminalOpen(
  cols: number,
  rows: number,
  onEvent: (event: TerminalEvent) => void,
  cwd?: string,
  label?: string,
  program?: string,
  args?: string[],
): Promise<string> {
  const channel = new Channel<TerminalEvent>();
  streaming(channel, onEvent, isTerminalStreamTerminal);
  return invoke<string>("terminal_open", { cwd, cols, rows, label, program, args, channel });
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

/**
 * The shells on this machine, and which of them a terminal opens with no
 * preference set. Read-only detection — nothing is spawned.
 *
 * `shells` may legitimately be **empty**, which is not a failure: a terminal
 * opened with no program still runs the platform default. And a shell may
 * appear or vanish between calls (a tool mid-upgrade, a PATH change), so the
 * list is what is here *now* rather than something to cache.
 */
export const listShells = () => invoke<DetectedShells>("list_shells");

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
  streaming(channel, onEvent, isProcessTerminal);
  return invoke<LaunchedApp>("launch_command", { ...spec, channel });
}

/** Stop a launched app by the key {@link launchCommand} returned. */
export const stopCommand = (key: string) =>
  invoke<boolean>("stop_command", { key });

/**
 * Apply a partial update to a remembered command; resolves to the updated file.
 * Every field is optional and an omitted one is left alone, so a caller sends
 * only what the user changed.
 */
export const saveLaunchable = (
  id: string,
  changes: {
    label?: string;
    pinned?: boolean;
    shortcut?: boolean;
    persistent?: boolean;
    headless?: boolean;
  },
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

/**
 * Create a worktree on a new branch and return its directory, to open in a new
 * project tab. `base` is the start point (a branch or revision; absent = HEAD);
 * `dir` overrides the default sibling location.
 */
export const gitAddWorktree = (name: string, base?: string, dir?: string) =>
  invoke<string>("git_add_worktree", { name, base, dir });

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

export const gitStashPaths = (message: string, paths: string[]) =>
  invoke<string>("git_stash_paths", { message, paths });

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
  streaming(channel, onEvent, isProcessTerminal);
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

/** Where a move is going: an existing card, or a new one with this name. */
export interface MoveDestination {
  group?: string;
  label?: string;
}

/**
 * Move some of a card's changes into another card, or into a new one.
 *
 * `paths` narrows the move to those of the card's files; an empty array moves
 * the whole card. Stored like a hand-written note — as the moved lines'
 * *content* — so it rebinds when the lines shift and outranks any agent reason
 * on them.
 */
export const moveCardEdits = (
  group: string,
  paths: string[],
  destination: MoveDestination,
  mode: ComparisonMode,
) => invoke<void>("move_card_edits", { group, paths, destination, mode });

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

// ---------------------------------------------------------------------------
// The SQL MCP server (`mcp/install/`) — the same preview-then-apply shape as the
// quality gate, and deliberately over the same four IPC types: a status is
// exactly `InstallScope | null`, so nothing new crosses the boundary.
// ---------------------------------------------------------------------------

/** Where the SQL MCP server is installed for this workspace and provider, if anywhere. */
export const mcpServerStatus = (provider: ProviderId) =>
  invoke<InstallScope | null>("mcp_server_status", { provider });

/** Exactly what installing the SQL MCP server would write. Touches nothing. */
export const mcpServerInstallPlan = (provider: ProviderId, scope: InstallScope) =>
  invoke<InstallPlan>("mcp_server_install_plan", { provider, scope });

/** Perform an install the user has confirmed; returns the new status. */
export const installMcpServer = (provider: ProviderId, scope: InstallScope) =>
  invoke<InstallScope | null>("install_mcp_server", { provider, scope });

/**
 * Exactly what removing the SQL MCP server would rewrite. Touches nothing. An
 * empty `writes` means that configuration holds no entry of ours.
 */
export const mcpServerUninstallPlan = (provider: ProviderId, scope: InstallScope) =>
  invoke<InstallPlan>("mcp_server_uninstall_plan", { provider, scope });

/** Perform a removal the user has confirmed; returns the new status. */
export const uninstallMcpServer = (provider: ProviderId, scope: InstallScope) =>
  invoke<InstallScope | null>("uninstall_mcp_server", { provider, scope });

// ---------------------------------------------------------------------------
// The Tasks MCP server (`tasks/mcp/install`) — the same preview-then-apply shape
// as the SQL MCP server, over the same four IPC types. Unlike the SQL server it
// takes a `root`: the store is per-workspace, and a project-scope install bakes
// `--workspace <root>` in as the consent boundary.
// ---------------------------------------------------------------------------

/** Where the Tasks MCP server is installed for this workspace and provider, if anywhere. */
export const tasksMcpStatus = (root: string, provider: ProviderId) =>
  invoke<InstallScope | null>("tasks_mcp_status", { root, provider });

/** Exactly what installing the Tasks MCP server would write. Touches nothing. */
export const tasksMcpInstallPlan = (root: string, provider: ProviderId, scope: InstallScope) =>
  invoke<InstallPlan>("tasks_mcp_install_plan", { root, provider, scope });

/** Perform an install the user has confirmed; returns the new status. */
export const installTasksMcpServer = (root: string, provider: ProviderId, scope: InstallScope) =>
  invoke<InstallScope | null>("install_tasks_mcp_server", { root, provider, scope });

/**
 * Exactly what removing the Tasks MCP server would rewrite. Touches nothing. An
 * empty `writes` means that configuration holds no entry of ours.
 */
export const tasksMcpUninstallPlan = (root: string, provider: ProviderId, scope: InstallScope) =>
  invoke<InstallPlan>("tasks_mcp_uninstall_plan", { root, provider, scope });

/** Perform a removal the user has confirmed; returns the new status. */
export const uninstallTasksMcpServer = (root: string, provider: ProviderId, scope: InstallScope) =>
  invoke<InstallScope | null>("uninstall_tasks_mcp_server", { root, provider, scope });

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
  streaming(channel, onEvent, isProcessTerminal);
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
  streaming(channel, onEvent, isProcessTerminal);
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

/**
 * Whether the symbol at `line`/`character` can be renamed, and over what span.
 *
 * Same position convention as {@link lspFindUsages}. Ask this *before* opening
 * the rename field, and treat three answers as distinct:
 *
 * * `outcome !== "ready"` — nobody could be asked. Say why; do not open a field.
 * * `outcome === "ready"` with `renameable: false` — the server says this is not
 *   a rename site. A refusal, never an empty box.
 * * `outcome === "ready"` with `renameable: true` — go ahead. The four position
 *   fields may all be `null` (the `defaultBehavior` shape) and `placeholder` is
 *   `null` for real Roslyn, so the field's prefill comes from the buffer
 *   (`renameLogic.identifierAt`) rather than from this answer.
 */
export const lspPrepareRename = (path: string, line: number, character: number) =>
  invoke<PrepareRenameResult>("lsp_prepare_rename", { path, line, character });

/**
 * Rename the symbol at `line`/`character`.
 *
 * Same position convention as {@link lspFindUsages}. **Flush any owed
 * `lspChangeDocument` before calling this and wait for it to resolve**: the
 * ranges coming back are applied to the editor's text, so they must have been
 * computed from the editor's text. A server answering about a buffer two edits
 * old returns ranges that are plausible and wrong.
 *
 * `oldName` is the identifier the field was prefilled with. It is the backend's
 * stale-mirror check — each edit must land on a token of that name — and `""` is
 * a documented abstention from that check rather than a convenient default.
 *
 * **The answer is split and both halves must be honoured.** `written` names the
 * closed files the backend already wrote to disk; `buffers` carries the edits for
 * files this editor has open, which only the editor can apply without clobbering
 * an unsaved buffer. Dispatch those synchronously in the `.then`, and *report* a
 * `buffers` entry no editor received rather than dropping it — that gap is the
 * one window the design cannot close.
 */
export const lspRename = (
  path: string,
  line: number,
  character: number,
  oldName: string,
  newName: string,
) => invoke<RenameResult>("lsp_rename", { path, line, character, oldName, newName });

// ---------------------------------------------------------------------------
// The SQL console
// ---------------------------------------------------------------------------

/**
 * Every saved connection, redacted.
 *
 * No command in this section ever returns a connection string: a saved literal
 * comes back only as the redacted `display` on its {@link SqlSecretView}. The
 * store is user-global, not per-workspace, because a connection string is a
 * password and `.code-basics/` is the directory this app shares with the team.
 */
export const sqlListConnections = () =>
  invoke<SqlConnectionView[]>("sql_list_connections");

/**
 * The connections a workspace mentions — appsettings, user secrets, `.env`.
 *
 * Reads files and nothing else: it connects to nothing and saves nothing, and a
 * candidate carries a *reference* to where its connection string lives rather
 * than the string. Read `state` before offering to connect: `unresolved` means
 * the value is still a variable reference, which is not the same as an engine
 * nobody could determine.
 */
export const sqlDiscover = (root: string) =>
  invoke<SqlDiscovery>("sql_discover", { root });

/**
 * Add or update a saved connection; returns the redacted list.
 *
 * `allowWrites` on the payload is **ignored**. Consent moves only through
 * {@link sqlSetAllowWrites}, so no form round-trip can turn the read-only guard
 * off, and a newly saved profile always starts with writes disallowed.
 */
export const sqlSaveConnection = (connection: SqlConnectionProfile) =>
  invoke<SqlConnectionView[]>("sql_save_connection", { connection });

/** Forget a saved connection. Rejects when the id names nothing. */
export const sqlDeleteConnection = (id: string) =>
  invoke<SqlConnectionView[]>("sql_delete_connection", { id });

/**
 * Rename a saved connection, and record that the user chose the name.
 *
 * Its own verb rather than a `sqlSaveConnection` round-trip, and not for
 * tidiness: a {@link SqlConnectionView} carries a **redacted** secret, so the
 * only profile a caller holding one can rebuild has the display form where the
 * password was. Posting a rename that way would break the connection it renamed.
 *
 * `name` must already have passed `acceptedConnectionName` — the one acceptance
 * rule, shared with the create form.
 */
export const sqlRenameConnection = (id: string, name: string) =>
  invoke<SqlConnectionView[]>("sql_rename_connection", { id, name });

/**
 * Allow or disallow writes on one connection — the consent action.
 *
 * Its own verb on purpose. Enabling writes both lifts the read-only guard for
 * recognised writes (it never lifts a *refusal*, which is a different verdict)
 * and, on an engine whose driver has a read-only open mode, gives up that
 * protection: SQLite is then opened without `SQLITE_OPEN_READONLY`.
 */
export const sqlSetAllowWrites = (id: string, allowWrites: boolean) =>
  invoke<SqlConnectionView[]>("sql_set_allow_writes", { id, allowWrites });

/**
 * Expose or un-expose one connection to agents through the MCP server — the
 * second consent action, and the stronger one.
 *
 * Orthogonal to {@link sqlSetAllowWrites}: the agent path forces read-only
 * regardless of `allowWrites`, so no combination of the two lets an agent
 * write. What this grants is *reading*, and an agent with read access can read
 * anything that login can read — including credentials the database itself
 * stores. Expose only connections whose login you would give a colleague read
 * access to.
 *
 * Un-exposing takes effect on the next call: the server re-reads the store per
 * request and caches no connection.
 */
export const sqlSetExposeToAgents = (id: string, exposeToAgents: boolean) =>
  invoke<SqlConnectionView[]>("sql_set_expose_to_agents", { id, exposeToAgents });

// --- Redis plugin -----------------------------------------------------------

export const redisListConnections = () =>
  invoke<RedisConnectionView[]>("redis_list_connections");

export const redisDiscover = (root: string) =>
  invoke<RedisDiscovery>("redis_discover", { root });

export const redisSaveConnection = (connection: unknown) =>
  invoke<RedisConnectionView[]>("redis_save_connection", { connection });

export const redisDeleteConnection = (id: string) =>
  invoke<RedisConnectionView[]>("redis_delete_connection", { id });

export const redisRenameConnection = (id: string, name: string) =>
  invoke<RedisConnectionView[]>("redis_rename_connection", { id, name });

export const redisSetAllowWrites = (id: string, allowWrites: boolean) =>
  invoke<RedisConnectionView[]>("redis_set_allow_writes", { id, allowWrites });

export const redisSetExposeToAgents = (id: string, exposeToAgents: boolean) =>
  invoke<RedisConnectionView[]>("redis_set_expose_to_agents", { id, exposeToAgents });

export const redisTestConnection = (id: string) =>
  invoke<RedisStatusKind>("redis_test_connection", { id });

export const redisScanKeys = (
  id: string,
  match: string | null,
  cursor: string | null,
  count: number | null,
) => invoke<RedisScanPage>("redis_scan_keys", { id, match, cursor, count });

export const redisGetKey = (id: string, key: string) =>
  invoke<RedisValue>("redis_get_key", { id, key });

export const redisKeyInfo = (id: string, key: string) =>
  invoke<RedisKeyInfo>("redis_key_info", { id, key });

export const redisSetString = (id: string, key: string, value: string, ttlMs: number | null) =>
  invoke<void>("redis_set_string", { id, key, value, ttlMs });

export const redisHashSet = (id: string, key: string, field: string, value: string) =>
  invoke<void>("redis_hash_set", { id, key, field, value });

export const redisHashDelete = (id: string, key: string, field: string) =>
  invoke<void>("redis_hash_delete", { id, key, field });

export const redisListPush = (id: string, key: string, value: string, front: boolean) =>
  invoke<void>("redis_list_push", { id, key, value, front });

export const redisListRemove = (id: string, key: string, value: string, count: number) =>
  invoke<void>("redis_list_remove", { id, key, value, count });

export const redisSetAdd = (id: string, key: string, member: string) =>
  invoke<void>("redis_set_add", { id, key, member });

export const redisSetRemove = (id: string, key: string, member: string) =>
  invoke<void>("redis_set_remove", { id, key, member });

export const redisZsetAdd = (id: string, key: string, member: string, score: number) =>
  invoke<void>("redis_zset_add", { id, key, member, score });

export const redisZsetRemove = (id: string, key: string, member: string) =>
  invoke<void>("redis_zset_remove", { id, key, member });

export const redisStreamAdd = (
  id: string,
  key: string,
  entryId: string | null,
  fields: [string, string][],
) => invoke<void>("redis_stream_add", { id, key, entryId, fields });

export const redisDeleteKey = (id: string, key: string) =>
  invoke<void>("redis_delete_key", { id, key });

export const redisExpire = (id: string, key: string, ttlMs: number | null) =>
  invoke<void>("redis_expire", { id, key, ttlMs });

/**
 * Open the connection, prove a database is behind it, ask its version, and
 * close it.
 *
 * The outcome is a variant and not a boolean, and the variants are the point: a
 * timeout, a wrong password, an unresolvable secret and an engine this build
 * has no driver for are four things the user does four different things about.
 * `failed` is the honest fallback for a driver message the backend has no rule
 * for — do not present it as "unreachable".
 *
 * Two of them are easy to merge and must not be. `notADatabase` means the
 * handle opened and what is behind it is not a database (SQLite opens any file
 * and only fails when a page is read, so this is *not* an `ok`), and
 * `cannotOpenFile` means the path itself would not open — which is a wrong
 * path, not an `unreachable` host.
 */
export const sqlTestConnection = (id: string) =>
  invoke<SqlTestOutcome>("sql_test_connection", { id });

/**
 * Test a manually entered connection without saving it. The string travels
 * only toward the backend and no response type has a field that can echo it.
 */
export const sqlTestConnectionString = (engine: SqlEngine, connectionString: string) =>
  invoke<SqlTestOutcome>("sql_test_connection_string", { engine, connectionString });

/** List the selected database's objects. Tables are the first supported kind. */
export const sqlListObjects = (connectionId: string) =>
  invoke<SqlObjectView[]>("sql_list_objects", { connectionId });

/** Lazily load one table's column definitions. */
export const sqlListColumns = (connectionId: string, schema: string | null, table: string) =>
  invoke<SqlColumnView[]>("sql_list_columns", { connectionId, schema, table });

/**
 * Run SQL, streaming its rows to `onEvent`.
 *
 * `queryId` is minted by the caller — it is the handle {@link sqlCancel} stops
 * the statement by, and it must be unique among the statements running on this
 * connection. The promise resolves once the run is over; the last event is
 * always `finished`, and everything the driver produced has already been
 * delivered before it arrives.
 *
 * **The `rows` events are the rows.** `completed` carries a {@link
 * SqlCompletion} — the counts, the cap and the elapsed time — and not the
 * result set, so a grid must accumulate what it is streamed rather than waiting
 * for a copy at the end. A `notice` is neither a refusal nor a failure: it is
 * the guard saying what it thinks this statement is (an allowed write, for
 * instance) while the statement runs.
 *
 * The submitted text is guarded and run as **one** statement at index 0: there
 * is no splitter, because cutting a script on `;` would split a string literal
 * or a `BEGIN … END` block and send the pieces.
 */
export function sqlExecute(
  queryId: string,
  connectionId: string,
  sql: string,
  onEvent: (event: SqlEvent) => void,
): Promise<void> {
  const channel = new Channel<SqlEvent>();
  streaming(channel, onEvent, isSqlTerminal);
  return invoke<void>("sql_execute", { queryId, connectionId, sql, channel });
}

/**
 * Ask a running statement to stop reading.
 *
 * **Not a server-side cancel.** It stops this side reading and drops the
 * connection; the server may still be executing the statement. `notFound` means
 * nothing is running under that id — ordinary when a Stop click races a
 * statement that has just finished, and not an error.
 */
export const sqlCancel = (queryId: string) =>
  invoke<SqlStopOutcome>("sql_cancel", { queryId });

/** Tauri returns command errors as plain strings. */
export function errorMessage(error: unknown): string {
  if (typeof error === "string") return error;
  if (error instanceof Error) return error.message;
  return String(error);
}

// --- The embedded browser panel --------------------------------------------
//
// Every one of these is `async` on the Rust side and must stay so: the host
// reaches a main-thread-affine wry `WebView` through `run_on_main_thread`, and
// Tauri warns that webview creation deadlocks from a synchronous command on
// Windows. See `src-tauri/src/browser/mod.rs`.
//
// Every call is scoped by `root` — the workspace the page belongs to. The
// browser is per-codebase now (each open codebase keeps its own live page, only
// the active one visible), so the caller names which codebase's page it means.

/**
 * Open the page at `rect`, optionally navigating straight to `url` (raw user
 * input — the backend normalises and may refuse it). Creates the webview, or
 * re-places and re-shows one that was only minimized.
 */
export const browserOpen = (root: string, rect: BrowserRect, url: string | null) =>
  invoke<BrowserSnapshot>("browser_open", { root, rect, url });

/**
 * Close the panel, **dropping** the webview and its WebView2 process tree.
 * `pluginDisabled` says why: "the user switched the browser off" and "the panel
 * is closed" are one click apart and an agent acts on the difference.
 */
export const browserClose = (root: string, pluginDisabled: boolean) =>
  invoke<BrowserSnapshot>("browser_close", { root, pluginDisabled });

/** Move and resize the page to follow the panel. Logical pixels. */
export const browserSetBounds = (root: string, rect: BrowserRect) =>
  invoke<void>("browser_set_bounds", { root, rect });

/**
 * Show or hide the OS surface — the only mechanism that works. The page is a
 * child HWND compositing above the DOM, so a React `hidden` cannot hide it.
 */
export const browserSetVisible = (root: string, visible: boolean) =>
  invoke<void>("browser_set_visible", { root, visible });

/** Navigate to whatever the user typed. A search phrase is refused, not searched. */
export const browserNavigate = (root: string, input: string) =>
  invoke<BrowserSnapshot>("browser_navigate", { root, input });

export const browserBack = (root: string) => invoke<void>("browser_back", { root });
export const browserForward = (root: string) => invoke<void>("browser_forward", { root });
export const browserReload = (root: string) => invoke<void>("browser_reload", { root });

/** The whole readable state. A pure data read — no main thread, cheap to poll. */
export const browserState = (root: string) =>
  invoke<BrowserSnapshot>("browser_state", { root });

/** Console rows after `cursor`, with what the cursor missed. */
export const browserConsole = (root: string, cursor: number) =>
  invoke<BrowserConsoleBatch>("browser_console", { root, cursor });

/** Network rows after `cursor`, with the coverage note. */
export const browserNetwork = (root: string, cursor: number) =>
  invoke<BrowserNetworkBatch>("browser_network", { root, cursor });

/** The page's rendered text. Refused unless the page is `ready`. */
export const browserPageText = (root: string) =>
  invoke<BrowserPageText>("browser_page_text", { root });

/** Record consent the user gave or withdrew. The only thing that moves it. */
export const browserSetAutomationConsent = (root: string, reads: boolean, writes: boolean) =>
  invoke<BrowserSnapshot>("browser_set_automation_consent", { root, reads, writes });

// --- The browser MCP server's installer ------------------------------------
//
// The same five-call shape as the SQL server's (`mcpServerStatus` and friends)
// because it is the same machinery: `cb_core::browser::install` reuses
// `mcp::install`'s merge rather than growing a second one. Separate calls
// rather than a `kind` parameter, because the two servers carry different
// caveats and the failure mode of one shared call is showing somebody the
// database warning before granting an agent their browser session.

/** Where the browser MCP server is installed for `provider`, if anywhere. */
export const browserMcpStatus = (provider: ProviderId) =>
  invoke<InstallScope | null>("browser_mcp_status", { provider });

/** Exactly what installing it would write. Touches nothing. */
export const browserMcpInstallPlan = (provider: ProviderId, scope: InstallScope) =>
  invoke<InstallPlan>("browser_mcp_install_plan", { provider, scope });

/** Perform an install the user has confirmed; returns the new status. */
export const installBrowserMcp = (provider: ProviderId, scope: InstallScope) =>
  invoke<InstallScope | null>("install_browser_mcp", { provider, scope });

/** Exactly what removing it would rewrite. A zero-write plan means nothing of
 * ours was there. */
export const browserMcpUninstallPlan = (provider: ProviderId, scope: InstallScope) =>
  invoke<InstallPlan>("browser_mcp_uninstall_plan", { provider, scope });

/** Perform a removal the user has confirmed; returns the new status. */
export const uninstallBrowserMcp = (provider: ProviderId, scope: InstallScope) =>
  invoke<InstallScope | null>("uninstall_browser_mcp", { provider, scope });

// --- The Roslyn / LSP MCP server's installer -------------------------------
//
// The same five-call shape as the SQL and browser servers because it is the
// same machinery: `cb_core::roslyn::install` reuses `mcp::install`'s merge
// rather than growing a second one. Separate calls rather than a `kind`
// parameter, because the three servers carry different caveats and the failure
// mode of one shared call is showing somebody the wrong warning before install.

/** Where the Roslyn MCP server is installed for `provider`, if anywhere. */
export const roslynMcpStatus = (provider: ProviderId) =>
  invoke<InstallScope | null>("roslyn_mcp_server_status", { provider });

/** Exactly what installing it would write. Touches nothing. */
export const roslynMcpInstallPlan = (provider: ProviderId, scope: InstallScope) =>
  invoke<InstallPlan>("roslyn_mcp_server_install_plan", { provider, scope });

/** Perform an install the user has confirmed; returns the new status. */
export const installRoslynMcp = (provider: ProviderId, scope: InstallScope) =>
  invoke<InstallScope | null>("install_roslyn_mcp_server", { provider, scope });

/** Exactly what removing it would rewrite. A zero-write plan means nothing of
 * ours was there. */
export const roslynMcpUninstallPlan = (provider: ProviderId, scope: InstallScope) =>
  invoke<InstallPlan>("roslyn_mcp_server_uninstall_plan", { provider, scope });

/** Perform a removal the user has confirmed; returns the new status. */
export const uninstallRoslynMcp = (provider: ProviderId, scope: InstallScope) =>
  invoke<InstallScope | null>("uninstall_roslyn_mcp_server", { provider, scope });

// --- The Editor context MCP server -----------------------------------------
//
// The twin of the Roslyn installer above — the same five-call shape over the
// same `mcp::install` merge — plus one thing no other MCP server has: a push.
// Editor state lives in the React frontend, so `setEditorContext` feeds it into
// the backend (exactly as the browser's automation consent is fed), and the
// per-workspace shim reads it back. The command only records; the feature-off
// gate is the frontend not calling it plus the pipe host re-checking the feature.

/**
 * Push the live editor state for `root` into the backend.
 *
 * Called debounced, and **only while the `editorContextMcp` feature is on** — the
 * caller (`RunView`) is one half of the privacy gate; the pipe host re-checking
 * the feature before serving is the other.
 */
export const setEditorContext = (root: string, ctx: EditorContext) =>
  invoke<void>("set_editor_context", { root, ctx });

/** Where the Editor context MCP server is installed for `provider`, if anywhere. */
export const editorMcpStatus = (provider: ProviderId) =>
  invoke<InstallScope | null>("editor_mcp_server_status", { provider });

/** Exactly what installing it would write. Touches nothing. */
export const editorMcpInstallPlan = (provider: ProviderId, scope: InstallScope) =>
  invoke<InstallPlan>("editor_mcp_server_install_plan", { provider, scope });

/** Perform an install the user has confirmed; returns the new status. */
export const installEditorMcp = (provider: ProviderId, scope: InstallScope) =>
  invoke<InstallScope | null>("install_editor_mcp_server", { provider, scope });

/** Exactly what removing it would rewrite. A zero-write plan means nothing of
 * ours was there. */
export const editorMcpUninstallPlan = (provider: ProviderId, scope: InstallScope) =>
  invoke<InstallPlan>("editor_mcp_server_uninstall_plan", { provider, scope });

/** Perform a removal the user has confirmed; returns the new status. */
export const uninstallEditorMcp = (provider: ProviderId, scope: InstallScope) =>
  invoke<InstallScope | null>("uninstall_editor_mcp_server", { provider, scope });

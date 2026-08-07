/**
 * TypeScript mirrors of the `cb-core` model types.
 *
 * These correspond one-for-one with the Rust structs in
 * `crates/core/src/model.rs`, `workspace.rs` and `git/`. The Rust side derives
 * `serde(rename_all = "camelCase")`, which is what makes the field names below
 * match.
 *
 * A Rust test (`serialisation_shape` in `crates/core/src/model.rs`) asserts the
 * exact JSON keys each type produces, so renaming a Rust field fails there and
 * points here.
 */

// ---------------------------------------------------------------------------
// Projects and configurations
// ---------------------------------------------------------------------------

export type ProjectKind = "executable" | "library" | "test" | "unknown";

export type TestRunner =
  | "vsTest"
  | "microsoftTestingPlatform"
  | "vitest"
  | "jest"
  | "custom";

export type RunKind = "app" | "test";

/** A build-system action on a .NET project (`adapters/dotnet.rs`). */
export type BuildAction = "build" | "rebuild" | "clean";

export type ConfigSource = "detected" | "userFile" | "riderImport";

export interface Project {
  id: string;
  name: string;
  manifestPath: string;
  dir: string;
  ecosystem: string;
  kind: ProjectKind;
  frameworks: string[];
  /**
   * Build configurations the project offers — `Debug`/`Release` plus anything
   * a .NET project adds via `<Configurations>`. Empty for ecosystems with no
   * such concept.
   */
  configurations: string[];
  isTestProject: boolean;
  testRunner: TestRunner | null;
}

/**
 * One profile from a .NET project's `Properties/launchSettings.json`
 * (`adapters/dotnet.rs`).
 */
export interface LaunchProfile {
  name: string;
  /** `Project`, `IISExpress`, `Docker`, ... */
  commandName: string | null;
  args: string[];
  env: Record<string, string>;
  workingDirectory: string | null;
  applicationUrl: string | null;
  /**
   * Whether `dotnet run --launch-profile` can apply this profile. Only
   * `Project` profiles can; the rest are listed but cannot be selected.
   */
  launchable: boolean;
}

/** A .NET solution and the projects it groups (`adapters/solution.rs`). */
export interface Solution {
  name: string;
  /** Workspace-relative path to the `.sln` / `.slnx`. */
  path: string;
  projects: SolutionProject[];
}

export interface SolutionProject {
  name: string;
  /** Workspace-relative path to the project file, with forward slashes. */
  path: string;
  /** Solution folder path (`src/core`), or absent at the solution root. */
  folder?: string;
}

export interface RunConfig {
  id: string;
  name: string;
  kind: RunKind;
  ecosystem: string;
  source: ConfigSource;
  project?: string;
  buildConfiguration?: string;
  framework?: string;
  launchProfile?: string;
  /**
   * Skip launchSettings.json entirely (`--no-launch-profile`). When absent
   * and no profile is named, `dotnet run` applies its default profile.
   */
  ignoreLaunchSettings?: boolean;
  script?: string;
  args?: string[];
  env?: Record<string, string>;
  cwd?: string;
  warnings?: string[];
  /** Member config ids; non-empty only for compounds (ecosystem "compound"). */
  compound?: string[];
}

export interface Workspace {
  root: string;
  name: string;
  projects: Project[];
  configs: RunConfig[];
  /** .NET solutions found in the workspace, for grouping projects. */
  solutions: Solution[];
  /** Ids of starred configs; they sort before everything else. */
  favorites: string[];
  /** The user's preferred config ordering, as config ids. */
  order: string[];
}

/** One entry in a directory listing (`crates/core/src/files.rs`). */
export interface DirEntry {
  name: string;
  /** Workspace-relative path, usable directly in `fsReadFile`/`fsListDir`. */
  path: string;
  isDir: boolean;
}

/** A .NET project's user secrets (`crates/core/src/secrets.rs`). */
export interface ProjectSecrets {
  /** The project's `<UserSecretsId>`, when it has one. */
  secretsId: string | null;
  /** Absolute path of the `secrets.json` the id resolves to. */
  path: string | null;
  /** Contents of that file, when it exists. */
  content: string | null;
}

// ---------------------------------------------------------------------------
// Test results
// ---------------------------------------------------------------------------

export type TestOutcome = "passed" | "failed" | "skipped" | "other";

export interface TestCase {
  id: string;
  name: string;
  fullName: string;
  suite: string | null;
  project: string | null;
  outcome: TestOutcome;
  durationMs: number | null;
  message: string | null;
  stackTrace: string | null;
  stdout: string | null;
}

export interface TestSummary {
  total: number;
  passed: number;
  failed: number;
  skipped: number;
  other: number;
}

export interface TestRunResult {
  summary: TestSummary;
  cases: TestCase[];
  durationMs: number | null;
}

export interface TestNode {
  id: string;
  label: string;
  outcome: TestOutcome;
  summary: TestSummary;
  durationMs: number | null;
  case: TestCase | null;
  children: TestNode[];
}

export interface TestRunOutcome {
  result: TestRunResult;
  tree: TestNode[];
  warnings: string[];
  exitCode: number | null;
}

// ---------------------------------------------------------------------------
// Process events
// ---------------------------------------------------------------------------

export type Stream = "stdout" | "stderr";

export type ProcessEvent =
  | { type: "started"; pid: number | null; program: string; args: string[]; cwd: string }
  | { type: "output"; stream: Stream; text: string }
  | { type: "exited"; code: number | null; success: boolean; durationMs: number; cancelled: boolean }
  | { type: "failed"; message: string };

// ---------------------------------------------------------------------------
// Git
// ---------------------------------------------------------------------------

/**
 * Which two states a diff compares. This also decides what "revert" restores:
 * in `workingToIndex` a reverted line returns to its staged state, not to HEAD.
 */
export type ComparisonMode = "workingToHead" | "workingToIndex" | "indexToHead";

export type ChangeKind =
  | "added"
  | "modified"
  | "deleted"
  | "renamed"
  | "copied"
  | "typeChange"
  | "untracked"
  | "conflicted"
  | "unknown";

export interface FileChange {
  path: string;
  oldPath: string | null;
  staged: ChangeKind | null;
  unstaged: ChangeKind | null;
  isBinary: boolean;
}

export interface WorkingStatus {
  branch: string | null;
  upstream: string | null;
  ahead: number;
  behind: number;
  files: FileChange[];
  inProgressOperation: string | null;
}

export type LineOrigin = "context" | "addition" | "deletion";

export interface DiffLine {
  index: number;
  origin: LineOrigin;
  content: string;
  oldLineno: number | null;
  newLineno: number | null;
  noNewline: boolean;
}

export interface Hunk {
  oldStart: number;
  oldLines: number;
  newStart: number;
  newLines: number;
  header: string;
  lines: DiffLine[];
}

export interface FileDiff {
  path: string;
  oldPath: string | null;
  hunks: Hunk[];
  isBinary: boolean;
}

export interface FileContents {
  baseline: string | null;
  working: string | null;
}

export interface Branch {
  name: string;
  isHead: boolean;
  isRemote: boolean;
  upstream: string | null;
}

/**
 * A named group of working-tree files (`changelists.rs`).
 *
 * Git has no equivalent, so this is local bookkeeping stored in
 * `.code-basics/changelists.json` (gitignored). A file belongs to at most one
 * group.
 */
export interface Changelist {
  name: string;
  /** Workspace-relative paths with forward slashes, as `git status` reports. */
  paths: string[];
}

export interface Changelists {
  version: number;
  /** Groups in display order. */
  groups: Changelist[];
}

/** How a merge ended (`git/repo.rs`). */
export type MergeOutcome =
  | "upToDate"
  | "fastForward"
  | "merged"
  | "conflicted";

export interface MergeReport {
  outcome: MergeOutcome;
  /** Resulting commit, for a fast-forward or a merge commit. */
  commit?: string;
  /**
   * Paths left conflicted when `outcome` is `"conflicted"`. The merge is still
   * in progress — resolve in the Changes tab, or call `gitAbortMerge`.
   */
  conflicts?: string[];
}

export interface Commit {
  id: string;
  shortId: string;
  summary: string;
  body: string | null;
  authorName: string;
  authorEmail: string;
  time: number;
}

export type NetworkKind = "fetch" | "pull" | "push" | "pushSetUpstream";

export interface RiderImportPreview {
  configs: RunConfig[];
  /** `[name, riderType]` pairs that were recognised but cannot be launched. */
  skipped: [string, string][];
}

// ---------------------------------------------------------------------------
// Agent intent (`intents/`, `git/attribution.rs`, `git/grouping.rs`)
// ---------------------------------------------------------------------------

/** Which coding agent recorded an edit. */
export type ProviderId = "claudeCode" | "codex";

/** Where an agent's hook configuration is written. */
export type InstallScope = "project" | "user";

/** How much to trust an attribution (`git/attribution.rs`). */
export type Confidence = "low" | "medium" | "high";

/** What each agent can currently do for this workspace. */
export interface ProviderStatus {
  provider: ProviderId;
  /** The agent appears to be installed on this machine. */
  detected: boolean;
  /** Where our hooks are configured, if they are. */
  capture: InstallScope | null;
  /** Past sessions found for this workspace. */
  sessions: number;
  /**
   * Anything standing between this configuration and actual records — an
   * untrusted Codex project, a hook awaiting review, skipped compressed
   * sessions. Shown to the user rather than swallowed.
   */
  caveats?: string[];
}

/** One file an install would create or change. */
export interface PlannedWrite {
  path: string;
  /** The file's full contents afterwards. */
  content: string;
  /** True when merging into a file that already exists — the case that matters. */
  mergesExisting: boolean;
}

/** Everything enabling capture would do, computed without touching disk. */
export interface InstallPlan {
  provider: ProviderId;
  scope: InstallScope;
  writes: PlannedWrite[];
  caveats?: string[];
}

/** Why a set of hunks belongs together (`git/grouping.rs`). */
export type GroupKind =
  | "intent"
  | "formatting"
  | "newSymbol"
  | "modifiedSymbol"
  | "other";

/**
 * The lines of one file belonging to a group.
 *
 * `lineIndices` are `DiffLine.index` values and are only meaningful for the
 * comparison mode the group was built in — which is why group actions name
 * the group and let Rust re-derive them, rather than sending them back.
 */
export interface GroupFile {
  path: string;
  lineIndices: number[];
  /** Indices into the file's `FileDiff.hunks`. */
  hunks: number[];
}

/** One card in the Changes tab. */
export interface IntentGroup {
  id: string;
  kind: GroupKind;
  label: string;
  /** The symbol the group sits in, when one was identified. */
  symbol?: string;
  files: GroupFile[];
  /** Total changed lines across every file. */
  lineCount: number;
  /** The weakest confidence of any hunk in the group. */
  confidence: Confidence;
}

// ---------------------------------------------------------------------------
// Object inspection (`crates/core/src/inspect/model.rs`)
// ---------------------------------------------------------------------------

export type Bitness = "x64" | "x86";

/** What was inspected: a crash dump on disk, or a running process. */
export type InspectTarget =
  | { kind: "dump"; path: string }
  | { kind: "live"; pid: number };

export interface TargetSummary {
  target: InspectTarget;
  bitness?: Bitness;
  runtimeVersion?: string;
  processName?: string;
}

/** How much of the graph the inspector was allowed to walk. */
export interface Caps {
  maxDepth: number;
  maxChildren: number;
  maxStringLength: number;
  maxNodes: number;
}

export type ElidedReason = "depthLimit" | "childLimit" | "nodeLimit";

/**
 * What a field holds.
 *
 * Two variants exist to admit ignorance and must be rendered as such:
 * `elided` means "there is more here and the walk stopped"; `unavailable`
 * means "this could not be read". Neither may be shown as a value — a field
 * displayed as `0` that was never actually read is the failure this whole
 * feature is designed against, because the user believes it.
 *
 * `address` is a hex string, never a number: it is the identity used to expand
 * a node and to spot a cycle, and a rounded address points at the wrong object.
 */
export type ObjectValue =
  | { kind: "primitive"; text: string }
  | { kind: "text"; text: string; truncated: boolean }
  | { kind: "null" }
  | { kind: "reference"; address: string; typeName: string; expandable: boolean }
  /** Already shown at `path`; a leaf, so rendering never recurses. */
  | { kind: "cycle"; address: string; path: string }
  | { kind: "elided"; reason: ElidedReason }
  | { kind: "unavailable"; reason: string };

/** One row in the object tree. Shaped like `TestNode`, rendered the same way. */
export interface InspectNode {
  /** Path-shaped and stable, e.g. `root.orders[3]._total`. */
  id: string;
  label: string;
  typeName?: string;
  value: ObjectValue;
  children: InspectNode[];
  /** True when `children` is a prefix of what exists. */
  hasMore: boolean;
  /** Turns "100 items" into "100 of 5,412" when the inspector could count. */
  childCountTotal?: number;
}

export interface InspectGraph {
  sessionId: string;
  /**
   * Identifies this snapshot. Expanding past a cap on a live process yields a
   * new one, which is the signal to warn that a branch may not agree with the
   * rest of the tree.
   */
  snapshotId: string;
  capturedAt: string;
  target: TargetSummary;
  roots: InspectNode[];
  caps: Caps;
  warnings?: string[];
}

/**
 * Where to start walking.
 *
 * There is deliberately no "find the interesting object" option — a live heap
 * holds millions, and a heuristic that picked one would mislead often enough
 * to be worse than asking.
 */
export type RootSpec =
  | { kind: "type"; name: string; limit: number }
  | { kind: "statics"; name: string }
  | { kind: "address"; address: string }
  /** Every live `System.Exception` — the best-effort answer for a caught one. */
  | { kind: "exceptions" }
  /** The exception that killed the process. Dump targets only. */
  | { kind: "crashException" };

export interface InspectRequest {
  schemaVersion: number;
  target: InspectTarget;
  root: RootSpec;
  caps: Caps;
  /** Suspends the user's application while capturing. Opt-in. */
  suspend: boolean;
}

/** A crash dump under `.code-basics/dumps/`. */
export interface DumpFile {
  path: string;
  /** From `%e` in the filename template; matches a dump to the run. */
  executable: string;
  pid: number;
  /** Unix seconds, from `%t`. */
  capturedAt: number;
  bytes: number;
}

/**
 * One .NET process the machine is running, as the inspector enumerated it.
 *
 * Only `pid` and `name` are guaranteed; the rest is read through APIs that
 * legitimately fail on another user's process and is omitted rather than
 * filled in when it could not be.
 */
export interface DotnetProcess {
  pid: number;
  /** Executable name without its extension. */
  name: string;
  path?: string;
  parentPid?: number;
  /** ISO-8601, exactly as the enumerator wrote it. */
  startedAt?: string;
}

/**
 * How a process was linked to a run configuration.
 *
 * `descendant` is the one that matters: `dotnet run` starts the application as
 * a child, so the pid code-basics launched is the CLI and the child is where
 * the user's objects are.
 */
export type Attribution = "launched" | "descendant" | "unrelated";

/**
 * A .NET process that can be attached to, and what is known about whose it is.
 *
 * Every published .NET process on the machine appears, including ones
 * code-basics never started — so `attribution` is what a view must branch on
 * before it says anything about ownership. An empty list is a normal answer.
 */
export interface AttachableProcess {
  pid: number;
  name: string;
  path?: string;
  attribution: Attribution;
  /**
   * The run configuration this process belongs to. Present only when
   * `attribution` is `launched` or `descendant` — an unrelated process must
   * never be shown under one of the user's configuration names.
   */
  configId?: string;
  configName?: string;
  /**
   * Whether the backend has *evidence* this process holds the application's own
   * objects: it is the pid code-basics launched and nothing marks it as a
   * launcher, or it is the single child a launcher could be said to have
   * started as the application.
   *
   * `false` means no evidence, not "this is not the application" — a
   * `descendant` that could equally be an MSBuild worker node or the compiler
   * server is false. So a view may preselect a `true` and must never preselect
   * a `false` as though it were one.
   */
  isApplication: boolean;
  /**
   * Why this pid is not the process holding the user's objects — absent when
   * there is no evidence that it is anything but the application itself.
   *
   * Present when the process is demonstrably a launcher: something it started
   * is on this same list, or it is the .NET CLI for a `dotnet run`
   * configuration whose application has not appeared yet. Capturing it finds
   * none of the user's types and renders an empty tree that reads like "your
   * object is not there", so this must be shown wherever the pid is offered.
   */
  launcherCaveat?: string;
}

/**
 * The attachable processes, and anything that stopped the list being complete.
 *
 * A `warnings` entry means the list is real but degraded — most importantly,
 * that no process's parent could be read, which leaves every row `unrelated`
 * and so indistinguishable from a machine running nothing of the user's. A list
 * that could not be produced at all is a rejected promise instead, because
 * "nothing is attachable" and "I could not look" are different answers.
 */
export interface AttachableList {
  processes: AttachableProcess[];
  warnings?: string[];
}

/** A dump a finished run may have written, and whether it is certainly its. */
export interface RunDump {
  dump: DumpFile;
  /**
   * True only when the dump carries the pid the run reported. False means it is
   * a candidate written while the run was going — possibly by another
   * configuration — and must never be described as this run's crash.
   */
  certain: boolean;
}

export interface InspectStatus {
  available: boolean;
  unavailableReason?: string;
  dumpCaptureEnabled: boolean;
  /** Newest first. */
  dumps: DumpFile[];
  caveats?: string[];
  /**
   * What attaching to a running process costs it, on its own so a view that
   * offers an attach button can state it beside that button. Also contained in
   * `caveats`, which only the Objects tab shows.
   */
  attachCaveats?: string[];
}

/** Per-workspace inspector settings in `.code-basics/config.json`. */
export interface InspectorConfig {
  captureDumps: boolean;
  caps?: Caps;
  keepDumps?: number;
  maxDumpMegabytes?: number;
  env?: Record<string, string>;
}

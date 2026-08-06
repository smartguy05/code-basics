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

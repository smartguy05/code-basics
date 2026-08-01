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

export type ConfigSource = "detected" | "userFile" | "riderImport";

export interface Project {
  id: string;
  name: string;
  manifestPath: string;
  dir: string;
  ecosystem: string;
  kind: ProjectKind;
  frameworks: string[];
  isTestProject: boolean;
  testRunner: TestRunner | null;
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
  script?: string;
  args?: string[];
  env?: Record<string, string>;
  cwd?: string;
  warnings?: string[];
}

export interface Workspace {
  root: string;
  name: string;
  projects: Project[];
  configs: RunConfig[];
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

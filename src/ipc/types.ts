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
  /**
   * Why this project could not be fully read — a manifest that will not parse,
   * or a file that could not be opened.
   *
   * **Optional, not nullable**: the Rust side is `#[serde(default,
   * skip_serializing_if = "Option::is_none")]`, so a healthy project has no
   * `unreadable` key at all rather than `unreadable: null`.
   *
   * A project carrying a reason is listed but inert — no configurations, no
   * frameworks, `kind` is `"unknown"` — and should be shown greyed out with the
   * reason rather than hidden. Dropping it is what the scan used to do, and a
   * shorter list looks exactly like a correct one.
   */
  unreadable?: string;
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

/**
 * Events from an interactive terminal session (`cb_core::pty`). Deliberately
 * not {@link ProcessEvent}: a PTY merges what would be stdout and stderr onto
 * one stream (no `stream` field), and the shell prints its own prompt (no
 * `started` banner). Mirrors the Rust `TerminalEvent`, pinned by
 * `crates/core/src/pty/model.rs`.
 */
export type TerminalEvent =
  | { type: "output"; text: string }
  | { type: "exited"; code: number | null; success: boolean }
  | { type: "failed"; message: string };

// ---------------------------------------------------------------------------
// Running processes (the Running panel)
// ---------------------------------------------------------------------------

/**
 * What kind of process a running record describes. Mirrors the Rust
 * `running::RunKind`, pinned by `crates/core/src/running/record.rs`. Named
 * `ProcessKind` here because `RunKind` ("app" | "test") is already taken by the
 * run-configuration kind above.
 */
export type ProcessKind =
  | "run"
  | "build"
  | "terminal"
  | "review"
  | "behavioral"
  | "external";

/**
 * One process the app has running (or an orphan candidate). Mirrors the Rust
 * `RunningRecord`. `key` is the handle the app addresses a live process by (the
 * supervisor config id, or the PTY session id); `root` is the workspace it
 * belongs to; `program` + `startedAtMs` are the identity used to guard against
 * killing a reused pid.
 */
export interface RunningRecord {
  pid: number;
  kind: ProcessKind;
  label: string;
  root: string;
  key: string;
  program: string;
  startedAtMs: number;
}

/** What `list_running` returns. Mirrors the Rust `RunningReport`. */
export interface RunningReport {
  live: RunningRecord[];
  orphans: RunningRecord[];
  warnings: string[];
}

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

/**
 * One entry in the stash list (`repo.rs::StashEntry`). A stash is stored as a
 * commit, so `id` feeds the ordinary `gitCommitDiff`/`gitCommitFileContents`
 * preview path.
 */
export interface StashEntry {
  index: number;
  id: string;
  message: string;
  branch: string | null;
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

/** Which coding agent recorded an edit — or the user, for a note they wrote. */
export type ProviderId = "claudeCode" | "codex" | "user";

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

/**
 * One instruction template in the Enhancements menu (`enhancements::EnhancementInfo`).
 */
export interface EnhancementInfo {
  id: string;
  title: string;
  /** Present in CLAUDE.md or AGENTS.md in the current workspace. */
  installed: boolean;
}

/** One prompt in the Enhancements → Run Agent submenu (`enhancements::PromptInfo`). */
export interface PromptInfo {
  id: string;
  title: string;
  /**
   * Declared run-once (`once: true`): the menu records a successful run per
   * workspace and confirms before re-running.
   */
  once: boolean;
  /** The prompt text run as an agent (front matter stripped). */
  body: string;
}

/** One prompt's last successful run in this workspace (`enhancements::runs::PromptRun`). */
export interface PromptRun {
  /** When the run finished, in milliseconds since the Unix epoch. */
  lastRunAtMs: number;
}

/** Run-once records keyed by prompt id (`enhancements::runs::PromptRuns`). */
export type PromptRuns = Record<string, PromptRun>;

/**
 * One note in the global scratchpad (`notes::Note`). The key names are pinned by
 * `serialisation_shape_pins_the_wire_keys` in `crates/core/src/notes_tests.rs`.
 */
export interface Note {
  id: string;
  title: string;
  body: string;
  /** When the note was created, milliseconds since the Unix epoch. */
  createdAtMs: number;
  /** When the note was last edited, milliseconds since the Unix epoch. */
  updatedAtMs: number;
}

/** The whole global notes file (`notes::NotesFile`). */
export interface NotesFile {
  /** Schema version (currently 1), so the format can migrate. */
  version: number;
  /** The notes, in the order the panel shows their tabs. */
  notes: Note[];
}

/**
 * One remembered command line from the app launcher (`launcher::Launchable`).
 * Keys pinned by `launchable_serialises_with_camel_case_keys` in
 * `crates/core/src/launcher/model_tests.rs`. `label` is deliberately nullable
 * rather than optional: "never renamed" is a state the picker reads.
 */
export interface Launchable {
  id: string;
  /** The command line exactly as the user typed it. */
  command: string;
  /** Where it runs. Part of a recent's identity, and the grouping key. */
  cwd: string;
  env: Record<string, string>;
  /** The user's rename, or null to show the command itself. */
  label: string | null;
  /** Run through the default shell (needed for `|`, `>`, `&&`). */
  shell: boolean;
  /** Pinned entries sort first and are never evicted by the recents cap. */
  pinned: boolean;
  /** When it last ran, ms since the Unix epoch. */
  lastRunMs: number;
  runCount: number;
}

/** The whole launchers file (`launcher::LauncherFile`). */
export interface LauncherFile {
  version: number;
  entries: Launchable[];
}

/**
 * What the launcher picker renders (`launcher::LauncherGroups`): the open
 * codebase's commands, then everything else the user has run anywhere.
 */
export interface LauncherGroups {
  thisCodebase: Launchable[];
  global: Launchable[];
}

/** What a launch hands back (`commands::launcher::LaunchedApp`). */
export interface LaunchedApp {
  /** The supervisor key: what stops it, and what the Running panel kills. */
  key: string;
  /** The recents entry id, for pin/rename/delete. */
  id: string;
  label: string;
  cwd: string;
}

/** An installed review agent (`commands::review::ReviewAgentInfo`). */
export interface ReviewAgentInfo {
  id: string;
  label: string;
  /** Model aliases the picker may offer; empty means the agent's own default. */
  models: string[];
}

/** Why a set of hunks belongs together (`git/grouping.rs`). */
export type GroupKind =
  | "intent"
  /**
   * One turn made these hunks but never said why. Grouped because they really
   * did change together; the label is derived from the changes, so it is a
   * description and must not be read as a reason.
   */
  | "sameTurn"
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

/**
 * How sure the agent said it was that its own change is correct, when it
 * appended a `[confidence: …]` token to the declared `Intent:` line.
 *
 * NOT `Confidence`: that is the matcher's trust that it located the recorded
 * edit in the diff — a property of the tooling. This is the agent's own stated
 * belief, offered voluntarily and therefore usually absent. `low` = "please
 * review this closely".
 */
export type SelfConfidence = "low" | "medium" | "high";

/** One card in the Changes tab. */
export interface IntentGroup {
  id: string;
  kind: GroupKind;
  /** What to show on the card; empty for an ambiguous intent (see `candidates`). */
  label: string;
  /**
   * When several declared reasons scope this file and none could be bound
   * uniquely, every candidate reason — shown instead of dropping the intent to a
   * symbol title. Absent/empty in the normal single-reason case.
   */
  candidates?: string[];
  /** The symbol the group sits in, when one was identified. */
  symbol?: string;
  files: GroupFile[];
  /** Total changed lines across every file. */
  lineCount: number;
  /** The weakest confidence of any hunk in the group. */
  confidence: Confidence;
  /**
   * The agent's own stated confidence for this card's declared reason
   * (distinct from `confidence` above — see {@link SelfConfidence}). Present
   * only when the agent appended a `[confidence: …]` token; absent otherwise,
   * so the confidence heatmap abstains rather than guessing a level.
   */
  selfConfidence?: SelfConfidence;
  /**
   * The label is a note the user wrote (not an agent reason). Present only when
   * true, so it is safe to read as falsy. Distinguishes editing your own note
   * from overwriting an agent's stated intent.
   */
  userAuthored?: boolean;
}

/**
 * A declared intent for which no changed hunk shows matching content
 * (`git/coverage.rs`).
 *
 * NOT an accusation: the edit may be committed, later overwritten,
 * hand-reverted, or transformed past the matcher's normalisation ladder.
 * Reported as unmatched, never as undone.
 */
export interface UnfulfilledClaim {
  turnId: string;
  /** The declared label's text. */
  label: string;
  provider: ProviderId;
  /** Files in this diff the claim's turn touched. */
  paths: string[];
}

/**
 * The per-turn tally shown above the cards (`git/coverage.rs`): the direct
 * answer to "did the agent do what it told me it did".
 */
export interface Scorecard {
  /** Declared intents whose turn edited a file in this diff. */
  claims: number;
  /** Claims evidenced by at least one matched change anywhere in the diff. */
  evidenced: number;
  /** Claims with no matching change — always equal to `unfulfilled.length`. */
  unmatched: number;
  /** Changed hunks across the tree. */
  hunks: number;
  /** Hunks with at least one attributed span. */
  attributedHunks: number;
  /** Changed lines across the tree that no record claimed. */
  unattributedLines: number;
}

/** Grouping plus the two coverage failures and the aggregate (`git/coverage.rs`). */
export interface IntentReview {
  groups: IntentGroup[];
  /**
   * The evidenced half of the claim universe — declared intents an accepted
   * span matches. Same row shape as `unfulfilled`, so one per-claim checklist
   * renders both; `evidenced.length === scorecard.evidenced`.
   */
  evidenced: UnfulfilledClaim[];
  unfulfilled: UnfulfilledClaim[];
  scorecard: Scorecard;
}

// ---------------------------------------------------------------------------
// Behavioral before/after testing (`behavioral/`) — the runtime counterpart to
// the static intent Scorecard above.
// ---------------------------------------------------------------------------

/** How one test case's outcome moved between the HEAD and working-tree runs. */
export type CaseTransition =
  | "unchanged"
  | "fixed"
  | "regressed"
  | "stillFailing"
  | "added"
  | "removed";

/** One test case's before/after outcome (`behavioral/compare.rs`). */
export interface CaseDelta {
  fullName: string;
  base: TestOutcome | null;
  work: TestOutcome | null;
  transition: CaseTransition;
  /** Source files this case plausibly exercises; filled in during attribution. */
  filesHint: string[];
}

/** Every case whose outcome changed, plus before/after summaries. */
export interface TestDelta {
  cases: CaseDelta[];
  summaryBefore: TestSummary;
  summaryAfter: TestSummary;
}

/** A difference in captured console output, after masking known noise. */
export interface ConsoleDelta {
  addedLines: string[];
  removedLines: string[];
  normalized: boolean;
  confidence: Confidence;
}

/** One header whose presence or value changed between the two responses. */
export interface HeaderChange {
  name: string;
  before: string | null;
  after: string | null;
}

/** A difference in a response body, after type-aware normalisation. */
export interface BodyDelta {
  addedLines: string[];
  removedLines: string[];
  normalized: boolean;
}

/** A difference between the HEAD and working-tree responses for one `.http` request. */
export interface HttpDelta {
  name: string;
  /** `[before, after]` status codes, only when they differ. */
  status: [number, number] | null;
  headerChanges: HeaderChange[];
  body: BodyDelta | null;
  confidence: Confidence;
}

/** One observable difference, internally tagged on `kind` (`behavioral/mod.rs`). */
export type BehavioralDelta =
  | ({ kind: "test" } & CaseDelta)
  | ({ kind: "console" } & ConsoleDelta)
  | ({ kind: "http" } & HttpDelta);

/** The behavioral deltas attributed to one intent card. */
export interface CardBehavior {
  groupId: string;
  deltas: BehavioralDelta[];
  confidence: Confidence;
}

/** The per-run tally shown beside the static {@link Scorecard}. */
export interface BehavioralScorecard {
  outcomesCompared: number;
  deltas: number;
  attributedDeltas: number;
  unattributedDeltas: number;
  abstained: number;
}

/** The whole before/after comparison — runtime twin of {@link IntentReview}. */
export interface BehavioralReport {
  tests: TestDelta | null;
  console: ConsoleDelta | null;
  http: HttpDelta[];
  attributions: CardBehavior[];
  unattributed: BehavioralDelta[];
  scorecard: BehavioralScorecard;
  warnings: string[];
}

/** Whether a label was declared by the agent or mined from its prose (`intents/`). */
export type LabelSource = "declared" | "inferred";

/**
 * The recorded reason behind one line of a past commit (`git/why.rs`).
 *
 * Content-keyed and durable: it survives reformatting and rebase because the
 * key is the line's normalised skeleton, never its position. Absent for any
 * line whose content matches no stored key — the History tab shows the empty
 * state rather than a guessed reason.
 */
export interface LineIntent {
  /** 1-based line number in the committed file. */
  line: number;
  label?: string;
  labelSource?: LabelSource;
  turnId: string;
  confidence: Confidence;
  /** The user prompt that caused it, when captured; currently always absent. */
  prompt?: string;
}

// ---------------------------------------------------------------------------
// Erosion detector (`erosion/`)
// ---------------------------------------------------------------------------

/** The kind of weakening the erosion scan detects (`erosion/rules.rs`). */
export type ErosionCategory =
  | "deletedAssertion"
  | "ignoredTest"
  | "widenedCatch"
  | "removedNullCheck"
  | "unsafeCast"
  | "leftoverStub"
  | "removedSafeguard"
  | "droppedLog"
  | "logDowngrade"
  | "secret"
  | "schemaRisk";

/** One located weakening the scan found (`erosion/scan.rs`). */
export interface ErosionFlag {
  path: string;
  /** Source line number, for display as `path:line`. */
  line: number;
  /** `DiffLine.index` of the offending line, for highlighting in the diff pane. */
  index: number;
  origin: LineOrigin;
  category: ErosionCategory;
  ruleId: string;
  message: string;
  /** The offending line, trimmed, for display. */
  content: string;
}

/**
 * Everything the erosion scan found (`erosion/scan.rs`).
 *
 * `warnings` carries rules whose TOML would not parse or whose regex would not
 * compile — surfaced to the user, never silently dropped.
 */
export interface ErosionReport {
  flags: ErosionFlag[];
  warnings: string[];
}

// ---------------------------------------------------------------------------
// Coverage of change (`crates/core/src/testing/changecov.rs`)
// ---------------------------------------------------------------------------

/** One changed line the test run never executed (`changecov.rs`). */
export interface UncoveredLine {
  /** The 1-based new-side source line number. */
  line: number;
  /** `DiffLine.index` of that line, the highlight anchor in the diff pane. */
  index: number;
}

/** Coverage of the changed lines in one file (`changecov.rs`). */
export interface FileChangeCoverage {
  /** The diff (working-copy) path these numbers describe. */
  path: string;
  /** The changed lines that were coverable but never executed. */
  uncovered: UncoveredLine[];
  /** How many changed lines were executed at least once. */
  coveredChanged: number;
  /** How many changed lines were coverable but never executed. */
  uncoveredChanged: number;
}

/**
 * Coverage of every changed line across the diff (`changecov.rs`).
 *
 * `changedLines` counts only the changed lines that could be *classified*
 * (covered + uncovered); a changed line the coverage tool considered
 * non-executable is abstained on and excluded. `warnings` carries files whose
 * coverage could not be matched — no match, or an ambiguous one — reported
 * rather than silently dropped.
 */
export interface ChangeCoverage {
  files: FileChangeCoverage[];
  changedLines: number;
  coveredLines: number;
  uncoveredLines: number;
  warnings: string[];
}

/**
 * One business-rule invariant, authored as a markdown file (`rules/mod.rs`).
 *
 * Prose for a human and for a reviewing agent — no pattern, matches nothing on
 * its own. Handed to a review as context (`start_review`'s `context`) so the
 * agent judges the diff against the rules the team stated.
 */
export interface RuleDoc {
  id: string;
  title: string;
  /** The markdown body, front matter stripped. */
  body: string;
}

/**
 * Every rule doc loaded from a workspace (`rules::RulesReport`).
 *
 * `warnings` carries any file that would not read — surfaced, never dropped.
 */
export interface RulesReport {
  rules: RuleDoc[];
  warnings: string[];
}

/**
 * What rejecting a group did (`intents/reject.rs`).
 *
 * `unmarked` is the field the UI must not swallow: those files were reverted,
 * but they have no line-comment syntax, so the reason the reviewer typed was
 * written nowhere and no agent will ever read it.
 */
export interface RejectSummary {
  reverted: number;
  /** Files that now carry the reason as a comment. */
  marked: string[];
  /** Files reverted without a note. */
  unmarked: string[];
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
  /** A dictionary entry: a container with no value of its own; its `Key` and
   * `Value` children carry everything. */
  | { kind: "pair" }
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
  /**
   * Full command line, when it could be read. The only place a `dotnet run`
   * child's assembly appears — its OS name is just `dotnet` — so it is what
   * separates the real application from the SDK's own build tools. Omitted when
   * unknown (reading another process's command line is refused across an
   * elevation or session boundary), which costs only preselection.
   */
  commandLine?: string;
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

// ---------------------------------------------------------------------------
// Search everywhere
// ---------------------------------------------------------------------------

/**
 * What a line of source appeared to declare
 * (`crates/core/src/symbols/declarations.rs`).
 *
 * The Rust enum derives `serde(rename_all = "camelCase")`, so each variant
 * crosses as its own name lowercased — every variant here is a single word, so
 * that is simply the lowercase spelling. Deliberately coarse: it is a badge in
 * a palette, not a type system, and `other` is the honest answer for everything
 * a one-line word scan cannot place. A row with `other` should draw **no**
 * badge rather than the word "other" — the scan abstained, and dressing an
 * abstention up as a classification is the wrong answer that module exists to
 * refuse.
 */
export type SymbolKind =
  | "function"
  | "class"
  | "struct"
  | "enum"
  | "interface"
  | "trait"
  | "type"
  | "namespace"
  | "constant"
  /**
   * A member with accessors. Only a real language server produces this — the
   * one-line word scan behind the palette cannot tell `public string Name
   * { get; set; }` from a variable, so it never claims one. LSP's `Field` (8)
   * deliberately stays `variable`: a field and a property are different things
   * in C#, TypeScript, Java and Kotlin alike.
   */
  | "property"
  | "variable"
  | "other";

/**
 * Which populations a query is allowed to match
 * (`crates/core/src/symbols/search.rs`).
 *
 * A scope filters and never reweights, so the rows of a scoped list appear in
 * the same relative order they would inside an `all` list.
 */
export type SearchScope = "all" | "files" | "symbols" | "actions";

/** Which of the three questions a hit answers. */
export type HitKind = "file" | "symbol" | "action";

/**
 * One row of the palette (`SearchHit` in `crates/core/src/symbols/search.rs`).
 *
 * **Every key is present on every hit**, `null` where a kind has no answer —
 * the Rust struct deliberately carries no `skip_serializing_if`, so these are
 * `T | null` and not optional `?`. The reason is stated in the Rust doc and
 * pinned by `search_hit_serialises_with_the_keys_the_ui_reads`: an absent key
 * would make "this kind has no line" indistinguishable from "the backend did
 * not send a line", and the second of those is a bug that would then be
 * invisible.
 *
 * A trailing `:123` on the query text means "line 123" and is parsed off in
 * Rust, once, by `split_line_suffix`. **Do not re-implement that here** — pass
 * the raw text through and read `line` off the hit. Two implementations of a
 * parse always disagree eventually, and this one decides where the editor
 * jumps.
 */
export interface SearchHit {
  kind: HitKind;
  /**
   * What the row shows, and the only string `positions` refers to: a file's
   * name, a symbol's name, a configuration's name.
   */
  label: string;
  /**
   * The secondary line: the workspace-relative path for a file or symbol, the
   * configuration's project for an action.
   */
  detail: string;
  /**
   * Workspace-relative with forward slashes (a Rust `PathBuf`, which crosses
   * as a plain string). `null` for an action, which opens nothing.
   */
  path: string | null;
  /**
   * 1-based, matching an editor's gutter. A symbol's declaration line, or the
   * line the query named explicitly, which wins. `null` on a file hit whose
   * query named no line, and always `null` on an action.
   */
  line: number | null;
  /** Present only on a symbol hit; the badge the UI draws. */
  symbolKind: SymbolKind | null;
  /** The `RunConfig` id to launch, present only on an action hit. */
  actionId: string | null;
  /**
   * **Character** indices into `label`, for highlighting — not byte offsets
   * and not UTF-16 code unit offsets. Rust counts them with `chars()`, which
   * walks Unicode scalar values, so `Array.from(label)` — which iterates code
   * points — indexes by exactly the same units and is the only correct way to
   * read them here. `searchLogic.ts`'s `highlightSpans` does that.
   *
   * Two wrong ways to read them, both of which look fine until a name is not
   * ASCII: a byte view (`TextEncoder`, a `Buffer`) shifts on any non-ASCII
   * character at all, and raw string indexing or `slice` shifts by one per
   * astral character (an emoji, some CJK extensions) and can cut a surrogate
   * pair in half, which renders as a replacement glyph. Decomposed properly
   * there is no residual skew of either kind: a Rust scalar value and a
   * JavaScript code point are the same unit, so emoji, CJK, combining marks
   * and ZWJ sequences all align exactly. Each side is pinned against a
   * non-ASCII label — `positions_are_char_indices_not_byte_indices` in
   * `symbols/fuzzy_tests.rs`, and the emoji case in `searchLogic.test.ts`.
   *
   *
   * Empty when the match was found somewhere the label does not show — a file
   * matched only on its path carries no positions rather than positions into a
   * string the row is not drawing.
   */
  positions: number[];
  /** Comparable only against other hits for the same query. */
  score: number;
}

/**
 * What the backend's symbol index currently holds.
 *
 * `ready` and `building` are separate booleans rather than one state, because
 * a rebuild happens over an index that is still answerable: `ready: true,
 * building: true` is the normal state during a refresh and the palette should
 * keep serving from what it has. `ready: false, building: true` is the only
 * state that justifies showing a spinner instead of results.
 *
 * `truncated` means a cap was hit and the counts below are therefore
 * incomplete. A view must say so — presenting a clipped index as if it were the
 * whole workspace is the wrong answer `cb-core` refuses to give, and this flag
 * is the only way the frontend can tell.
 *
 * `files` and `symbols` are Rust `usize`, which crosses as a JSON number.
 */
export interface SymbolIndexStatus {
  ready: boolean;
  building: boolean;
  files: number;
  symbols: number;
  truncated: boolean;
}

// ---------------------------------------------------------------------------
// Architecture diagrams (`crates/core/src/architecture/`)
// ---------------------------------------------------------------------------

/**
 * What a box in the diagram stands for.
 *
 * `external` is something referenced from inside the workspace that lives
 * outside it — there is no `Project` behind it, because the scan never saw it.
 * `solutionFolder` exists only inside a `.sln` file and nowhere on disk, which
 * is why such a node carries no `path`.
 *
 * `service` and `dataStore` only ever appear in a *component* map
 * (`architecture::components`), never in a project map. A `service` is a
 * project that also declares it serves HTTP, and carries the same `id`,
 * `projectId`, `path` and `ecosystem` a `project` node would — code that does
 * not care about the distinction can treat the two alike. A `dataStore` is a
 * database, cache or broker some project declares a client for; it carries no
 * `projectId`, no `path` and no `ecosystem`, because it is not in this
 * workspace and there is nothing to open.
 */
export type ArchKind =
  | "project"
  | "solution"
  | "solutionFolder"
  | "external"
  | "service"
  | "dataStore";

/**
 * What an arrow asserts about its two endpoints.
 *
 * `contains` is membership and nothing else: a solution, a solution folder or
 * an npm workspace root holding a project. It says which things ship together
 * and is deliberately silent about which needs which — reading a dependency
 * out of a `.sln` would be inventing one out of filing.
 *
 * `dataAccess` runs project → `dataStore` and appears only in a component map.
 * It asserts that the project's manifest names a client library for that
 * technology — capability, not runtime use, and never a shared instance.
 *
 * `serviceCall` runs service → service and appears only in a component map. It
 * asserts that the caller's `AddHttpClient` registration wrote a literal base
 * address matching exactly one other project's `launchSettings.json`
 * `applicationUrl`. The caller's address is read from source, but the callee's
 * identity rests on that declaration file, which is what lets the arrow be
 * drawn; it is only ever drawn between two boxes that already exist.
 */
export type EdgeKind =
  | "projectReference"
  | "packageDependency"
  | "contains"
  | "dataAccess"
  | "serviceCall";

/**
 * Where a graph came from, and therefore how much of it can be trusted.
 *
 * A Rust enum with data, which serde tags **externally**: the two variants
 * that carry a payload cross as a single-key object, and the one that does not
 * crosses as a bare string. Pinned by
 * `an_arch_graph_serialises_with_the_keys_the_ui_reads` in
 * `architecture/graph_tests.rs`, which asserts exactly:
 *
 * ```json
 * { "derived": { "scanner": 1 } }
 * { "inferred": { "agent": "claude" } }
 * "user"
 * ```
 *
 * Narrow it with `typeof d === "string"` first, then `"derived" in d` — there
 * is no discriminant field to switch on, and assuming one (`kind`, `type`) is
 * the shape this comment exists to stop being assumed.
 *
 * The three cases are kept apart because they fail differently. `derived` is
 * reproducible and can only be wrong if the rules are; `inferred` came from a
 * language model and may be confidently wrong; `user` is authoritative about
 * intent and says nothing about what the code does. Collapsing them into one
 * source string would lose exactly the distinction a reader needs.
 */
export type Derivation =
  /** `scanner` is the rule version that produced it (`SCANNER_VERSION`). */
  | { derived: { scanner: number } }
  | { inferred: { agent: string } }
  | "user";

/**
 * One box.
 *
 * Every field is present on the wire. None of the Rust `Option`s carries
 * `skip_serializing_if`, so an absent value arrives as an explicit `null`
 * rather than a missing key — `"projectId" in node` is always true and tells
 * you nothing.
 */
export interface ArchNode {
  /**
   * Unique within a graph. A project node reuses the scan's `Project.id`, so
   * it can be traced back to something runnable; containers and externals use
   * a prefixed id (`solution:`, `workspace:`, `external:`) that cannot collide
   * with one.
   */
  id: string;
  label: string;
  kind: ArchKind;
  /** The `Project.id`, or `null` for containers and anything external. */
  projectId: string | null;
  /**
   * Relative to the workspace root, forward slashes on every platform, so a
   * stored diagram survives being moved or opened on another machine.
   * Externals carry a `../`-prefixed path. `null` for solution folders, which
   * exist only inside a solution file.
   */
  path: string | null;
  /** Which adapter found it; `null` for containers and externals. */
  ecosystem: string | null;
}

/** One arrow. */
export interface ArchEdge {
  /** `ArchNode.id` of the source. */
  from: string;
  /** `ArchNode.id` of the target. */
  to: string;
  kind: EdgeKind;
  /**
   * Always `null` on a derived edge — the facts available to the deriver are
   * already carried by `kind`, and anything further would be commentary it
   * cannot substantiate. The field exists for inferred and user graphs, where
   * the label *is* the content.
   */
  label: string | null;
}

/** A whole diagram. */
export interface ArchGraph {
  /** Sorted by `ArchNode.id`. */
  nodes: ArchNode[];
  /** Sorted by endpoint, then kind. */
  edges: ArchEdge[];
  /**
   * Everything found that could not be turned into an edge, in the deriver's
   * own words. Never populated because the tool went wrong — always because
   * something in the workspace does not line up. A view that hides these is
   * showing a diagram that looks complete and is not.
   */
  warnings: string[];
  derivation: Derivation;
}

/**
 * Where a *stored* diagram came from.
 *
 * Deliberately not `Derivation`: this one has no `scanner` payload, because a
 * file on disk records that it was derived and the rule version belongs to the
 * graph, not the filing. Same external tagging — `"derived"` and `"user"` are
 * bare strings, `inferred` is an object. Pinned by
 * `a_diagram_file_serialises_with_the_keys_the_frontend_reads` and
 * `a_derived_or_user_derivation_serialises_as_a_bare_string` in
 * `architecture/store_tests.rs`.
 */
export type DiagramDerivation = "derived" | { inferred: { agent: string } } | "user";

/**
 * One diagram file, as `arch_list_diagrams` reports it.
 *
 * All seven keys are always present; the `Option` fields cross as `null`.
 */
export interface DiagramFile {
  /** File name including `.md`. */
  name: string;
  /** Relative to the workspace root, forward slashes. */
  path: string;
  /** Free-text level from the front matter, or `null`. */
  level: string | null;
  derivation: DiagramDerivation;
  /** When it was generated, as the front matter recorded it. */
  generated: string | null;
  /**
   * An inferred diagram a person has since changed. Both halves matter to a
   * reader: the arrows still came from that agent, and a person edited them.
   */
  edited: boolean;
  /**
   * Why this file could not be fully understood — unreadable front matter, or
   * an unreadable file. It is still listed, and it is presented as a user
   * diagram, because refusing to show a person a file that is plainly on disk
   * is worse than showing it unlabelled.
   */
  warning: string | null;
}

/** Which rule a piece of Mermaid source broke. */
export type ValidationRule =
  | "fenceCount"
  | "diagramType"
  | "forbiddenDirective"
  | "unbalancedSubgraph"
  | "undeclaredNode";

/**
 * Why a piece of Mermaid source was rejected, and where.
 *
 * `line` is 1-based and counted **in the source as given**, fence and
 * surrounding prose included, so it can be pointed at directly in an editor
 * holding the whole document.
 */
export interface ValidationError {
  rule: ValidationRule;
  line: number;
  /** One sentence naming what was found. */
  detail: string;
}

// ---------------------------------------------------------------------------
// Language servers (`crates/core/src/lsp/model.rs`)
// ---------------------------------------------------------------------------

/**
 * Why there is, or is not, an answer from a language server.
 *
 * **Never collapse two of these into one when rendering.** A server that was
 * never configured, one still starting, one still loading its projects, one that
 * died, and one that does not advertise the capability are five different things
 * to tell somebody, and only `"ready"` licenses a count. Every other outcome
 * carries a `message`, and that message is what the user gets instead of a
 * number — an empty list drawn for any of them reads as "there are none", which
 * is the one answer the backend refused to give.
 *
 * Pinned against these exact spellings by
 * `every_availability_variant_serialises_to_its_exact_string` and
 * `availability_has_exactly_six_variants` in `crates/core/src/lsp/model_tests.rs`.
 */
export type Availability =
  | "notConfigured"
  | "starting"
  | "loading"
  | "ready"
  | "failed"
  | "unsupported";

/**
 * Every use site of one symbol, or the reason there is no list.
 *
 * **Every key is present on every result**, `null` where there is no value: the
 * Rust struct deliberately carries no `skip_serializing_if`, so these are
 * `T | null` and never optional `?`. Written with `?` instead, `tsc` accepts
 * code that then reads `undefined` at runtime, and "the backend has no answer"
 * becomes indistinguishable from "the backend forgot to send one".
 */
export interface UsageResult {
  outcome: Availability;
  /**
   * The **true** number of distinct use sites.
   *
   * `null` unless `outcome` is `"ready"`; `0` genuinely means "no usages", which
   * is a real answer and not the same thing — so branch on `total === null`, not
   * on falsiness. When `truncated` is set this is still the full count, larger
   * than `usages.length`.
   */
  total: number | null;
  /** At most the backend's cap. Always empty when `total` is `null`. */
  usages: Usage[];
  /** Whether `usages` is shorter than `total`. Say so wherever the rows show. */
  truncated: boolean;
  /**
   * Why, in the user's terms, when `outcome` is not `"ready"` — **and the
   * qualification when it is.**
   *
   * A `"ready"` answer is usually `null` here. It is not always: a server that
   * was promoted at the backend's readiness ceiling (90 s, because a project load
   * never finished) answers with the rows it really has and says here that the
   * count may be low. So render this whenever it is non-null, not only for
   * non-`"ready"` outcomes — that message is the difference between a count and a
   * count you can trust.
   */
  message: string | null;
  /** Which server answered, for a status line. `null` when none did. */
  server: string | null;
}

/** One row of the usages list. */
export interface Usage {
  /**
   * Workspace-relative with forward slashes (a Rust `PathBuf`, which crosses as
   * a plain string), ready for `fsReadFile` and the open-a-file chain.
   *
   * `null` when the location is outside the workspace or in a non-`file:`
   * document — Roslyn really does answer with `source-generated:` and metadata
   * URIs. The row still exists and is still counted; it just cannot be opened,
   * so render it unclickable rather than dropping it, which would contradict
   * `total`.
   */
  path: string | null;
  /** The relative path, or the raw URI when there is no path. */
  label: string;
  /**
   * **1-based**, matching the editor gutter, `SearchHit.line` and everything in
   * the existing open-a-file-at-a-line chain. Its asymmetry with the `character`
   * fields elsewhere in this section is deliberate; see {@link Target.character}.
   */
  line: number;
  /** One trimmed line of context. */
  snippet: string;
  /**
   * Where to underline inside `snippet`. `null` when the match did not survive
   * trimming — no underline is honest, an underline over the wrong characters is
   * a claim.
   */
  highlight: Highlight | null;
}

/**
 * A span to underline, as **UTF-16 code-unit** offsets into a `snippet`.
 *
 * UTF-16 because the reader is JavaScript: these are the units `String.length`,
 * `slice`, `substring` and CodeMirror all count in, so slicing the snippet with
 * them directly is correct. The Rust side holds the same span in **bytes** and
 * `lsp::positions::byte_to_utf16` converts — a byte offset read as if it were
 * this would shift the underline on any non-ASCII character earlier in the line
 * and can cut a surrogate pair in half.
 *
 * Note the contrast with `SearchHit.positions`, which are **character** (code
 * point) indices and must be read with `Array.from`. The two are different units
 * on purpose and are not interchangeable.
 */
export interface Highlight {
  start: number;
  end: number;
}

/**
 * Where a symbol is declared, implemented and typed.
 *
 * Three lists rather than one because they answer three different questions, and
 * a symbol legitimately appears in more than one — an interface method is a
 * declaration from one angle and an implementation from another.
 *
 * **There is one `outcome` for three lists, so an empty list on a `"ready"`
 * answer does not by itself mean "none".** A server that advertises
 * `definitionProvider` but not `implementationProvider` answers the first group
 * and refuses the second; the outcome stays `"ready"` and the reason is in
 * `message`, naming the group ("No implementations: …"). Only a refusal of **all
 * three** changes `outcome` (to the most severe of the three reasons — `"failed"`
 * over `"loading"` over `"unsupported"`). So: an empty group licenses "none" only
 * when `message` is `null`; otherwise show the message beside it, or a UI states
 * "no implementations" about a question nobody could ask. Pinned by
 * `an_unadvertised_implementation_capability_empties_that_group_and_says_so` and
 * `a_goto_answer_nobody_could_be_asked_for_is_not_ready_with_three_empty_lists`
 * in `crates/core/tests/lsp_session.rs`.
 */
export interface DefinitionResult {
  outcome: Availability;
  declarations: Target[];
  implementations: Target[];
  typeDefinitions: Target[];
  /**
   * Why a group is empty, or the qualification a `"ready"` answer needs — see
   * {@link UsageResult.message}. The group is named in English prose, so this is
   * for showing, not for parsing.
   */
  message: string | null;
}

/** Somewhere to jump to. */
export interface Target {
  /** Workspace-relative, forward slashes. `null` as on {@link Usage.path}. */
  path: string | null;
  /** The relative path, or the raw URI when there is no path. */
  label: string;
  /** **1-based**, matching the editor gutter. See {@link Target.character}. */
  line: number;
  /**
   * **0-based UTF-16 code units**, which is exactly what CodeMirror hands over
   * and takes back.
   *
   * The asymmetry with `line` is deliberate and holds in **both** directions
   * across this IPC surface: there is a 1-based line convention everywhere in
   * this app (gutters, `SymbolIndex`, `SearchHit.line`) and no 1-based column
   * convention anywhere at all, so inventing one to match would mean converting
   * at every call site instead of none. Rust converts in
   * `lsp::positions::{to_editor_line, to_lsp_line}` and nowhere else. Stated on
   * every field it touches because an undocumented mixed convention is how an
   * off-by-one ships.
   */
  character: number;
  snippet: string;
  /** The enclosing declaration, when the answer carried one; else `null`. */
  container: string | null;
}

/** The inline usage rows one file should show, or the reason there are none. */
export interface AnchorResult {
  outcome: Availability;
  anchors: DeclarationAnchor[];
  /**
   * Why there are no anchors, or the qualification a `"ready"` answer needs — see
   * {@link UsageResult.message}.
   *
   * Note that `"failed"` here includes "the file could not be read, so no server
   * could be told about it": `documentSymbol` is a question about an open buffer,
   * and an unopened one is a failure rather than a file that declares nothing.
   */
  message: string | null;
}

/** One declaration in a file that deserves an inline "N usages" row. */
export interface DeclarationAnchor {
  /**
   * Stable within one file across calls, and distinct for two same-named
   * overloads — so a widget can be keyed on it without remounting on every
   * refresh.
   */
  id: string;
  /** The bare identifier; Roslyn sends a whole signature, this is the name. */
  name: string;
  kind: SymbolKind;
  /** Where the inline row is drawn: the declaration's first line, **1-based**. */
  line: number;
  /**
   * Where a usages request must be aimed — the identifier's column, not the
   * body's. **0-based UTF-16 code units**, and an offset into `selectionLine`
   * rather than into `line`. See {@link Target.character} for the asymmetry.
   */
  character: number;
  /**
   * The identifier's own line, **1-based**. Differs from `line` whenever
   * attributes, doc comments or a wrapped signature push the name below the
   * start of the declaration — so pass this line, not `line`, to
   * `lspFindUsages` alongside `character`.
   */
  selectionLine: number;
}

/** What every configured server is doing, for the status surface. */
export interface LspStatus {
  servers: ServerStatus[];
}

/**
 * One server's state, and what to do about it.
 *
 * A language appears here once it has been started, or immediately and
 * permanently if it could not be resolved — there is no `Availability` for
 * "found, idle", so an absent row means "nothing has asked this language
 * anything yet" and not "no server exists".
 */
export interface ServerStatus {
  id: string;
  /** Human-readable, for the row's label. */
  language: string;
  state: Availability;
  /** The version line, the exit code, the error — whatever there is to say. */
  detail: string | null;
  /**
   * Why this server's answers may be incomplete, when they may be.
   *
   * Non-null only for a server promoted at the backend's 90 s readiness ceiling,
   * whose `state` is `ready` because requests do proceed and whose counts arrive
   * carrying `UsageResult.message`. A field of its own rather than prose in
   * `detail` — `detail` is the program path for a plain ready server, so a caveat
   * delivered there cannot be told from a healthy server without parsing English,
   * and the indicator (which says nothing about a ready server) dropped it.
   */
  caveat: string | null;
  /**
   * Everywhere that was searched for the program; empty once one was found.
   *
   * A list rather than a sentence because "not found" is only actionable if the
   * user can see *where* we looked, so show all of it rather than the first.
   */
  lookedFor: string[];
  /** What the user could do about it, when there is something. */
  hint: string | null;
}

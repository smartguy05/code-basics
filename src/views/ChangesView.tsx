import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { DiffView, type DiffLayout, type DiffViewHandle } from "../components/DiffView";
import {
  allChangedIndices,
  focusedBaseline,
  normaliseEndings,
  onlyHunks,
} from "../components/diffLogic";
import { buildSections, sortFilesByRisk, statusLetter, type FileSection } from "./changesLogic";
import {
  buildFileTree,
  decodeCollapsedFolders,
  defaultFilesLayout,
  encodeCollapsedFolders,
  flattenFileTree,
  type FilesLayout,
} from "./folderTreeLogic";
import { fileRisk, hunkRisk, type RiskIndex } from "../components/riskLogic";
import { confidenceForFile } from "../components/confidenceLogic";
import { Sidebar } from "../components/Sidebar";
import { IntentPanel } from "../components/IntentPanel";
import {
  httpFileCandidates,
  pickBehavioralConfig,
  resolveHttpFiles,
} from "../components/behavioralPanelLogic";
import { verifyClaimsAction } from "../components/claimVerifyLogic";
import { ErosionPanel } from "../components/ErosionPanel";
import { badgeCount } from "../components/erosionLogic";
import { uncoveredIndicesForPath } from "../components/coverageOfChangeLogic";
import { StashPanel } from "../components/StashPanel";
import * as api from "../ipc/api";
import { applyEditorFontSize, loadEditorFontSize, onEditorFontSizeChange } from "../editorFontSize";
import {
  MAX_EDITOR_FONT_SIZE,
  MIN_EDITOR_FONT_SIZE,
  stepFontSize,
} from "../editorFontSizeLogic";
import type {
  BehavioralReport,
  ChangeCoverage,
  Changelist,
  ComparisonMode,
  FileChange,
  FileContents,
  FileDiff,
  ErosionFlag,
  ErosionReport,
  GroupFile,
  InstallScope,
  IntentGroup,
  ProviderId,
  ProviderStatus,
  RejectSummary,
  RetireSummary,
  RunConfig,
  Scorecard,
  UnfulfilledClaim,
  Workspace,
  WorkingStatus,
} from "../ipc/types";

const DIFF_LAYOUT_KEY = "code-basics.diffLayout";

/** A zeroed scorecard for before the first intent refresh. */
const EMPTY_SCORECARD: Scorecard = {
  claims: 0,
  evidenced: 0,
  unmatched: 0,
  hunks: 0,
  attributedHunks: 0,
  unattributedLines: 0,
};
const GROUPING_KEY = "code-basics.changesGrouping";
const FILES_LAYOUT_KEY = "code-basics.filesLayout";
const COLLAPSED_FOLDERS_KEY = "code-basics.collapsedFolders";
const COLLAPSE_KEY = "code-basics.diffCollapseUnchanged";
const WHITESPACE_KEY = "code-basics.diffIgnoreWhitespace";

/**
 * How often the Changes tab re-reads the working tree while it is showing, so
 * edits made outside the app (by an agent, a terminal, another tool) appear
 * without switching tabs. Gentle on purpose: each tick re-runs the intent and
 * erosion scans, and the results are de-churned so nothing re-renders unless
 * the working tree actually changed.
 */
const POLL_MS = 2000;

/** How the sidebar organises the working tree. */
type Grouping = "files" | "intent" | "stashes" | "erosion";

function loadGrouping(): Grouping {
  const stored = localStorage.getItem(GROUPING_KEY);
  return stored === "intent" || stored === "stashes" || stored === "erosion"
    ? stored
    : "files";
}

function loadFilesLayout(): FilesLayout {
  return defaultFilesLayout(localStorage.getItem(FILES_LAYOUT_KEY));
}

/**
 * Where one workspace's folded-away tree folders are remembered.
 *
 * Keyed by root because the keys are section-and-folder paths, which mean
 * nothing in another codebase — a shared key would fold away folders the user
 * never touched here.
 */
function collapsedFoldersKey(root: string): string {
  return `${COLLAPSED_FOLDERS_KEY}:${root}`;
}

function loadDiffLayout(): DiffLayout {
  return localStorage.getItem(DIFF_LAYOUT_KEY) === "inline" ? "inline" : "sideBySide";
}

/** Off unless it was explicitly turned on — folding hides code by default. */
function loadCollapse(): boolean {
  return localStorage.getItem(COLLAPSE_KEY) === "true";
}

/** Off by default: the honest comparison is the one git actually made. */
function loadIgnoreWhitespace(): boolean {
  return localStorage.getItem(WHITESPACE_KEY) === "true";
}

const MODE_LABELS: Record<ComparisonMode, string> = {
  workingToHead: "Working tree vs HEAD",
  workingToIndex: "Unstaged (vs staged)",
  indexToHead: "Staged (vs HEAD)",
};

export function ChangesView({
  workspace,
  behavioral,
  onOpenReview,
  onRunBehavioral,
  onVerifyClaims,
}: {
  /**
   * The workspace this view is for, passed down rather than fetched, so it always
   * matches the tab that mounted it — a background poll must never read the
   * backend's *active* workspace, which may be another tab.
   */
  workspace: Workspace;
  /**
   * The finished before/after report, owned by `App` (the run itself happens in
   * the floating `BehavioralPanel`). Held here only to badge each intent card
   * with the deltas attributed to it; `null` until a run completes.
   */
  behavioral: BehavioralReport | null;
  onOpenReview: () => void;
  /**
   * Open the before/after window for `configId` (the run streams there).
   * `httpFiles` names the `.http` files to replay, or `null` to let the backend
   * discover them (the default).
   */
  onRunBehavioral: (configId: string, httpFiles: string[] | null) => void;
  /** Open the before/after window, then hand its evidence to the claim agent. */
  onVerifyClaims: (configId: string, httpFiles: string[] | null) => void;
}) {
  const [status, setStatus] = useState<WorkingStatus | null>(null);
  const [mode, setMode] = useState<ComparisonMode>("workingToHead");
  const [selectedPath, setSelectedPath] = useState<string | null>(null);
  const [contents, setContents] = useState<FileContents | null>(null);
  const [diff, setDiff] = useState<FileDiff | null>(null);
  const [selectedLines, setSelectedLines] = useState<number[]>([]);
  const [error, setError] = useState<string | null>(null);
  const [message, setMessage] = useState("");
  const [amend, setAmend] = useState(false);
  const [busy, setBusy] = useState(false);
  const [diffLayout, setDiffLayout] = useState<DiffLayout>(loadDiffLayout);
  const [collapseUnchanged, setCollapseUnchanged] = useState(loadCollapse);
  const [ignoreWhitespace, setIgnoreWhitespace] = useState(loadIgnoreWhitespace);
  const [groups, setGroups] = useState<Changelist[]>([]);
  /** Right-click target: where the menu sits and which file it acts on. */
  const [context, setContext] = useState<{ x: number; y: number; change: FileChange } | null>(
    null,
  );
  /** Sections the user folded away, by section key. */
  const [collapsed, setCollapsed] = useState<Set<string>>(() => new Set());
  /** Flat path list vs. a collapsible folder tree, in the Files view. */
  const [filesLayout, setFilesLayout] = useState<FilesLayout>(loadFilesLayout);
  /**
   * Folders the user folded away in the tree layout, keyed by
   * `${section.key}:${folderPath}` so the same folder can be open in one
   * section and closed in another. Persisted per workspace root, so folding a
   * noisy folder away survives leaving and returning to the tab — this view is
   * mounted conditionally and re-reads from disk every time.
   */
  const [collapsedFolders, setCollapsedFolders] = useState<Set<string>>(() =>
    decodeCollapsedFolders(localStorage.getItem(collapsedFoldersKey(workspace.root))),
  );
  // Persisted from an effect rather than from inside the state updater, so the
  // updater stays a pure function of its previous value.
  useEffect(() => {
    localStorage.setItem(
      collapsedFoldersKey(workspace.root),
      encodeCollapsedFolders(collapsedFolders),
    );
  }, [collapsedFolders, workspace.root]);
  /**
   * Name being typed for a new group. `null` means the input is hidden;
   * `pendingPath` is a file to drop into it as soon as it exists, so "New
   * group…" from a file's menu is one step rather than two.
   */
  const [newGroup, setNewGroup] = useState<{ name: string; pendingPath: string | null } | null>(
    null,
  );

  /**
   * The editor font size, mirrored here only so the buttons can grey out at the
   * ends of the range.
   *
   * `editorFontSize.ts` is the single source of truth — the buttons apply
   * through it and this state is refreshed from the change event, so pressing
   * Ctrl+= keeps the buttons honest without this view owning the value.
   */
  const [fontSize, setFontSize] = useState(loadEditorFontSize);
  useEffect(() => onEditorFontSizeChange(() => setFontSize(loadEditorFontSize())), []);

  /** The open diff, for the toolbar arrows and F7. */
  const diffHandle = useRef<DiffViewHandle | null>(null);

  /**
   * State the background poll reads without wanting to re-fire on every change.
   *
   * `busyRef` mirrors `busy` so the interval can skip a tick while an action is
   * in flight, without the interval effect resubscribing each time `busy`
   * flips. The `*Signature` refs hold a serialisation of the last committed
   * result for each scan, so a poll that finds nothing changed skips the
   * setState entirely — no re-render, no card recompute — while a real change
   * still goes through.
   */
  const busyRef = useRef(false);
  const statusSignature = useRef<string | null>(null);
  const intentSignature = useRef<string | null>(null);
  const erosionSignature = useRef<string | null>(null);
  const coverageSignature = useRef<string | null>(null);
  busyRef.current = busy;

  const [grouping, setGrouping] = useState<Grouping>(loadGrouping);
  const [intentGroups, setIntentGroups] = useState<IntentGroup[]>([]);
  const [scorecard, setScorecard] = useState<Scorecard>(EMPTY_SCORECARD);
  const [unfulfilled, setUnfulfilled] = useState<UnfulfilledClaim[]>([]);
  const [evidenced, setEvidenced] = useState<UnfulfilledClaim[]>([]);
  const [erosion, setErosion] = useState<ErosionReport | null>(null);
  /**
   * Coverage of the changed lines from the last coverage-enabled test run,
   * mapped onto the current diff. Feeds the amber "uncovered" wash in the diff
   * pane (any grouping) and the summary + per-card badges in the intent view.
   * `null` until a coverage run has been collected.
   */
  const [coverage, setCoverage] = useState<ChangeCoverage | null>(null);
  const [providers, setProviders] = useState<ProviderStatus[]>([]);
  /**
   * The runtime before/after comparison for the current intent view, and the
   * config it runs. The intent sidebar has no console of its own, so the run's
   * streamed output is condensed into a one-line status rather than shown raw.
   */
  // Derived from the workspace prop, not fetched: the before/after action needs
  // this tab's configs, which must not come from the backend's active workspace.
  const configs: RunConfig[] = workspace.configs;
  /** The compact before/after actions menu in the intent header. */
  const [evidenceOpen, setEvidenceOpen] = useState(false);
  /**
   * Optional overrides for the before/after run, set in the Evidence dropdown.
   *
   * `configOverride` is a config id to replay instead of the auto-picked one
   * (`null` keeps the auto pick); `selectedHttp` is the set of changed `.http`
   * files to replay explicitly (empty keeps the backend's auto discovery). Both
   * default to the existing behaviour, so a user who never opens the picker gets
   * exactly the old auto config + null path.
   */
  const [configOverride, setConfigOverride] = useState<string | null>(null);
  const [selectedHttp, setSelectedHttp] = useState<string[]>([]);
  const [selectedGroup, setSelectedGroup] = useState<string | null>(null);
  /** Diff lines to preselect, so opening a card lands on its lines. */
  const [highlight, setHighlight] = useState<number[]>([]);
  /**
   * The card's hunks in the open file, or `null` for the whole diff.
   *
   * In intent mode one file can sit in several cards; opening a file from a
   * card scopes the diff pane to that card's hunks so the changes shown are
   * exactly the ones the card claims.
   */
  const [groupHunks, setGroupHunks] = useState<number[] | null>(null);

  function changeDiffLayout(layout: DiffLayout) {
    setDiffLayout(layout);
    localStorage.setItem(DIFF_LAYOUT_KEY, layout);
  }

  function changeIgnoreWhitespace(next: boolean) {
    setIgnoreWhitespace(next);
    localStorage.setItem(WHITESPACE_KEY, String(next));
  }

  function changeCollapse(next: boolean) {
    setCollapseUnchanged(next);
    localStorage.setItem(COLLAPSE_KEY, String(next));
  }

  function changeGrouping(next: Grouping) {
    setGrouping(next);
    localStorage.setItem(GROUPING_KEY, next);
    // Only the intent view has cards, so nothing may stay scoped to one when
    // leaving it (for either the files or the stashes view).
    if (next !== "intent") {
      setSelectedGroup(null);
      setHighlight([]);
      setGroupHunks(null);
    }
  }

  const refreshStatus = useCallback(async () => {
    try {
      const [nextStatus, nextGroups] = await Promise.all([
        api.gitStatus(),
        // Change groups are workspace-local bookkeeping; a workspace that has
        // never used them simply has none.
        api.gitChangelists().catch(() => ({ version: 1, groups: [] })),
      ]);
      // De-churn: the poll calls this on a timer, and replacing state with an
      // identical value would re-render the list for nothing. Only commit when
      // the working tree actually changed.
      const signature = JSON.stringify([nextStatus, nextGroups]);
      if (signature !== statusSignature.current) {
        statusSignature.current = signature;
        setStatus(nextStatus);
        setGroups(nextGroups.groups);
      }
      setError(null);
    } catch (e) {
      setError(api.errorMessage(e));
    }
  }, []);

  useEffect(() => {
    void refreshStatus();
  }, [refreshStatus]);

  const behavioralConfig = pickBehavioralConfig(configs);
  // Whether "Verify claims" can run, against which config, and why — the whole
  // decision lives in the tested helper.
  const verify = verifyClaimsAction(configs);

  // The changed .http/.rest files the run could replay explicitly, and the wire
  // value the picker's toggles resolve to (null == let the backend discover).
  const httpCandidates = httpFileCandidates(status?.files ?? []);
  const httpArg = resolveHttpFiles(
    selectedHttp.length > 0 ? { mode: "explicit", files: selectedHttp } : { mode: "auto" },
  );
  /** The config the run replays: the picker override, else the auto pick. */
  const chosenBehavioralConfig =
    (configOverride && configs.find((c) => c.id === configOverride)) || behavioralConfig;
  const chosenVerifyConfig =
    (configOverride && configs.find((c) => c.id === configOverride)) || verify.config;

  /**
   * Recompute the intent cards.
   *
   * Only while the intent view is showing: the grouping walks every changed
   * file, and there is no reason to pay for it when nothing displays it.
   */
  const refreshIntent = useCallback(async () => {
    if (grouping !== "intent") return;
    try {
      const [review, status] = await Promise.all([
        api.intentGroups(mode),
        api.intentCaptureStatus().catch(() => [] as ProviderStatus[]),
      ]);
      // De-churn (see refreshStatus): the poll recomputes the cards every tick
      // to catch edits to files already in the list, so only commit — and only
      // re-render the panel — when the result changed.
      const signature = JSON.stringify([review, status]);
      if (signature !== intentSignature.current) {
        intentSignature.current = signature;
        setIntentGroups(review.groups);
        setScorecard(review.scorecard);
        setUnfulfilled(review.unfulfilled);
        setEvidenced(review.evidenced ?? []);
        setProviders(status);
      }
    } catch (e) {
      setError(api.errorMessage(e));
    }
  }, [grouping, mode]);

  useEffect(() => {
    void refreshIntent();
  }, [refreshIntent, status]);

  /**
   * Recompute the erosion flags. Runs for the Erosion view (which shows them)
   * and the Intent view (whose risk badges read them), for the same reason the
   * intent grouping does: it walks every changed file. Both re-run on a mode
   * change, so the flags' `index` values always match the diff currently shown.
   */
  const refreshErosion = useCallback(async () => {
    if (grouping !== "erosion" && grouping !== "intent") return;
    try {
      const report = await api.erosionScan(mode);
      const signature = JSON.stringify(report);
      if (signature !== erosionSignature.current) {
        erosionSignature.current = signature;
        setErosion(report);
      }
    } catch (e) {
      setError(api.errorMessage(e));
    }
  }, [grouping, mode]);

  useEffect(() => {
    void refreshErosion();
  }, [refreshErosion, status]);

  /**
   * Re-read the coverage-of-change map. Non-streaming (like the erosion scan):
   * it reads the cached artifact from the last coverage-enabled test run and
   * maps it onto the diff in the current mode. Runs for every grouping except
   * stashes, because the amber wash in the diff pane is shown in the files and
   * erosion views too, not only intent.
   */
  const refreshCoverage = useCallback(async () => {
    if (grouping === "stashes") return;
    try {
      const report = await api.coverageOfChange(mode);
      const signature = JSON.stringify(report);
      if (signature !== coverageSignature.current) {
        coverageSignature.current = signature;
        setCoverage(report);
      }
    } catch (e) {
      setError(api.errorMessage(e));
    }
  }, [grouping, mode]);

  useEffect(() => {
    void refreshCoverage();
  }, [refreshCoverage, status]);

  // Erosion flags are keyed by DiffLine.index, which only means anything within
  // one comparison mode. Drop them the instant the mode changes so a risk badge
  // can never intersect a stale flag against a differently-numbered diff; the
  // effect above then rescans for the new mode.
  useEffect(() => {
    setErosion(null);
    // Coverage's uncovered indices are DiffLine.index values too, so they are
    // only valid for one mode — drop them the instant the mode changes so the
    // amber wash can never paint against a differently-numbered diff.
    setCoverage(null);
    // The intent, erosion and coverage signatures are keyed to the diff's line
    // numbering, which is per-mode; clear them so the next scan is never
    // suppressed by a match against the previous mode's result.
    erosionSignature.current = null;
    intentSignature.current = null;
    coverageSignature.current = null;
  }, [mode]);

  /**
   * Poll the working tree while this tab is showing, so changes made outside
   * the app appear without switching tabs. This view is mounted only while the
   * Changes tab is active (see `App.tsx`), so the interval is naturally scoped
   * to the tab and torn down on leave.
   *
   * Each of the three refreshers de-churns its own result, so a quiet tick does
   * no work beyond the read; the open diff pane is deliberately left alone, so
   * an unsaved edit in the editor is never discarded under the user. A tick is
   * skipped while an action is in flight or the window is hidden, and while the
   * stash view (which owns its own refresh) is showing.
   */
  useEffect(() => {
    const id = window.setInterval(() => {
      if (busyRef.current || document.hidden || grouping === "stashes") return;
      void refreshStatus();
      void refreshIntent();
      void refreshErosion();
      void refreshCoverage();
    }, POLL_MS);
    return () => window.clearInterval(id);
  }, [refreshStatus, refreshIntent, refreshErosion, refreshCoverage, grouping]);

  const loadFile = useCallback(
    async (path: string, comparison: ComparisonMode) => {
      try {
        const [fileContents, fileDiff] = await Promise.all([
          api.gitFileContents(path, comparison),
          api.gitFileDiff(path, comparison),
        ]);
        setContents(fileContents);
        setDiff(fileDiff);
        setSelectedLines([]);
        setError(null);
      } catch (e) {
        setError(api.errorMessage(e));
      }
    },
    [],
  );

  useEffect(() => {
    if (selectedPath) void loadFile(selectedPath, mode);
  }, [selectedPath, mode, loadFile]);

  /** Re-read the file list, the open file and the cards after a mutation. */
  const refreshAll = useCallback(async () => {
    await refreshStatus();
    if (selectedPath) await loadFile(selectedPath, mode);
    await refreshIntent();
    await refreshErosion();
    await refreshCoverage();
  }, [refreshStatus, loadFile, selectedPath, mode, refreshIntent, refreshErosion, refreshCoverage]);

  async function withBusy(action: () => Promise<void>) {
    setBusy(true);
    try {
      await action();
      setError(null);
    } catch (e) {
      setError(api.errorMessage(e));
    } finally {
      setBusy(false);
    }
  }

  const revert = (lines: number[]) =>
    withBusy(async () => {
      if (!selectedPath || lines.length === 0) return;
      await api.gitRevertLines(selectedPath, mode, lines);
      await refreshAll();
    });

  const stage = (lines: number[]) =>
    withBusy(async () => {
      if (!selectedPath) return;
      if (lines.length === 0) await api.gitStageFile(selectedPath);
      else await api.gitStageLines(selectedPath, lines);
      await refreshAll();
    });

  const unstage = (lines: number[]) =>
    withBusy(async () => {
      if (!selectedPath) return;
      if (lines.length === 0) await api.gitUnstageFile(selectedPath);
      else await api.gitUnstageLines(selectedPath, lines);
      await refreshAll();
    });

  /**
   * Open one file of a card: the diff pane shows only the card's hunks in it.
   */
  const selectGroupFile = (group: IntentGroup, file: GroupFile) => {
    setSelectedGroup(group.id);
    setSelectedPath(file.path);
    setHighlight(file.lineIndices);
    setGroupHunks(file.hunks);
  };

  /** Open a card: show its first file, scoped to the card. */
  const selectGroup = (group: IntentGroup) => {
    setSelectedGroup(group.id);
    const first = group.files[0];
    if (!first) return;
    selectGroupFile(group, first);
  };

  /**
   * Stage or revert a whole card.
   *
   * The group is named rather than its lines sent: indices are only valid for
   * one comparison mode, and staging uses a different one from the view. Rust
   * re-derives them from a fresh diff.
   */
  const stageGroup = (group: IntentGroup) =>
    withBusy(async () => {
      const staged = await api.stageIntentGroup(group.id);
      if (staged === 0) setError("Nothing in that group could be staged.");
      await refreshAll();
    });

  const revertGroup = (group: IntentGroup) =>
    withBusy(async () => {
      const reverted = await api.revertIntentGroup(group.id, mode);
      if (reverted === 0) setError("Nothing in that group could be reverted.");
      await refreshAll();
    });

  /**
   * Write the user's own intent on a card, then re-read so the card retitles.
   * The note binds to the card's current changed lines by content (see
   * `cb_core::intents::user`) and wins over any agent reason there.
   */
  const setCardIntent = (group: IntentGroup, label: string) =>
    withBusy(async () => {
      await api.setCardIntent(group.id, label, mode);
      await refreshAll();
    });

  /**
   * Move some of a card's changes into another card, or into a new one.
   *
   * Stored as a user note over the moved lines' content, like a hand-written
   * intent, so it survives the lines shifting and outranks any agent reason on
   * them. `paths` empty moves the whole card.
   */
  const moveCardEdits = (
    group: IntentGroup,
    paths: string[],
    destination: { group?: string; label?: string },
  ) =>
    withBusy(async () => {
      await api.moveCardEdits(group.id, paths, destination, mode);
      await refreshAll();
    });

  /** Remove the user's note from a card, restoring its previous title. */
  const clearCardIntent = (group: IntentGroup) =>
    withBusy(async () => {
      await api.clearCardIntent(group.id, mode);
      await refreshAll();
    });

  const stageGroupFile = (group: IntentGroup, file: GroupFile) =>
    withBusy(async () => {
      const staged = await api.stageIntentGroup(group.id, file.path);
      if (staged === 0) setError("Nothing in that file's share of the group could be staged.");
      await refreshAll();
    });

  const revertGroupFile = (group: IntentGroup, file: GroupFile) =>
    withBusy(async () => {
      const reverted = await api.revertIntentGroup(group.id, mode, file.path);
      if (reverted === 0)
        setError("Nothing in that file's share of the group could be reverted.");
      await refreshAll();
    });

  /**
   * Reject: revert, and leave the reason in the code where it was.
   *
   * The summary is handed back rather than reported here, for the same reason
   * `importHistory` hands its count back — the panel says what happened next to
   * the button that was pressed, and `withBusy` clears the view's error on a
   * successful action.
   */
  const rejectGroup = async (group: IntentGroup, reason: string, file?: GroupFile) => {
    let summary: RejectSummary | null = null;
    await withBusy(async () => {
      summary = await api.rejectIntentGroup(group.id, mode, reason, file?.path);
      await refreshAll();
    });
    return summary;
  };

  const enableCapture = async (provider: ProviderId, scope: InstallScope) => {
    setProviders(await api.enableIntentCapture(provider, scope));
    await refreshIntent();
  };

  const disableCapture = async (provider: ProviderId, scope: InstallScope) => {
    setProviders(await api.disableIntentCapture(provider, scope));
    await refreshIntent();
  };

  /**
   * Import, and hand the count back: the panel reports the outcome inline,
   * next to the banner that offered the action, rather than as a view error.
   */
  const importHistory = async () => {
    let total = 0;
    await withBusy(async () => {
      total = await api.importIntentHistory();
      await refreshAll();
    });
    return total;
  };

  /**
   * Preview the archive, and run it. Two calls rather than one so the panel can
   * confirm with real counts: retirement moves records out of the live store,
   * and the user should see how many before agreeing to it.
   */
  const previewPrune = () => api.intentPrunePreview();
  const prune = async () => {
    let summary: RetireSummary = {
      recordsRetired: 0,
      labelsRetired: 0,
      keptRecords: 0,
      pruned: false,
    };
    await withBusy(async () => {
      summary = await api.pruneIntentHistory();
      await refreshAll();
    });
    return summary;
  };

  /**
   * Both before/after actions run in the floating {@link BehavioralPanel} at the
   * app level (so the run survives a tab switch and its report is shown in full,
   * not condensed into this sidebar). These only pick the config and open the
   * window; "Verify claims" opens it primed to chain into the claim-check agent.
   */
  const runBehavioral = () => {
    if (chosenBehavioralConfig) onRunBehavioral(chosenBehavioralConfig.id, httpArg);
  };
  const verifyClaims = () => {
    if (chosenVerifyConfig) onVerifyClaims(chosenVerifyConfig.id, httpArg);
  };

  /** Stage or unstage a whole file, whichever one was right-clicked. */
  const stageFile = (path: string, staged: boolean) =>
    withBusy(async () => {
      if (staged) await api.gitUnstageFile(path);
      else await api.gitStageFile(path);
      await refreshAll();
    });

  const moveToGroup = (path: string, group: string | null) =>
    withBusy(async () => {
      setGroups((await api.gitAssignToChangelist([path], group)).groups);
    });

  const createGroup = () =>
    withBusy(async () => {
      const pending = newGroup;
      setNewGroup(null);
      const name = pending?.name.trim();
      if (!name) return;

      let next = await api.gitCreateChangelist(name);
      // "New group…" from a file's menu should land the file in it, not just
      // create an empty group and leave the user to drag it over.
      if (pending?.pendingPath) {
        next = await api.gitAssignToChangelist([pending.pendingPath], name);
      }
      setGroups(next.groups);
    });

  const deleteGroup = (name: string) =>
    withBusy(async () => {
      setGroups((await api.gitDeleteChangelist(name)).groups);
    });

  const commit = () =>
    withBusy(async () => {
      await api.gitCommit(message, amend);
      setMessage("");
      setAmend(false);
      await refreshAll();
    });

  const save = (content: string) =>
    withBusy(async () => {
      if (!selectedPath) return;
      await api.gitWriteFile(selectedPath, content);
      await refreshAll();
    });

  const files = status?.files ?? [];
  const hasSelection = selectedLines.length > 0;
  /**
   * What the diff pane shows: the whole diff, or — when a card's file is
   * open — only that card's hunks in it. The whole-file buttons below act on
   * this, so "Revert file" reverts what is on screen, never hidden changes.
   */
  const shownDiff =
    grouping === "intent" && groupHunks != null && diff != null
      ? onlyHunks(diff, groupHunks)
      : diff;

  // With an intent card open, scope the side-by-side colouring to the card's
  // own change: rebuild the baseline as the working copy with only those hunks
  // reverted, so every other region is identical on both sides and only this
  // intent's hunk shows as a difference. Memoised so the diff editor is not
  // torn down and rebuilt on unrelated renders (the baseline is in its deps).
  const viewBaseline = useMemo(() => {
    if (
      grouping === "intent" &&
      groupHunks != null &&
      diff != null &&
      contents?.working != null &&
      contents?.baseline != null
    ) {
      return focusedBaseline(normaliseEndings(contents.working), onlyHunks(diff, groupHunks).hunks);
    }
    return contents?.baseline ?? null;
  }, [grouping, groupHunks, diff, contents]);
  const canRevertAll = shownDiff != null && allChangedIndices(shownDiff).length > 0;
  // The uncovered changed-line indices for the open file only. Scoped by path
  // (not the whole report) because DiffLine.index is per-file, so an index from
  // another file could collide with a real line number in this one.
  const uncoveredForFile = uncoveredIndicesForPath(coverage, selectedPath);

  /**
   * Per-line risk for the open file, computed against the **full** diff so the
   * hunk indices line up with the intent groups' `files[].hunks` (which index
   * the full `FileDiff.hunks`). The entries are keyed by `DiffLine.index`, which
   * survives the `onlyHunks` scoping unchanged, so they still land correctly
   * when the diff pane shows only a card's hunks. Derived from erosion flags and
   * intent groups already on the client — nothing new is fetched — so it clears
   * and recomputes with the mode exactly as those two do.
   */
  const riskForFile = useMemo<RiskIndex[]>(() => {
    if (!diff || !selectedPath) return [];
    const flags = erosion?.flags ?? [];
    const out: RiskIndex[] = [];
    diff.hunks.forEach((hunk, hunkIndex) => {
      const level = hunkRisk(selectedPath, hunkIndex, diff, flags, intentGroups);
      if (!level) return;
      for (const line of hunk.lines) {
        if (line.origin === "context") continue;
        out.push({ index: line.index, level });
      }
    });
    return out;
  }, [diff, selectedPath, erosion, intentGroups]);

  /**
   * Per-line agent self-confidence for the open file — the confidence heatmap.
   * Fans each intent group's stated `selfConfidence` out to the changed
   * `DiffLine.index` values it owns (via `files[].lineIndices`), strongest tint
   * on `low`. Derived from the intent grouping already on the client, so it
   * clears and recomputes with the mode exactly as the risk overlay does. The
   * indices are keyed by `DiffLine.index`, which survives the `onlyHunks`
   * scoping, so they still land when the pane shows only a card's hunks.
   */
  const confidenceForOpenFile = useMemo(() => {
    if (!diff || !selectedPath) return [];
    return confidenceForFile(selectedPath, diff, intentGroups);
  }, [diff, selectedPath, intentGroups]);

  // Surface the riskier files first within each section, and keep the plain git
  // order for everything unweighted. Uses the same erosion + intent signals; in
  // the files view those are quiet, so this is mostly the sensitive-path emphasis.
  const sections = sortFilesByRisk(buildSections(files, groups), (path) =>
    fileRisk(path, erosion?.flags ?? [], intentGroups),
  );
  /** What the marker strip marks and what F7 steps through. */
  const differences = shownDiff?.hunks.length ?? 0;

  function toggleSection(key: string) {
    setCollapsed((previous) => {
      const next = new Set(previous);
      if (next.has(key)) next.delete(key);
      else next.add(key);
      return next;
    });
  }

  function changeFilesLayout(next: FilesLayout) {
    setFilesLayout(next);
    localStorage.setItem(FILES_LAYOUT_KEY, next);
  }

  function toggleFolder(key: string) {
    setCollapsedFolders((previous) => {
      const next = new Set(previous);
      if (next.has(key)) next.delete(key);
      else next.add(key);
      return next;
    });
  }

  /** Opening a file directly is not opening a card; drop any card selection. */
  function openFile(path: string) {
    setSelectedPath(path);
    setSelectedGroup(null);
    setHighlight([]);
    setGroupHunks(null);
  }

  /** Open an erosion flag: show its file and highlight the offending line. */
  function openErosionFlag(flag: ErosionFlag) {
    setSelectedPath(flag.path);
    setSelectedGroup(null);
    setGroupHunks(null);
    setHighlight([flag.index]);
  }

  /**
   * A file row. In the flat layout it shows the full path; in the tree layout
   * `tree` carries the leaf name and nesting depth so the row indents under its
   * folder and shows only the filename. Selection, risk emphasis and the
   * right-click menu are identical either way.
   */
  function renderFileRow(
    change: FileChange,
    section: FileSection,
    tree?: { depth: number; label: string },
  ) {
    const { letter, className } = statusLetter(change, section.side);
    // Emphasis for a file the risk signals elevated; abstains (no class) for an
    // ordinary one. Same signals as the sort above.
    const risk = fileRisk(change.path, erosion?.flags ?? [], intentGroups);
    return (
      <button
        key={`${section.key}:${change.path}`}
        className={`row ${change.path === selectedPath ? "selected" : ""}${
          risk ? ` risk-${risk.level}` : ""
        }`}
        style={tree ? { paddingLeft: 6 + (tree.depth + 1) * 14 } : undefined}
        onClick={() => openFile(change.path)}
        onContextMenu={(e) => {
          e.preventDefault();
          openFile(change.path);
          setContext({ x: e.clientX, y: e.clientY, change });
        }}
        title={`${change.path} — right-click to stage or group`}
      >
        <span className={`status ${className}`}>{letter}</span>
        <span className="path">{tree ? tree.label : change.path}</span>
      </button>
    );
  }

  /** The files of one section as a collapsible folder tree. */
  function renderSectionTree(section: FileSection) {
    const rows = flattenFileTree(buildFileTree(section.files), (folderPath) =>
      collapsedFolders.has(`${section.key}:${folderPath}`),
    );
    return rows.map((row) => {
      if (row.kind === "file") {
        return renderFileRow(row.change, section, { depth: row.depth, label: row.label });
      }
      const key = `${section.key}:${row.path}`;
      return (
        <button
          key={`folder:${key}`}
          className="row folder-row"
          style={{ paddingLeft: 6 + row.depth * 14 }}
          onClick={() => toggleFolder(key)}
          title={row.path}
        >
          <span className="twisty">{row.collapsed ? "▸" : "▾"}</span>
          <span className="path">{row.label}</span>
          <span className="badge">{row.fileCount}</span>
        </button>
      );
    });
  }

  const groupingToggle = (
    <div className="segmented">
      <button
        className={grouping === "files" ? "active" : ""}
        onClick={() => changeGrouping("files")}
        title="List the changed files"
      >
        Files
      </button>
      <button
        className={grouping === "intent" ? "active" : ""}
        onClick={() => changeGrouping("intent")}
        title="Collapse hunks into the decisions behind them"
      >
        Intent
      </button>
      <button
        className={grouping === "erosion" ? "active" : ""}
        onClick={() => changeGrouping("erosion")}
        title="Flag changes that quietly weaken the codebase"
      >
        Erosion
        {badgeCount(erosion) > 0 && (
          <span className="badge" style={{ marginLeft: 4 }}>
            {badgeCount(erosion)}
          </span>
        )}
      </button>
      <button
        className={grouping === "stashes" ? "active" : ""}
        onClick={() => changeGrouping("stashes")}
        title="View, apply and drop stashes"
      >
        Stashes
      </button>
    </div>
  );

  // Stashes are a self-contained list-with-preview that replaces the file list,
  // diff and commit box entirely — none of which apply to a stash.
  if (grouping === "stashes") {
    return <StashPanel header={groupingToggle} onChanged={() => void refreshAll()} />;
  }

  return (
    <>
      <Sidebar className="file-list">
        {/* The branch name lives in the titlebar branch widget, so it is not
            repeated here — this row is just the ahead/behind badge and the
            runtime-evidence / review actions. */}
        <div className="group-label" style={{ display: "flex", alignItems: "center" }}>
          {status && (status.ahead > 0 || status.behind > 0) && (
            <span className="badge">
              ↑{status.ahead} ↓{status.behind}
            </span>
          )}
          <span style={{ flex: 1 }} />
          {grouping === "intent" && (
            <div className="evidence-menu" style={{ position: "relative" }}>
              <button
                onClick={() => setEvidenceOpen((v) => !v)}
                title="Runtime evidence: run the code against HEAD and your working tree and compare the observable outcomes — with or without an agent judging the diff's claims"
              >
                Evidence ▾
              </button>
              {evidenceOpen && (
                <>
                  <div className="dropdown-backdrop" onClick={() => setEvidenceOpen(false)} />
                  <div className="dropdown-menu" style={{ right: 0, left: "auto" }}>
                    {/* Override the auto-picked config. Defaults to the tested
                        `pickBehavioralConfig` choice; the user can replay any
                        other config instead. */}
                    {configs.length > 1 && (
                      <div className="dropdown-section" style={{ padding: "4px 8px" }}>
                        <div className="group-label">Config</div>
                        <select
                          value={configOverride ?? behavioralConfig?.id ?? ""}
                          onClick={(e) => e.stopPropagation()}
                          onChange={(e) => setConfigOverride(e.target.value || null)}
                          style={{ width: "100%" }}
                          title="Which configuration to replay against HEAD and the working tree"
                        >
                          {configs.map((c) => (
                            <option key={c.id} value={c.id}>
                              {c.name}
                            </option>
                          ))}
                        </select>
                      </div>
                    )}

                    {/* Replay specific changed .http files instead of letting the
                        backend discover them. No toggles = auto discovery. */}
                    {httpCandidates.length > 0 && (
                      <div className="dropdown-section" style={{ padding: "4px 8px" }}>
                        <div className="group-label">
                          HTTP files {selectedHttp.length === 0 && "(auto)"}
                        </div>
                        {httpCandidates.map((path) => (
                          <label
                            key={path}
                            className="dropdown-item"
                            style={{ display: "flex", gap: 6, alignItems: "center" }}
                            onClick={(e) => e.stopPropagation()}
                            title="Replay this .http file explicitly in the before/after run"
                          >
                            <input
                              type="checkbox"
                              checked={selectedHttp.includes(path)}
                              onChange={(e) =>
                                setSelectedHttp((prev) =>
                                  e.target.checked
                                    ? [...prev, path]
                                    : prev.filter((p) => p !== path),
                                )
                              }
                            />
                            <span className="path">{path}</span>
                          </label>
                        ))}
                      </div>
                    )}

                    {(configs.length > 1 || httpCandidates.length > 0) && (
                      <div className="dropdown-separator" />
                    )}

                    <div
                      className={`dropdown-item${chosenBehavioralConfig ? "" : " disabled"}`}
                      title={
                        chosenBehavioralConfig
                          ? `Run "${chosenBehavioralConfig.name}" against HEAD and your working tree, then show what changed in the observable outcomes — test results, console output, HTTP responses. No agent involved.`
                          : "No run configuration is available to replay before/after"
                      }
                      onClick={() => {
                        if (!chosenBehavioralConfig) return;
                        setEvidenceOpen(false);
                        runBehavioral();
                      }}
                    >
                      Run before/after
                    </div>
                    <div
                      className={`dropdown-item${verify.enabled ? "" : " disabled"}`}
                      title={verify.hint}
                      onClick={() => {
                        if (!verify.enabled) return;
                        setEvidenceOpen(false);
                        verifyClaims();
                      }}
                    >
                      Verify claims
                    </div>
                  </div>
                </>
              )}
            </div>
          )}
          <button
            onClick={onOpenReview}
            title="Run an adversarial review of the current changes (Claude Code or Codex)"
          >
            Review
          </button>
        </div>

        {status?.inProgressOperation && (
          <div className="warning">A {status.inProgressOperation} is in progress.</div>
        )}

        {groupingToggle}

        {grouping === "files" && files.length > 0 && (
          <div
            className="group-label"
            style={{ display: "flex", alignItems: "center", gap: 4 }}
          >
            <span style={{ flex: 1 }}>Layout</span>
            <div className="segmented">
              <button
                className={filesLayout === "flat" ? "active" : ""}
                onClick={() => changeFilesLayout("flat")}
                title="List every changed file by its full path"
              >
                List
              </button>
              <button
                className={filesLayout === "tree" ? "active" : ""}
                onClick={() => changeFilesLayout("tree")}
                title="Group the changed files into a collapsible folder tree"
              >
                Tree
              </button>
            </div>
          </div>
        )}

        {grouping === "intent" && (
          <>
            <IntentPanel
              groups={intentGroups}
              scorecard={scorecard}
              unfulfilled={unfulfilled}
              evidenced={evidenced}
              providers={providers}
              selectedGroup={selectedGroup}
              selectedPath={selectedPath}
              statusFiles={files}
              mode={mode}
              busy={busy}
              onSelect={selectGroup}
              onSelectFile={selectGroupFile}
              onStage={stageGroup}
              onRevert={revertGroup}
              onStageFile={stageGroupFile}
              onRevertFile={revertGroupFile}
              onReject={rejectGroup}
              onEnable={enableCapture}
              onDisable={disableCapture}
              onImportHistory={importHistory}
              onPreviewPrune={previewPrune}
              onPrune={prune}
              onSetIntent={setCardIntent}
              onClearIntent={clearCardIntent}
              onMoveEdits={moveCardEdits}
              behavioral={behavioral}
              erosionFlags={erosion?.flags}
              coverage={coverage}
            />
          </>
        )}

        {grouping === "erosion" && (
          <ErosionPanel
            report={erosion}
            selectedPath={selectedPath}
            onOpenFlag={openErosionFlag}
          />
        )}

        {grouping === "files" && files.length === 0 && groups.length === 0 && (
          <div className="muted" style={{ padding: 8 }}>
            No changes.
          </div>
        )}

        {grouping === "files" && sections.map((section) => {
          if (section.files.length === 0 && !section.keepWhenEmpty) return null;
          const isCollapsed = collapsed.has(section.key);

          return (
            <div key={section.key}>
              <div
                className="group-label dropdown-section"
                style={{ display: "flex", alignItems: "center", gap: 4 }}
                onClick={() => toggleSection(section.key)}
              >
                <span className="twisty">{isCollapsed ? "▸" : "▾"}</span>
                <span style={{ flex: 1 }}>{section.label}</span>
                <span className="badge">{section.files.length}</span>
                {section.group && (
                  <span
                    className="remove"
                    role="button"
                    title={`Delete the "${section.group}" group (its files stay, ungrouped)`}
                    onClick={(e) => {
                      e.stopPropagation();
                      if (!busy) void deleteGroup(section.group as string);
                    }}
                  >
                    ×
                  </span>
                )}
              </div>

              {!isCollapsed &&
                (filesLayout === "tree"
                  ? renderSectionTree(section)
                  : section.files.map((change) => renderFileRow(change, section)))}

              {!isCollapsed && section.files.length === 0 && section.keepWhenEmpty && (
                <div className="muted" style={{ padding: "4px 8px 4px 22px", fontSize: 12 }}>
                  Empty — right-click a file to move it here.
                </div>
              )}
            </div>
          );
        })}

        {grouping === "files" &&
          (newGroup ? (
            <input
              autoFocus
              placeholder="Group name…"
              value={newGroup.name}
              onChange={(e) => setNewGroup({ ...newGroup, name: e.target.value })}
              // Enter confirms; clicking away or Escape abandons it. Creating on
              // blur would fire a second time after Enter had already unmounted
              // the input, and the duplicate name would surface as an error.
              onBlur={() => setNewGroup(null)}
              onKeyDown={(e) => {
                if (e.key === "Enter") void createGroup();
                if (e.key === "Escape") setNewGroup(null);
              }}
              style={{ width: "100%", marginTop: 4 }}
            />
          ) : (
            <button
              className="row"
              style={{ opacity: 0.75 }}
              disabled={busy}
              onClick={() => setNewGroup({ name: "", pendingPath: null })}
              title="Group related files together while you work on them"
            >
              + New group
            </button>
          ))}

        <div className="commit-box">
          <textarea
            placeholder="Commit message"
            value={message}
            onChange={(e) => setMessage(e.target.value)}
          />
          <label style={{ display: "flex", gap: 6, alignItems: "center" }}>
            <input
              type="checkbox"
              checked={amend}
              onChange={(e) => setAmend(e.target.checked)}
            />
            Amend previous commit
          </label>
          <button
            data-command="changes.commit"
            className="primary"
            onClick={commit}
            disabled={busy || !message.trim()}
          >
            Commit
          </button>
        </div>
      </Sidebar>

      {context && (
        <>
          <div className="dropdown-backdrop" onClick={() => setContext(null)} />
          <div
            className="dropdown-menu"
            style={{ position: "fixed", left: context.x, top: context.y, zIndex: 46 }}
          >
            {(() => {
              const change = context.change;
              const currentGroup =
                groups.find((g) => g.paths.includes(change.path))?.name ?? null;
              const close = () => setContext(null);

              return (
                <>
                  {change.unstaged != null && (
                    <div
                      className="dropdown-item"
                      onClick={() => {
                        close();
                        void stageFile(change.path, false);
                      }}
                    >
                      Stage file
                    </div>
                  )}
                  {change.staged != null && (
                    <div
                      className="dropdown-item"
                      onClick={() => {
                        close();
                        void stageFile(change.path, true);
                      }}
                    >
                      Unstage file
                    </div>
                  )}

                  <div className="dropdown-separator" />
                  <div className="group-label">Move to group</div>

                  {groups
                    .filter((group) => group.name !== currentGroup)
                    .map((group) => (
                      <div
                        key={group.name}
                        className="dropdown-item"
                        onClick={() => {
                          close();
                          void moveToGroup(change.path, group.name);
                        }}
                      >
                        {group.name}
                      </div>
                    ))}

                  {currentGroup && (
                    <div
                      className="dropdown-item"
                      onClick={() => {
                        close();
                        void moveToGroup(change.path, null);
                      }}
                    >
                      Remove from “{currentGroup}”
                    </div>
                  )}

                  <div
                    className="dropdown-item"
                    onClick={() => {
                      close();
                      setNewGroup({ name: "", pendingPath: change.path });
                    }}
                  >
                    New group…
                  </div>
                </>
              );
            })()}
          </div>
        </>
      )}

      <div className="main">
        <div className="toolbar">
          <select
            value={mode}
            onChange={(e) => setMode(e.target.value as ComparisonMode)}
          >
            {(Object.keys(MODE_LABELS) as ComparisonMode[]).map((value) => (
              <option key={value} value={value}>
                {MODE_LABELS[value]}
              </option>
            ))}
          </select>

          <button
            onClick={() => revert(selectedLines)}
            disabled={busy || !hasSelection}
            title={
              hasSelection
                ? `Revert ${selectedLines.length} selected line(s)`
                : "Click line numbers to select lines to revert"
            }
          >
            Revert selected{hasSelection ? ` (${selectedLines.length})` : ""}
          </button>
          <button
            onClick={() => shownDiff && revert(allChangedIndices(shownDiff))}
            disabled={busy || !canRevertAll}
          >
            Revert {groupHunks != null && grouping === "intent" ? "shown" : "file"}
          </button>

          <span style={{ width: 12 }} />

          <button data-command="changes.stage" onClick={() => stage(selectedLines)} disabled={busy || !selectedPath}>
            Stage{hasSelection ? " selected" : " file"}
          </button>
          <button data-command="changes.unstage" onClick={() => unstage(selectedLines)} disabled={busy || !selectedPath}>
            Unstage{hasSelection ? " selected" : " file"}
          </button>

          <span style={{ width: 12 }} />

          <button
            data-command="change.previous"
            onClick={() => diffHandle.current?.goToChange(-1)}
            disabled={differences === 0}
            title="Previous change (Shift+F7)"
            aria-label="Previous change"
          >
            ↑
          </button>
          <button
            data-command="change.next"
            onClick={() => diffHandle.current?.goToChange(1)}
            disabled={differences === 0}
            title="Next change (F7)"
            aria-label="Next change"
          >
            ↓
          </button>
          <span className="faint" style={{ fontSize: 11, whiteSpace: "nowrap" }}>
            {differences} difference{differences === 1 ? "" : "s"}
          </span>

          <span style={{ flex: 1 }} />

          <select
            value={diffLayout}
            onChange={(e) => changeDiffLayout(e.target.value as DiffLayout)}
            title="How to lay the comparison out"
          >
            <option value="sideBySide">Side by side</option>
            <option value="inline">Inline</option>
          </select>

          <select
            value={ignoreWhitespace ? "ignore" : "exact"}
            onChange={(e) => changeIgnoreWhitespace(e.target.value === "ignore")}
            title="Whitespace-only differences change how the diff is drawn, never what Stage or Revert act on"
          >
            <option value="exact">Do not ignore</option>
            <option value="ignore">Ignore whitespace</option>
          </select>

          <label
            style={{ display: "flex", gap: 4, alignItems: "center", fontSize: 11 }}
            title="Fold away long runs of unchanged code, keeping a few lines either side of each change"
          >
            <input
              type="checkbox"
              checked={collapseUnchanged}
              onChange={(e) => changeCollapse(e.target.checked)}
            />
            Collapse unchanged
          </label>

          <span className="font-size-controls">
            <button
              onClick={() => applyEditorFontSize(stepFontSize(fontSize, -1))}
              disabled={fontSize <= MIN_EDITOR_FONT_SIZE}
              title="Smaller text (Ctrl+-)"
              aria-label="Smaller text"
            >
              A−
            </button>
            <button
              onClick={() => applyEditorFontSize(stepFontSize(fontSize, 1))}
              disabled={fontSize >= MAX_EDITOR_FONT_SIZE}
              title="Larger text (Ctrl+=)"
              aria-label="Larger text"
            >
              A+
            </button>
          </span>

          <span className="faint" style={{ fontSize: 11 }}>
            Click a line number to select · ⌘S / Ctrl+S to save an edit
          </span>
        </div>

        {error && <div className="error">{error}</div>}

        <div className="content">
          {!selectedPath && (
            <div className="empty">Select a file to see its changes.</div>
          )}

          {selectedPath && diff?.isBinary && (
            <div className="empty">{selectedPath} is a binary file.</div>
          )}

          {selectedPath && contents && shownDiff && !shownDiff.isBinary && (
            contents.working == null ? (
              <div className="empty">
                {selectedPath} was deleted.
                {canRevertAll && (
                  <div style={{ marginTop: 12 }}>
                    <button onClick={() => revert(allChangedIndices(shownDiff))}>
                      Restore it
                    </button>
                  </div>
                )}
              </div>
            ) : (
              <DiffView
                path={selectedPath}
                baseline={viewBaseline}
                working={contents.working}
                diff={shownDiff}
                layout={diffLayout}
                collapseUnchanged={collapseUnchanged}
                ignoreWhitespace={ignoreWhitespace}
                editable
                onSave={save}
                onSelectionChange={setSelectedLines}
                highlight={highlight}
                uncovered={uncoveredForFile}
                risk={riskForFile}
                confidence={confidenceForOpenFile}
                handleRef={diffHandle}
              />
            )
          )}
        </div>
      </div>
    </>
  );
}

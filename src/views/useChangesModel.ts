/**
 * The working-tree model behind the Project tab's Changes rail.
 *
 * This is everything the old `ChangesView` owned that is **per view** rather
 * than per widget: the comparison mode, the git status read, the intent,
 * erosion and coverage scans, the busy/error pair every action runs under, and
 * the background poll. It was lifted out of the component for one structural
 * reason: after the Run/Changes merge the changed-file list and the diff no
 * longer live in the same component. The list is a side panel and the diff is an
 * editor tab in the shared main area, so a single owner has to sit above both —
 * otherwise the panel and the pane would each hold a `mode`, each hold a
 * `busy`, and each scan the tree, and the two would disagree the moment either
 * one acted.
 *
 * It is a hook rather than a `*Logic.ts` module because what it holds is React
 * state and effects, which vitest (node environment, no DOM) cannot exercise at
 * all. The rules it *decides* are therefore not written here — they are
 * imported from `../components/projectViewLogic`, which is tested. This file is
 * plumbing: state, effects, and IPC calls.
 */
import { useCallback, useEffect, useRef, useState } from "react";
import {
  shouldPollChanges,
  shouldRefreshChanges,
  type ChangesVisibility,
} from "../components/projectViewLogic";
import * as api from "../ipc/api";
import { reconcileSelection } from "./changesLogic";
import type {
  ChangeCoverage,
  Changelist,
  ComparisonMode,
  ErosionFlag,
  ErosionReport,
  GroupFile,
  IntentGroup,
  ProviderStatus,
  Scorecard,
  UnfulfilledClaim,
  Workspace,
  WorkingStatus,
} from "../ipc/types";

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

/**
 * How often the changes panel re-reads the working tree while it is showing, so
 * edits made outside the app (by an agent, a terminal, another tool) appear
 * without switching tabs. Gentle on purpose: each tick re-runs the intent and
 * erosion scans, and the results are de-churned so nothing re-renders unless
 * the working tree actually changed.
 */
const POLL_MS = 2000;

/** Two index arrays that hold the same values in the same order. */
function sameIndices(a: readonly number[], b: readonly number[]): boolean {
  return a.length === b.length && a.every((value, index) => value === b[index]);
}

/** How the changes panel organises the working tree. */
export type Grouping = "files" | "intent" | "stashes" | "erosion";

function loadGrouping(): Grouping {
  const stored = localStorage.getItem(GROUPING_KEY);
  return stored === "intent" || stored === "stashes" || stored === "erosion"
    ? stored
    : "files";
}

/** Everything the changes panel and the diff pane share. */
export interface ChangesModel {
  status: WorkingStatus | null;
  /** Every changed file, or an empty list before the first read. */
  files: WorkingStatus["files"];
  groups: Changelist[];
  setGroups: (groups: Changelist[]) => void;

  mode: ComparisonMode;
  setMode: (mode: ComparisonMode) => void;

  grouping: Grouping;
  changeGrouping: (next: Grouping) => void;

  /** The file the diff is showing — the row the panel paints as selected. */
  selectedPath: string | null;
  setSelectedPath: (path: string | null) => void;
  selectedGroup: string | null;
  /** Diff line indices to preselect, so opening a card lands on its lines. */
  highlight: number[];
  /** The open card's hunks in the open file, or `null` for the whole diff. */
  groupHunks: number[] | null;

  intentGroups: IntentGroup[];
  scorecard: Scorecard;
  unfulfilled: UnfulfilledClaim[];
  evidenced: UnfulfilledClaim[];
  providers: ProviderStatus[];
  setProviders: (providers: ProviderStatus[]) => void;
  intentLoading: boolean;
  erosion: ErosionReport | null;
  coverage: ChangeCoverage | null;

  busy: boolean;
  error: string | null;
  setError: (message: string | null) => void;

  /** Run an action under the busy flag and the error banner. */
  withBusy: (action: () => Promise<void>) => Promise<void>;
  /** Re-read the file list, the open diff and the three scans, in that order. */
  refreshAll: () => Promise<void>;
  refreshIntent: () => Promise<void>;
  /** Where {@link DiffPane} publishes its own "re-read my file". */
  reloadFile: { current: (() => Promise<void>) | null };

  /** Open a file's diff. Not a card: any card scoping is dropped. */
  openFile: (path: string) => void;
  /** Open one file of a card, scoping the diff to that card's hunks. */
  selectGroupFile: (group: IntentGroup, file: GroupFile) => void;
  /** Open a card: show its first file, scoped to the card. */
  selectGroup: (group: IntentGroup) => void;
  /** Open an erosion flag: show its file and highlight the offending line. */
  openErosionFlag: (flag: ErosionFlag) => void;
}

export function useChangesModel({
  workspace,
  visibility,
  onOpenDiff,
}: {
  /**
   * The workspace this model is for. Passed down rather than fetched so it
   * always matches the tab that owns it — a background poll must never read the
   * backend's *active* workspace, which may be another tab.
   */
  workspace: Workspace;
  /**
   * Whether the panel is actually on screen, in all three of its senses. Must
   * be a stable object (memoise it on its three fields), because the refresh
   * and poll effects depend on it.
   */
  visibility: ChangesVisibility;
  /**
   * Show this file's diff in the main area. Called by every "open a change"
   * entry point below, so the panel's own markup does not have to know that a
   * diff is now an editor tab rather than a pane beside it.
   */
  onOpenDiff: (path: string) => void;
}): ChangesModel {
  const [status, setStatus] = useState<WorkingStatus | null>(null);
  const [mode, setMode] = useState<ComparisonMode>("workingToHead");
  const [selectedPath, setSelectedPath] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const [intentLoading, setIntentLoading] = useState(loadGrouping() === "intent");
  const [groups, setGroups] = useState<Changelist[]>([]);
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
  const [selectedGroup, setSelectedGroup] = useState<string | null>(null);
  const [highlight, setHighlight] = useState<number[]>([]);
  const [groupHunks, setGroupHunks] = useState<number[] | null>(null);

  /**
   * The open diff pane's "re-read my file", published by `DiffPane`.
   *
   * A ref rather than state, so `refreshAll` can reload the diff in the
   * position it always occupied — after the status read, before the three
   * scans. A reload that ran last would clear an error a scan had just
   * reported.
   */
  const reloadFile = useRef<(() => Promise<void>) | null>(null);

  /**
   * Held in a ref so nothing below depends on the caller's closure identity:
   * `onOpenDiff` is an inline arrow at the one call site, and a fresh identity
   * each render would re-arm every callback that names it.
   */
  const openDiffSink = useRef(onOpenDiff);
  openDiffSink.current = onOpenDiff;

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
  // The intent recompute can take longer than the 2s poll on a large store; a
  // tick that fires while the previous one is still running must be dropped, or
  // the heavy backend work stacks up and its transient allocations pile on top of
  // each other.
  const intentInFlight = useRef(false);
  const erosionSignature = useRef<string | null>(null);
  const coverageSignature = useRef<string | null>(null);
  busyRef.current = busy;

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

  /**
   * Recompute the intent cards.
   *
   * Only while the intent view is showing: the grouping walks every changed
   * file, and there is no reason to pay for it when nothing displays it.
   */
  const refreshIntent = useCallback(async () => {
    if (grouping !== "intent") return;
    // Drop this tick if the previous recompute has not returned yet — see
    // `intentInFlight`.
    if (intentInFlight.current) return;
    // Show the "Loading intents" state only when there is nothing to show yet:
    // the very first load, or after a mode switch (which nulls the signature).
    // Every 2s poll tick calls this too, and toggling the flag on each one made
    // the indicator flash constantly even when the de-churn found no change.
    const firstLoad = intentSignature.current === null;
    if (firstLoad) setIntentLoading(true);
    intentInFlight.current = true;
    try {
      const [review, captureStatus] = await Promise.all([
        api.intentGroups(mode),
        api.intentCaptureStatus().catch(() => [] as ProviderStatus[]),
      ]);
      // De-churn (see refreshStatus): the poll recomputes the cards every tick
      // to catch edits to files already in the list, so only commit — and only
      // re-render the panel — when the result changed.
      const signature = JSON.stringify([review, captureStatus]);
      if (signature !== intentSignature.current) {
        intentSignature.current = signature;
        setIntentGroups(review.groups);
        setScorecard(review.scorecard);
        setUnfulfilled(review.unfulfilled);
        setEvidenced(review.evidenced ?? []);
        setProviders(captureStatus);
      }
    } catch (e) {
      setError(api.errorMessage(e));
    } finally {
      intentInFlight.current = false;
      if (firstLoad) setIntentLoading(false);
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

  /** Re-read the file list, the open file and the cards after a mutation. */
  const refreshAll = useCallback(async () => {
    await refreshStatus();
    await reloadFile.current?.();
    await refreshIntent();
    await refreshErosion();
    await refreshCoverage();
  }, [refreshStatus, refreshIntent, refreshErosion, refreshCoverage]);

  /**
   * Re-read git when the panel *becomes* visible.
   *
   * This restores, deliberately and by hand, something the old Changes tab got
   * for free: it was mounted conditionally, so leaving and returning remounted
   * it and its mount effect re-read the working tree. Inside an always-mounted
   * Project tab that unmount never happens. `shouldRefreshChanges` is
   * edge-triggered on becoming visible so it covers the rail switch, the inner
   * tab switch and the codebase switch alike — see its own comment for why
   * keying on the rail alone is not enough.
   */
  const previousVisibility = useRef<ChangesVisibility | null>(null);
  useEffect(() => {
    const refresh = shouldRefreshChanges(previousVisibility.current, visibility);
    previousVisibility.current = visibility;
    if (refresh) void refreshAll();
  }, [visibility, refreshAll]);

  /**
   * Poll the working tree while the panel is showing, so changes made outside
   * the app appear without switching tabs.
   *
   * The interval used to be scoped by mounting; always mounted, it would keep
   * scanning behind the file tree on every open codebase at once, so
   * `shouldPollChanges` carries that gate instead. Each of the four refreshers
   * de-churns its own result, so a quiet tick does no work beyond the read; the
   * open diff pane is deliberately left alone, so an unsaved edit in the editor
   * is never discarded under the user. A tick is skipped while an action is in
   * flight or the window is hidden.
   */
  useEffect(() => {
    // The stash view owns its own refresh, which is exactly what `paused` is
    // for — see `shouldPollChanges`.
    if (!shouldPollChanges(visibility, grouping === "stashes")) return;
    const id = window.setInterval(() => {
      if (busyRef.current || document.hidden) return;
      void refreshStatus();
      void refreshIntent();
      void refreshErosion();
      void refreshCoverage();
    }, POLL_MS);
    return () => window.clearInterval(id);
  }, [
    visibility,
    grouping,
    refreshStatus,
    refreshIntent,
    refreshErosion,
    refreshCoverage,
  ]);

  /**
   * Force a full refresh when the window becomes visible again.
   *
   * The poll skips every tick while `document.hidden`, and the intent records
   * are written by an out-of-process hook — typically while the user is watching
   * an agent run and the app is in the background. So the panel can hold a stale
   * set of cards until something forces a re-read (which is why reopening the
   * file "fixed" it). Reading the moment the window returns closes that gap.
   */
  useEffect(() => {
    const onVisible = () => {
      if (!document.hidden) void refreshAll();
    };
    document.addEventListener("visibilitychange", onVisible);
    return () => document.removeEventListener("visibilitychange", onVisible);
  }, [refreshAll]);

  /**
   * Keep an open card's highlight tied to the latest cards.
   *
   * When the cards are recomputed under a live selection (a poll tick, or the
   * visibility refresh above), the diff renumbers and the selected card's lines
   * move; a highlight captured at click time would then paint against stale line
   * numbers. Re-derive it from the current groups, and drop a selection whose
   * card genuinely went away. De-churned so an unchanged result never re-renders.
   */
  useEffect(() => {
    const next = reconcileSelection(intentGroups, selectedGroup, selectedPath);
    if (next.kind === "skip") return;
    if (next.kind === "clear") {
      setSelectedGroup(null);
      setHighlight([]);
      setGroupHunks(null);
      return;
    }
    setHighlight((prev) => (sameIndices(prev, next.highlight) ? prev : next.highlight));
    setGroupHunks((prev) =>
      prev !== null && sameIndices(prev, next.groupHunks ?? []) ? prev : next.groupHunks,
    );
  }, [intentGroups, selectedGroup, selectedPath]);

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

  /** Opening a file directly is not opening a card; drop any card selection. */
  function openFile(path: string) {
    setSelectedPath(path);
    setSelectedGroup(null);
    setHighlight([]);
    setGroupHunks(null);
    openDiffSink.current(path);
  }

  /**
   * Open one file of a card: the diff pane shows only the card's hunks in it.
   */
  function selectGroupFile(group: IntentGroup, file: GroupFile) {
    setSelectedGroup(group.id);
    setSelectedPath(file.path);
    setHighlight(file.lineIndices);
    setGroupHunks(file.hunks);
    openDiffSink.current(file.path);
  }

  /** Open a card: show its first file, scoped to the card. */
  function selectGroup(group: IntentGroup) {
    setSelectedGroup(group.id);
    const first = group.files[0];
    if (!first) return;
    selectGroupFile(group, first);
  }

  /** Open an erosion flag: show its file and highlight the offending line. */
  function openErosionFlag(flag: ErosionFlag) {
    setSelectedPath(flag.path);
    setSelectedGroup(null);
    setGroupHunks(null);
    setHighlight([flag.index]);
    openDiffSink.current(flag.path);
  }

  // Derived from the workspace prop, not fetched, for the same reason the
  // status read is: this model belongs to one tab, not to whichever workspace
  // the backend last made active.
  void workspace;

  return {
    status,
    files: status?.files ?? [],
    groups,
    setGroups,
    mode,
    setMode,
    grouping,
    changeGrouping,
    selectedPath,
    setSelectedPath,
    selectedGroup,
    highlight,
    groupHunks,
    intentGroups,
    scorecard,
    unfulfilled,
    evidenced,
    providers,
    setProviders,
    intentLoading,
    erosion,
    coverage,
    busy,
    error,
    setError,
    withBusy,
    refreshAll,
    refreshIntent,
    reloadFile,
    openFile,
    selectGroupFile,
    selectGroup,
    openErosionFlag,
  };
}

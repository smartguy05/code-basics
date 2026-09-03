import { useEffect, useState } from "react";
import { buildSections, sortFilesByRisk, statusLetter, type FileSection } from "./changesLogic";
import {
  buildFileTree,
  decodeCollapsedFolders,
  defaultFilesLayout,
  encodeCollapsedFolders,
  flattenFileTree,
  type FilesLayout,
} from "./folderTreeLogic";
import { fileRisk } from "../components/riskLogic";
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
import { StashPanel } from "../components/StashPanel";
import { ContextMenu } from "../components/ContextMenu";
import { FileIcon } from "../components/FileIcon";
import { baseName } from "../components/fileTreeLogic";
import {
  clickModifier,
  contextSelection,
  defaultStashMessage,
  stashMenuLabel,
  stashablePaths,
  toggleSelection,
} from "./changesSelectionLogic";
import * as api from "../ipc/api";
import type { ChangesModel } from "./useChangesModel";
import type {
  BehavioralReport,
  FileChange,
  GroupFile,
  InstallScope,
  IntentGroup,
  ProviderId,
  RejectSummary,
  RetireSummary,
  RunConfig,
  Workspace,
} from "../ipc/types";

const FILES_LAYOUT_KEY = "code-basics.filesLayout";
const COLLAPSED_FOLDERS_KEY = "code-basics.collapsedFolders";

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

/**
 * The working-tree **side panel** of the Project tab.
 *
 * Since the Run/Changes merge this is a panel and nothing else: the changed-file
 * list, the intent cards, the erosion flags, the stash manager and the commit
 * box. It renders no `.main` and no {@link DiffPane} — a change's diff is an
 * editor tab in the shared main area, opened by the model's `onOpenDiff` — and
 * it owns none of the state those two share. Everything per-view (the
 * comparison mode, the git read, the three scans, the busy/error pair, the
 * poll) lives in {@link ChangesModel} above both, because a panel and a pane
 * that each held their own would disagree the moment either one acted.
 *
 * What is still local is what only the panel has: the tick selection and its
 * Shift anchor, the commit message, the folded sections and folders, the
 * layout toggle, the right-click menu and the before/after picker.
 */
export function ChangesView({
  workspace,
  model,
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
   * Everything this panel shares with the diff tab in the main area. Owned by
   * the Project tab (`RunView`), which mounts the one {@link DiffPane}.
   */
  model: ChangesModel;
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
  // Everything below comes from the shared model rather than from state here.
  // Named locals rather than `model.x` at every use so the markup this file
  // renders is unchanged from when it owned them — the merge moved the state,
  // not the view.
  const {
    status,
    files,
    groups,
    setGroups,
    mode,
    grouping,
    changeGrouping,
    selectedPath,
    setSelectedPath,
    selectedGroup,
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
    setError,
    withBusy,
    refreshAll,
    refreshIntent,
    openFile,
    selectGroupFile,
    selectGroup,
    openErosionFlag,
  } = model;

  /**
   * Files ticked for a bulk action, keyed by path.
   *
   * Deliberately separate from `selectedPath`, which means "the file the diff
   * pane is showing": the same path can appear in a Staged and an Unstaged
   * section, and the actions here act on it once either way.
   */
  const [checked, setChecked] = useState<Set<string>>(() => new Set());
  /** Where a Shift-click extends from. */
  const [anchor, setAnchor] = useState<string | null>(null);
  const [message, setMessage] = useState("");
  const [amend, setAmend] = useState(false);
  /** Right-click target: where the menu sits and which file it acts on. */
  const [context, setContext] = useState<{
    x: number;
    y: number;
    change: FileChange;
    paths: string[];
  } | null>(null);
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

  // Surface the riskier files first within each section, and keep the plain git
  // order for everything unweighted. Uses the same erosion + intent signals; in
  // the files view those are quiet, so this is mostly the sensitive-path emphasis.
  const sections = sortFilesByRisk(buildSections(files, groups), (path) =>
    fileRisk(path, erosion?.flags ?? [], intentGroups),
  );
  /**
   * Every file row in the order it is drawn — what a Shift-click ranges over.
   *
   * The tree layout reorders and hides rows, so it is flattened exactly the
   * way it is rendered; ranging over the flat order there would select files
   * the user cannot see.
   */
  const orderedPaths =
    filesLayout === "tree"
      ? sections.flatMap((section) =>
          flattenFileTree(buildFileTree(section.files), (folderPath) =>
            collapsedFolders.has(`${section.key}:${folderPath}`),
          )
            .filter((row) => row.kind === "file")
            .map((row) => row.change.path),
        )
      : sections.flatMap((section) => section.files.map((file) => file.path));

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

  /**
   * Click a file row: show it, and update the tick selection the right-click
   * actions work over. Plain clicks keep the old behaviour of selecting just
   * the one row.
   */
  function selectRow(path: string, modifier: ReturnType<typeof clickModifier>) {
    const next = toggleSelection(checked, path, modifier, orderedPaths, anchor);
    setChecked(next.selected);
    setAnchor(next.anchor);
    openFile(path);
  }

  /**
   * Set the selected files aside as a stash, leaving every other change in the
   * working tree.
   */
  function stashSelected(paths: string[]) {
    // On the next frame, so the right-click menu has actually gone: a native
    // prompt blocks rendering, and closing it in the same handler would leave
    // the menu painted behind the dialog until the user answered.
    requestAnimationFrame(() => promptAndStash(paths));
  }

  function promptAndStash(paths: string[]) {
    const note = window.prompt("Stash message", defaultStashMessage(paths));
    // Cancel is not the same as an empty message: an empty one is allowed, and
    // the backend falls back to git's own "WIP on <branch>" wording.
    if (note == null) return;
    void withBusy(async () => {
      await api.gitStashPaths(note, paths);
      setChecked(new Set());
      setAnchor(null);
      if (selectedPath != null && paths.includes(selectedPath)) setSelectedPath(null);
      await refreshAll();
    });
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
          // Only once a real multi-selection exists: a plain click already
          // paints the row as selected, and marking it twice reads as noise.
          checked.size > 1 && checked.has(change.path) ? " checked" : ""
        }${risk ? ` risk-${risk.level}` : ""}`}
        style={tree ? { paddingLeft: 6 + (tree.depth + 1) * 14 } : undefined}
        onClick={(e) => selectRow(change.path, clickModifier(e))}
        onContextMenu={(e) => {
          e.preventDefault();
          const paths = contextSelection(checked, change.path);
          setChecked(paths);
          openFile(change.path);
          setContext({ x: e.clientX, y: e.clientY, change, paths: [...paths] });
        }}
        title={`${change.path} — right-click to stage, stash or group`}
      >
        <span className={`status ${className}`}>{letter}</span>
        {/* The icon sits beside the status letter, never in place of it: the
            letter is what git says happened to the file, which no icon can
            carry. In the flat layout the row shows the whole path, so the icon
            is resolved from the leaf name rather than the text on screen — a
            `src/package.json` is still a Node manifest. */}
        <FileIcon name={baseName(change.path)} />
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
          <FileIcon name={row.label} isDir expanded={!row.collapsed} />
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
              loading={intentLoading}
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
        <ContextMenu x={context.x} y={context.y} onClose={() => setContext(null)}>
          {(() => {
            const change = context.change;
            const currentGroup = groups.find((g) => g.paths.includes(change.path))?.name ?? null;
            const close = () => setContext(null);
            const stashable = stashablePaths(new Set(context.paths), files);

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

                {stashable.length > 0 && (
                  <div
                    className="dropdown-item"
                    onClick={() => {
                      close();
                      stashSelected(stashable);
                    }}
                  >
                    {stashMenuLabel(stashable.length)}
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
        </ContextMenu>
      )}

    </>
  );
}

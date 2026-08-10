import { useCallback, useEffect, useState } from "react";
import { DiffView, type DiffLayout } from "../components/DiffView";
import { allChangedIndices, onlyHunks } from "../components/diffLogic";
import { buildSections, statusLetter, type FileSection } from "./changesLogic";
import { Sidebar } from "../components/Sidebar";
import { IntentPanel } from "../components/IntentPanel";
import * as api from "../ipc/api";
import type {
  Changelist,
  ComparisonMode,
  FileChange,
  FileContents,
  FileDiff,
  GroupFile,
  InstallScope,
  IntentGroup,
  ProviderId,
  ProviderStatus,
  WorkingStatus,
} from "../ipc/types";

const DIFF_LAYOUT_KEY = "code-basics.diffLayout";
const GROUPING_KEY = "code-basics.changesGrouping";

/** How the sidebar organises the working tree. */
type Grouping = "files" | "intent";

function loadGrouping(): Grouping {
  return localStorage.getItem(GROUPING_KEY) === "intent" ? "intent" : "files";
}

function loadDiffLayout(): DiffLayout {
  return localStorage.getItem(DIFF_LAYOUT_KEY) === "inline" ? "inline" : "sideBySide";
}

const MODE_LABELS: Record<ComparisonMode, string> = {
  workingToHead: "Working tree vs HEAD",
  workingToIndex: "Unstaged (vs staged)",
  indexToHead: "Staged (vs HEAD)",
};

export function ChangesView() {
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
  const [groups, setGroups] = useState<Changelist[]>([]);
  /** Right-click target: where the menu sits and which file it acts on. */
  const [context, setContext] = useState<{ x: number; y: number; change: FileChange } | null>(
    null,
  );
  /** Sections the user folded away, by section key. */
  const [collapsed, setCollapsed] = useState<Set<string>>(() => new Set());
  /**
   * Name being typed for a new group. `null` means the input is hidden;
   * `pendingPath` is a file to drop into it as soon as it exists, so "New
   * group…" from a file's menu is one step rather than two.
   */
  const [newGroup, setNewGroup] = useState<{ name: string; pendingPath: string | null } | null>(
    null,
  );

  const [grouping, setGrouping] = useState<Grouping>(loadGrouping);
  const [intentGroups, setIntentGroups] = useState<IntentGroup[]>([]);
  const [providers, setProviders] = useState<ProviderStatus[]>([]);
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

  function changeGrouping(next: Grouping) {
    setGrouping(next);
    localStorage.setItem(GROUPING_KEY, next);
    // The files view has no cards, so nothing may stay scoped to one.
    if (next === "files") {
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
      setStatus(nextStatus);
      setGroups(nextGroups.groups);
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
    try {
      const [groups, status] = await Promise.all([
        api.intentGroups(mode),
        api.intentCaptureStatus().catch(() => [] as ProviderStatus[]),
      ]);
      setIntentGroups(groups);
      setProviders(status);
    } catch (e) {
      setError(api.errorMessage(e));
    }
  }, [grouping, mode]);

  useEffect(() => {
    void refreshIntent();
  }, [refreshIntent, status]);

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
  }, [refreshStatus, loadFile, selectedPath, mode, refreshIntent]);

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

  const enableCapture = async (provider: ProviderId, scope: InstallScope) => {
    setProviders(await api.enableIntentCapture(provider, scope));
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
  const canRevertAll = shownDiff != null && allChangedIndices(shownDiff).length > 0;
  const sections = buildSections(files, groups);

  function toggleSection(key: string) {
    setCollapsed((previous) => {
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

  function renderFileRow(change: FileChange, section: FileSection) {
    const { letter, className } = statusLetter(change, section.side);
    return (
      <button
        key={`${section.key}:${change.path}`}
        className={`row ${change.path === selectedPath ? "selected" : ""}`}
        onClick={() => openFile(change.path)}
        onContextMenu={(e) => {
          e.preventDefault();
          openFile(change.path);
          setContext({ x: e.clientX, y: e.clientY, change });
        }}
        title={`${change.path} — right-click to stage or group`}
      >
        <span className={`status ${className}`}>{letter}</span>
        <span className="path">{change.path}</span>
      </button>
    );
  }

  return (
    <>
      <Sidebar className="file-list">
        <div className="group-label">
          {status?.branch ?? "no branch"}
          {status && (status.ahead > 0 || status.behind > 0) && (
            <span className="badge" style={{ marginLeft: 6 }}>
              ↑{status.ahead} ↓{status.behind}
            </span>
          )}
        </div>

        {status?.inProgressOperation && (
          <div className="warning">A {status.inProgressOperation} is in progress.</div>
        )}

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
        </div>

        {grouping === "intent" && (
          <IntentPanel
            groups={intentGroups}
            providers={providers}
            selectedGroup={selectedGroup}
            selectedPath={selectedPath}
            busy={busy}
            onSelect={selectGroup}
            onSelectFile={selectGroupFile}
            onStage={stageGroup}
            onRevert={revertGroup}
            onStageFile={stageGroupFile}
            onRevertFile={revertGroupFile}
            onEnable={enableCapture}
            onImportHistory={importHistory}
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

              {!isCollapsed && section.files.map((change) => renderFileRow(change, section))}

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

          <button onClick={() => stage(selectedLines)} disabled={busy || !selectedPath}>
            Stage{hasSelection ? " selected" : " file"}
          </button>
          <button onClick={() => unstage(selectedLines)} disabled={busy || !selectedPath}>
            Unstage{hasSelection ? " selected" : " file"}
          </button>

          <span style={{ flex: 1 }} />

          <select
            value={diffLayout}
            onChange={(e) => changeDiffLayout(e.target.value as DiffLayout)}
            title="How to lay the comparison out"
          >
            <option value="sideBySide">Side by side</option>
            <option value="inline">Inline</option>
          </select>

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
                baseline={contents.baseline}
                working={contents.working}
                diff={shownDiff}
                layout={diffLayout}
                editable
                onSave={save}
                onSelectionChange={setSelectedLines}
                highlight={highlight}
              />
            )
          )}
        </div>
      </div>
    </>
  );
}

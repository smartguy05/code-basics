import { useCallback, useEffect, useRef, useState } from "react";
import { DiffView, type DiffLayout, type DiffViewHandle } from "../components/DiffView";
import { OutputConsole, type ConsoleHandle } from "../components/OutputConsole";
import { Sidebar } from "../components/Sidebar";
import * as api from "../ipc/api";
import type {
  Branch,
  Commit,
  FileContents,
  FileDiff,
  LineIntent,
  NetworkKind,
  WorkingStatus,
} from "../ipc/types";
import { ancestorPaths, buildTree, type BranchFolder } from "../components/treeLogic";
import {
  bulkDeleteBranches,
  bulkDeleteMessage,
  formatTime,
  intentForLine,
  whyCaption,
  whyTooltip,
} from "./historyLogic";

export function HistoryView() {
  const [commits, setCommits] = useState<Commit[]>([]);
  const [branches, setBranches] = useState<Branch[]>([]);
  const [status, setStatus] = useState<WorkingStatus | null>(null);
  const [selected, setSelected] = useState<Commit | null>(null);
  const [diffs, setDiffs] = useState<FileDiff[]>([]);
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const [showConsole, setShowConsole] = useState(false);
  /** Which of the commit's files the diff pane is showing. */
  const [selectedFile, setSelectedFile] = useState<string | null>(null);
  const [contents, setContents] = useState<FileContents | null>(null);
  /** The recorded reason behind each line of the open file, from its git note. */
  const [why, setWhy] = useState<LineIntent[]>([]);
  // The Local section starts open, Remote folded; both are toggleable.
  const [expanded, setExpanded] = useState<Set<string>>(
    () => new Set(["section:local"]),
  );
  /** Local branches ticked for bulk deletion, by name. */
  const [selectedBranches, setSelectedBranches] = useState<Set<string>>(
    () => new Set(),
  );

  const consoleRef = useRef<ConsoleHandle>(null);
  const diffHandle = useRef<DiffViewHandle | null>(null);

  /**
   * The layout preference the Changes tab owns, read rather than duplicated —
   * one setting for "how do I like diffs laid out" is enough.
   */
  const diffLayout: DiffLayout =
    localStorage.getItem("code-basics.diffLayout") === "inline" ? "inline" : "sideBySide";

  const shownDiff = diffs.find((diff) => diff.path === selectedFile) ?? null;

  const refresh = useCallback(async () => {
    try {
      const [history, branchList, currentStatus] = await Promise.all([
        api.gitHistory(200),
        api.gitBranches(),
        api.gitStatus(),
      ]);
      setCommits(history);
      setBranches(branchList);
      setStatus(currentStatus);
      // Drop ticks for branches that no longer exist (deleted, renamed) so a
      // stale name can never be handed to a later bulk delete.
      setSelectedBranches((previous) => {
        const live = new Set(branchList.map((branch) => branch.name));
        const next = new Set([...previous].filter((name) => live.has(name)));
        return next.size === previous.size ? previous : next;
      });
      // The path to the current branch starts open; the rest stays folded.
      if (currentStatus.branch) {
        setExpanded((previous) => {
          const next = new Set(previous);
          for (const path of ancestorPaths(currentStatus.branch as string)) {
            next.add(`local:${path}`);
          }
          return next;
        });
      }
      setError(null);
    } catch (e) {
      setError(api.errorMessage(e));
    }
  }, []);

  useEffect(() => {
    void refresh();
  }, [refresh]);

  useEffect(() => {
    if (!selected) {
      setDiffs([]);
      setSelectedFile(null);
      return;
    }
    api
      .gitCommitDiff(selected.id)
      .then((files) => {
        setDiffs(files);
        // Open the first file straight away: a commit detail that shows only a
        // file list is one more click than it needs to be.
        setSelectedFile(files[0]?.path ?? null);
      })
      .catch((e) => setError(api.errorMessage(e)));
  }, [selected]);

  // Both sides of the open file, as this commit left them.
  useEffect(() => {
    if (!selected || !selectedFile) {
      setContents(null);
      return;
    }
    let cancelled = false;
    api
      .gitCommitFileContents(selected.id, selectedFile)
      .then((next) => {
        if (!cancelled) setContents(next);
      })
      .catch((e) => setError(api.errorMessage(e)));
    return () => {
      cancelled = true;
    };
  }, [selected, selectedFile]);

  // The recorded reason behind each line of the open file. Absent notes and
  // unresolved lines simply come back empty — never a guessed reason.
  useEffect(() => {
    if (!selected || !selectedFile) {
      setWhy([]);
      return;
    }
    let cancelled = false;
    api
      .gitCommitFileWhy(selected.id, selectedFile)
      .then((next) => {
        if (!cancelled) setWhy(next);
      })
      .catch(() => {
        if (!cancelled) setWhy([]);
      });
    return () => {
      cancelled = true;
    };
  }, [selected, selectedFile]);

  /**
   * F7 / Shift+F7, as in the Changes tab. This view is only mounted while its
   * tab is showing, so the binding is scoped without having to check.
   */
  useEffect(() => {
    const onKeyDown = (event: KeyboardEvent) => {
      if (event.key !== "F7" || event.ctrlKey || event.altKey || event.metaKey) return;
      event.preventDefault();
      event.stopPropagation();
      diffHandle.current?.goToChange(event.shiftKey ? -1 : 1);
    };

    window.addEventListener("keydown", onKeyDown, true);
    return () => window.removeEventListener("keydown", onKeyDown, true);
  }, []);

  async function network(kind: NetworkKind) {
    setBusy(true);
    setShowConsole(true);
    consoleRef.current?.clear();
    try {
      await api.gitNetwork(kind, (event) => consoleRef.current?.handle(event));
      await refresh();
      setError(null);
    } catch (e) {
      setError(api.errorMessage(e));
    } finally {
      setBusy(false);
    }
  }

  async function act(action: () => Promise<unknown>) {
    setBusy(true);
    try {
      await action();
      await refresh();
      setError(null);
    } catch (e) {
      setError(api.errorMessage(e));
    } finally {
      setBusy(false);
    }
  }

  function toggle(id: string) {
    setExpanded((previous) => {
      const next = new Set(previous);
      if (next.has(id)) next.delete(id);
      else next.add(id);
      return next;
    });
  }

  function toggleSelected(name: string) {
    setSelectedBranches((previous) => {
      const next = new Set(previous);
      if (next.has(name)) next.delete(name);
      else next.add(name);
      return next;
    });
  }

  /**
   * Delete every ticked branch, best-effort and sequentially (see
   * `bulkDeleteBranches` for why concurrent ref deletes corrupt each other):
   * one git refuses — not fully merged, or checked out in a linked worktree —
   * does not block the rest, and the failures are reported together.
   */
  async function bulkDelete() {
    const names = [...selectedBranches];
    if (names.length === 0) return;
    const plural = names.length === 1 ? "branch" : "branches";
    if (!window.confirm(`Delete ${names.length} ${plural}?\n\n${names.join("\n")}`)) {
      return;
    }
    setBusy(true);
    try {
      const failed = await bulkDeleteBranches(
        names,
        api.gitDeleteBranch,
        api.errorMessage,
      );
      setSelectedBranches(new Set());
      await refresh();
      setError(bulkDeleteMessage(failed, names.length));
    } catch (e) {
      setError(api.errorMessage(e));
    } finally {
      setBusy(false);
    }
  }

  function renderLeaf(
    leaf: { branch: Branch; label: string },
    depth: number,
  ) {
    const { branch, label } = leaf;
    const deletable = !branch.isRemote && !branch.isHead;
    return (
      <div key={branch.name} style={{ display: "flex", gap: 4, alignItems: "center" }}>
        {deletable && (
          <input
            type="checkbox"
            checked={selectedBranches.has(branch.name)}
            disabled={busy}
            title={`Select ${branch.name} for bulk delete`}
            onChange={() => toggleSelected(branch.name)}
            style={{ marginLeft: 8 + depth * 14 }}
          />
        )}
        <button
          className={`row ${branch.isHead ? "selected" : ""}`}
          style={{ paddingLeft: deletable ? 4 : 8 + depth * 14 }}
          onClick={() =>
            act(() =>
              branch.isRemote
                ? api.gitCheckoutRemoteBranch(branch.name)
                : api.gitCheckoutBranch(branch.name),
            )
          }
          disabled={busy}
          title={
            branch.isRemote
              ? `Check out ${branch.name} as a local tracking branch`
              : (branch.upstream ? `Tracks ${branch.upstream}` : branch.name)
          }
        >
          <span style={{ flex: 1 }}>{label}</span>
          {branch.upstream && <span className="badge">tracked</span>}
        </button>
        {!branch.isRemote && !branch.isHead && (
          <button
            onClick={() => act(() => api.gitDeleteBranch(branch.name))}
            disabled={busy}
            title={`Delete ${branch.name}`}
          >
            ×
          </button>
        )}
      </div>
    );
  }

  function renderFolder(folder: BranchFolder, section: string, depth: number) {
    const id = `${section}:${folder.path}`;
    const isOpen = expanded.has(id);
    return (
      <div key={id}>
        <button
          className="row"
          style={{ paddingLeft: 8 + depth * 14 }}
          onClick={() => toggle(id)}
        >
          <span className="twisty">{isOpen ? "▾" : "▸"}</span>
          <span style={{ flex: 1 }}>{folder.label}/</span>
        </button>
        {isOpen && renderChildren(folder, section, depth + 1)}
      </div>
    );
  }

  function renderChildren(folder: BranchFolder, section: string, depth: number) {
    return (
      <>
        {folder.folders.map((child) => renderFolder(child, section, depth))}
        {folder.leaves.map((leaf) => renderLeaf(leaf, depth))}
      </>
    );
  }

  const hasUpstream = status?.upstream != null;
  const localTree = buildTree(branches.filter((branch) => !branch.isRemote));
  const remoteTree = buildTree(branches.filter((branch) => branch.isRemote));

  return (
    <>
      <Sidebar>
        <div
          className="group-label dropdown-section"
          onClick={() => toggle("section:local")}
        >
          <span className="twisty">
            {expanded.has("section:local") ? "▾" : "▸"}
          </span>
          Branches
        </div>
        {expanded.has("section:local") && renderChildren(localTree, "local", 0)}

        {selectedBranches.size > 0 && (
          <button
            className="row"
            disabled={busy}
            onClick={() => void bulkDelete()}
            title="Delete the ticked branches"
            style={{ color: "var(--fail)" }}
          >
            Delete {selectedBranches.size} selected
          </button>
        )}

        {remoteTree.folders.length + remoteTree.leaves.length > 0 && (
          <>
            <div
              className="group-label dropdown-section"
              onClick={() => toggle("section:remote")}
            >
              <span className="twisty">
                {expanded.has("section:remote") ? "▾" : "▸"}
              </span>
              Remote
            </div>
            {expanded.has("section:remote") &&
              renderChildren(remoteTree, "remote", 0)}
          </>
        )}

        <button
          className="row"
          disabled={busy}
          onClick={() => {
            const name = window.prompt("New branch name");
            if (name?.trim()) act(() => api.gitCreateBranch(name.trim(), true));
          }}
        >
          + New branch
        </button>
      </Sidebar>

      <div className="main">
        <div className="toolbar">
          <button onClick={() => network("fetch")} disabled={busy}>
            Fetch
          </button>
          <button onClick={() => network("pull")} disabled={busy}>
            Pull
          </button>
          <button
            onClick={() => network(hasUpstream ? "push" : "pushSetUpstream")}
            disabled={busy}
            title={
              hasUpstream
                ? "Push to the tracked upstream"
                : "This branch has no upstream; pushing will create one"
            }
          >
            Push{hasUpstream ? "" : " (set upstream)"}
          </button>

          {busy && <span className="spinner" />}

          <span style={{ width: 12 }} />

          <button
            onClick={() => diffHandle.current?.goToChange(-1)}
            disabled={!shownDiff}
            title="Previous change (Shift+F7)"
            aria-label="Previous change"
          >
            ↑
          </button>
          <button
            onClick={() => diffHandle.current?.goToChange(1)}
            disabled={!shownDiff}
            title="Next change (F7)"
            aria-label="Next change"
          >
            ↓
          </button>

          <span style={{ flex: 1 }} />

          <button onClick={() => setShowConsole((value) => !value)}>
            {showConsole ? "Hide output" : "Show output"}
          </button>
          <span className="muted">
            {status?.branch ?? "detached"}
            {status?.upstream ? ` → ${status.upstream}` : " (no upstream)"}
          </span>
        </div>

        {error && <div className="error">{error}</div>}

        <div className="content split">
          <div className="top">
            {commits.length === 0 && <div className="empty">No commits yet.</div>}
            {commits.map((commit) => (
              <div
                key={commit.id}
                className={`commit-row ${selected?.id === commit.id ? "selected" : ""}`}
                onClick={() => setSelected(commit)}
              >
                <div className="summary">{commit.summary}</div>
                <div className="meta">
                  <span className="mono">{commit.shortId}</span>
                  <span>{commit.authorName}</span>
                  <span>{formatTime(commit.time)}</span>
                </div>
              </div>
            ))}
          </div>

          <div className="bottom">
            {showConsole ? (
              <OutputConsole ref={consoleRef} />
            ) : selected ? (
              <div className="commit-detail">
                <div className="commit-message">
                  <strong>{selected.summary}</strong>
                  {selected.body && <pre>{selected.body}</pre>}
                </div>

                {diffs.length === 0 ? (
                  <div className="muted" style={{ padding: 8 }}>
                    This commit changed no files.
                  </div>
                ) : (
                  <>
                    <div className="commit-files">
                      {diffs.map((diff) => (
                        <button
                          key={diff.path}
                          className={`row ${diff.path === selectedFile ? "selected" : ""}`}
                          onClick={() => setSelectedFile(diff.path)}
                          title={diff.path}
                        >
                          {diff.path}
                        </button>
                      ))}
                    </div>

                    {shownDiff && shownDiff.isBinary && (
                      <div className="empty">{shownDiff.path} is a binary file.</div>
                    )}

                    {shownDiff && !shownDiff.isBinary && contents && (
                      <div className="commit-diff">
                        <DiffView
                          // Rebuilt per commit *and* per file: the editor is
                          // constructed from the document it opens on.
                          key={`${selected.id}:${shownDiff.path}`}
                          path={shownDiff.path}
                          baseline={contents.baseline}
                          // A file the commit deleted has no "after" side; show
                          // the baseline so there is something to read.
                          working={contents.working ?? contents.baseline ?? ""}
                          diff={shownDiff}
                          layout={diffLayout}
                          editable={false}
                          onSave={() => {}}
                          onSelectionChange={() => {}}
                          handleRef={diffHandle}
                          lineWhy={(line) => whyTooltip(intentForLine(why, line))}
                        />
                      </div>
                    )}

                    {whyCaption(why) && (
                      <div className="commit-why">
                        <div className="group-label">Why these lines exist</div>
                        <div className="muted" style={{ padding: "0 8px 4px", fontSize: 11 }}>
                          {whyCaption(why)} — from the agent's recorded intent at commit
                        </div>
                        {why.map((intent) => (
                          <div
                            key={`${intent.line}:${intent.turnId}`}
                            className="why-row"
                            style={{ display: "flex", gap: 6, padding: "2px 8px", fontSize: 12 }}
                            title={
                              intent.labelSource === "inferred"
                                ? "Mined from the agent's prose, not stated as a label"
                                : "Stated by the agent as its intent"
                            }
                          >
                            <span className="mono faint" style={{ minWidth: 36, textAlign: "right" }}>
                              L{intent.line}
                            </span>
                            <span style={{ flex: 1 }}>
                              {intent.label ?? "(no reason recorded)"}
                              {intent.prompt && (
                                <span
                                  className="faint"
                                  style={{ display: "block", fontSize: 11, marginTop: 1 }}
                                  title={intent.prompt}
                                >
                                  Prompt: {intent.prompt}
                                </span>
                              )}
                            </span>
                          </div>
                        ))}
                      </div>
                    )}
                  </>
                )}
              </div>
            ) : (
              <div className="empty">Select a commit to see what it changed.</div>
            )}
          </div>
        </div>
      </div>
    </>
  );
}

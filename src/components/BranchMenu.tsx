import { useCallback, useEffect, useRef, useState } from "react";
import * as api from "../ipc/api";
import type { Branch, NetworkKind, WorkingStatus } from "../ipc/types";

/**
 * Slash-named branches (`users/anthony/thing`) rendered as a directory tree:
 * each segment before the last is a collapsible folder.
 */
interface BranchFolder {
  /** Full segment path, e.g. `users/anthony`. Unique within its section. */
  path: string;
  label: string;
  folders: BranchFolder[];
  leaves: { branch: Branch; label: string }[];
}

function buildTree(branches: Branch[]): BranchFolder {
  const root: BranchFolder = { path: "", label: "", folders: [], leaves: [] };

  for (const branch of branches) {
    const parts = branch.name.split("/");
    let node = root;
    for (let i = 0; i < parts.length - 1; i++) {
      const path = parts.slice(0, i + 1).join("/");
      let child = node.folders.find((folder) => folder.path === path);
      if (!child) {
        child = { path, label: parts[i] ?? "", folders: [], leaves: [] };
        node.folders.push(child);
      }
      node = child;
    }
    node.leaves.push({ branch, label: parts[parts.length - 1] ?? branch.name });
  }
  return root;
}

/** Folder paths leading to a branch, so the current one can start expanded. */
function ancestorPaths(name: string): string[] {
  const parts = name.split("/");
  return parts.slice(0, -1).map((_, i) => parts.slice(0, i + 1).join("/"));
}

/**
 * The titlebar's branch widget, in the spirit of Rider's: the current branch
 * with ahead/behind counts, and a menu with fetch/pull/push, branch creation,
 * switching (remote branches check out as local tracking branches), and
 * deletion. Available from every tab.
 */
export function BranchMenu() {
  const [status, setStatus] = useState<WorkingStatus | null>(null);
  const [branches, setBranches] = useState<Branch[]>([]);
  const [open, setOpen] = useState(false);
  const [busy, setBusy] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  /** Outcome of the last merge — success is otherwise invisible here. */
  const [notice, setNotice] = useState<string | null>(null);
  const [draft, setDraft] = useState("");
  // The Local section starts open, Remote folded; both are toggleable.
  const [expanded, setExpanded] = useState<Set<string>>(
    () => new Set(["section:local"]),
  );
  /** Right-click target: where the context menu sits and which branch. */
  const [context, setContext] = useState<{ x: number; y: number; branch: Branch } | null>(null);
  /** Base for the next created branch. `null` means HEAD. */
  const [createFrom, setCreateFrom] = useState<Branch | null>(null);
  const nameInputRef = useRef<HTMLInputElement>(null);

  const refresh = useCallback(async () => {
    try {
      const [nextStatus, nextBranches] = await Promise.all([
        api.gitStatus(),
        api.gitBranches(),
      ]);
      setStatus(nextStatus);
      setBranches(nextBranches);
      // The path to the current branch starts open; the rest stays folded.
      if (nextStatus.branch) {
        setExpanded((previous) => {
          const next = new Set(previous);
          for (const path of ancestorPaths(nextStatus.branch as string)) {
            next.add(`local:${path}`);
          }
          return next;
        });
      }
      setError(null);
    } catch (e) {
      // Not a git repository is a normal state, not an error banner.
      setStatus(null);
      setBranches([]);
      setError(api.errorMessage(e));
    }
  }, []);

  useEffect(() => {
    void refresh();
  }, [refresh]);

  async function act(label: string, action: () => Promise<unknown>) {
    setBusy(label);
    setError(null);
    setNotice(null);
    try {
      await action();
      await refresh();
    } catch (e) {
      setError(api.errorMessage(e));
    } finally {
      setBusy(null);
    }
  }

  function network(kind: NetworkKind, label: string) {
    void act(label, async () => {
      const code = await api.gitNetwork(kind, () => {
        /* output is not shown here; the History tab has the full console */
      });
      if (code !== 0 && code !== null) {
        throw new Error(`git ${label.toLowerCase()} exited with code ${code}`);
      }
    });
  }

  /**
   * Merge a branch into the current one.
   *
   * Every outcome is reported: a merge that changed nothing, fast-forwarded,
   * or stopped on conflicts all look identical in the branch list otherwise.
   * A conflicted merge is left in progress for the Changes tab to resolve.
   */
  function merge(branch: Branch) {
    const into = status?.branch ?? "HEAD";
    void act("Merging", async () => {
      const report = await api.gitMergeBranch(branch.name);
      switch (report.outcome) {
        case "upToDate":
          setNotice(`${into} already contains ${branch.name}.`);
          break;
        case "fastForward":
          setNotice(`Fast-forwarded ${into} to ${branch.name}.`);
          break;
        case "merged":
          setNotice(`Merged ${branch.name} into ${into}.`);
          break;
        case "conflicted": {
          const count = report.conflicts?.length ?? 0;
          // `refresh` picks the in-progress merge up from git status, which
          // is what drives the Abort button — no separate flag to keep true.
          setNotice(
            `Merging ${branch.name} left ${count} conflicted file${count === 1 ? "" : "s"}. ` +
              `Resolve them in the Changes tab and commit, or abort the merge.`,
          );
          break;
        }
      }
    });
  }

  function create(name: string) {
    const trimmed = name.trim();
    const from = createFrom?.name;
    setDraft("");
    setCreateFrom(null);
    if (!trimmed) return;
    void act("Creating", () => api.gitCreateBranch(trimmed, true, from));
  }

  function toggle(id: string) {
    setExpanded((previous) => {
      const next = new Set(previous);
      if (next.has(id)) next.delete(id);
      else next.add(id);
      return next;
    });
  }

  function renderLeaf(
    leaf: { branch: Branch; label: string },
    depth: number,
  ) {
    const { branch, label } = leaf;
    return (
      <div
        key={branch.name}
        className={`dropdown-item ${branch.isHead ? "selected" : ""}`}
        style={{ paddingLeft: 8 + depth * 14 }}
        onContextMenu={(e) => {
          e.preventDefault();
          setContext({ x: e.clientX, y: e.clientY, branch });
        }}
        onClick={() => {
          if (branch.isHead || busy !== null) return;
          if (branch.isRemote) {
            void act("Switching", () => api.gitCheckoutRemoteBranch(branch.name));
          } else {
            void act("Switching", () => api.gitCheckoutBranch(branch.name));
          }
        }}
        title={
          branch.isRemote
            ? `Check out ${branch.name} as a local tracking branch`
            : (branch.upstream ? `Tracks ${branch.upstream}` : branch.name)
        }
      >
        <span style={{ flex: 1 }}>{label}</span>
        {branch.isHead && <span className="badge">current</span>}
        {!branch.isRemote && !branch.isHead && (
          <span
            className="remove"
            role="button"
            title={`Delete ${branch.name}`}
            onClick={(e) => {
              e.stopPropagation();
              if (busy === null) {
                void act("Deleting", () => api.gitDeleteBranch(branch.name));
              }
            }}
          >
            ×
          </span>
        )}
      </div>
    );
  }

  function renderFolder(folder: BranchFolder, section: string, depth: number) {
    const id = `${section}:${folder.path}`;
    const isOpen = expanded.has(id);
    return (
      <div key={id}>
        <div
          className="dropdown-item"
          style={{ paddingLeft: 8 + depth * 14 }}
          onClick={() => toggle(id)}
        >
          <span className="twisty">{isOpen ? "▾" : "▸"}</span>
          <span style={{ flex: 1 }}>{folder.label}/</span>
        </div>
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

  // No repository (or git failed entirely): show nothing rather than a
  // broken widget. The History tab surfaces the details.
  if (!status && !open) return null;

  const localTree = buildTree(branches.filter((branch) => !branch.isRemote));
  const remoteTree = buildTree(branches.filter((branch) => branch.isRemote));

  return (
    <div className="dropdown">
      <button
        onClick={() => {
          setOpen((was) => !was);
          if (!open) void refresh();
        }}
        title="Branches — switch, create, fetch/pull/push"
      >
        ⎇ {status?.branch ?? "no branch"}
        {status && status.ahead > 0 ? ` ↑${status.ahead}` : ""}
        {status && status.behind > 0 ? ` ↓${status.behind}` : ""}
        {" ▾"}
      </button>

      {open && (
        <>
          <div className="dropdown-backdrop" onClick={() => setOpen(false)} />
          <div className="dropdown-menu" style={{ minWidth: 260 }}>
            <div style={{ display: "flex", gap: 4, padding: "2px 4px 6px" }}>
              <button disabled={busy !== null} onClick={() => network("fetch", "Fetch")}>
                Fetch
              </button>
              <button disabled={busy !== null} onClick={() => network("pull", "Pull")}>
                Pull
              </button>
              <button
                disabled={busy !== null}
                onClick={() =>
                  network(status?.upstream ? "push" : "pushSetUpstream", "Push")
                }
                title={
                  status?.upstream
                    ? `Push to ${status.upstream}`
                    : "Push and set upstream"
                }
              >
                Push{status?.upstream ? "" : "…"}
              </button>
              {busy && <span className="spinner" style={{ alignSelf: "center" }} />}
            </div>

            <input
              ref={nameInputRef}
              placeholder={`New branch from ${createFrom?.name ?? "HEAD"}…`}
              value={draft}
              onChange={(e) => setDraft(e.target.value)}
              onKeyDown={(e) => {
                if (e.key === "Enter") create(draft);
                if (e.key === "Escape") {
                  // First Escape cancels a right-click base; the second
                  // closes the menu.
                  if (createFrom) setCreateFrom(null);
                  else setOpen(false);
                }
              }}
              style={{ width: "100%", marginBottom: 4 }}
            />

            <div
              className="group-label dropdown-section"
              onClick={() => toggle("section:local")}
            >
              <span className="twisty">
                {expanded.has("section:local") ? "▾" : "▸"}
              </span>
              Local
            </div>
            {expanded.has("section:local") && renderChildren(localTree, "local", 0)}

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

            {/* A merge that stopped on conflicts leaves the repository in a
                state the user has to finish or undo; nothing else in the
                titlebar would say so. */}
            {status?.inProgressOperation === "merge" && (
              <div
                className="group-label dropdown-section"
                style={{ display: "flex", alignItems: "center", gap: 6 }}
              >
                <span style={{ flex: 1 }}>Merge in progress</span>
                <button
                  disabled={busy !== null}
                  title="Discard the merge and return to the previous commit"
                  onClick={() => void act("Aborting", api.gitAbortMerge)}
                >
                  Abort
                </button>
              </div>
            )}

            {notice && (
              <div style={{ margin: "6px 0 0", fontSize: 12, opacity: 0.85 }}>
                {notice}
              </div>
            )}

            {error && (
              <div className="error" style={{ margin: "6px 0 0", fontSize: 12 }}>
                {error}
              </div>
            )}
          </div>

          {context && (
            <>
              <div
                className="dropdown-backdrop"
                style={{ zIndex: 45 }}
                onClick={() => setContext(null)}
              />
              <div
                className="dropdown-menu"
                style={{ position: "fixed", left: context.x, top: context.y, zIndex: 46 }}
              >
                <div
                  className="dropdown-item"
                  onClick={() => {
                    setCreateFrom(context.branch);
                    setContext(null);
                    setTimeout(() => nameInputRef.current?.focus(), 0);
                  }}
                >
                  New branch from {context.branch.name}…
                </div>

                {/* Merging a branch into itself is the one case with no
                    meaning, so the current branch only offers the rest. */}
                {!context.branch.isHead && (
                  <div
                    className={`dropdown-item ${busy !== null ? "muted" : ""}`}
                    title={`Merge ${context.branch.name} into ${status?.branch ?? "HEAD"}`}
                    onClick={() => {
                      if (busy !== null) return;
                      const branch = context.branch;
                      setContext(null);
                      merge(branch);
                    }}
                  >
                    Merge {context.branch.name} into {status?.branch ?? "HEAD"}
                  </div>
                )}
              </div>
            </>
          )}
        </>
      )}
    </div>
  );
}

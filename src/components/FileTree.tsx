import { ChevronsDownUp, LocateFixed } from "lucide-react";
import { useEffect, useRef, useState } from "react";
import * as api from "../ipc/api";
import type { DirEntry } from "../ipc/types";
import { registerCommand } from "../shortcuts";
import { ContextMenu } from "./ContextMenu";
import { FileIcon } from "./FileIcon";
import {
  baseName,
  createPath,
  isRenameWorthSending,
  parentDir,
  renamePath,
  targetDir,
  validateName,
  type MenuTarget,
} from "./fileTreeLogic";
import { revealPlan } from "./fileTreeRevealLogic";

/** What the name box is being used for; `null` when it is not showing. */
type Prompt =
  | { kind: "newFile" | "newFolder"; dir: string; value: string }
  | { kind: "rename"; path: string; value: string };

/**
 * A lazy directory tree of the workspace.
 *
 * One backend call per directory, made the first time it is expanded, so
 * opening a huge workspace never pays for walking the whole tree. The listing
 * is filtered like the project scan (`node_modules`, `bin`, `obj`, … hidden).
 *
 * Right-clicking a row opens a menu to create, rename and delete. Every
 * decision those make — where a new file lands, whether a typed name is usable,
 * what a rename resolves to — is in `fileTreeLogic.ts`; this component asks and
 * renders, and the backend re-validates everything anyway.
 */
export function FileTree({
  /** Re-list already-loaded directories when this changes (Rescan). */
  refreshToken,
  onOpenFile,
  /** The file currently open in the editor, highlighted in the tree. */
  activePath,
  /**
   * A path that has just been renamed away or deleted, so whoever owns the
   * editor tabs can close the one showing it. The tree cannot do that itself —
   * it does not own the open files — and leaving a tab pointing at a path that
   * no longer resolves is how a later save recreates a deleted file.
   */
  onPathGone,
}: {
  refreshToken?: unknown;
  onOpenFile: (path: string, name: string) => void;
  activePath: string | null;
  onPathGone?: (path: string) => void;
}) {
  /** Loaded listings, keyed by workspace-relative directory path ("" = root). */
  const [listings, setListings] = useState<Map<string, DirEntry[]>>(new Map());
  const [expanded, setExpanded] = useState<Set<string>>(new Set());
  const [error, setError] = useState<string | null>(null);
  const [menu, setMenu] = useState<{ x: number; y: number; target: MenuTarget | null } | null>(
    null,
  );
  const [prompt, setPrompt] = useState<Prompt | null>(null);
  const [confirmDelete, setConfirmDelete] = useState<MenuTarget | null>(null);
  const nameInput = useRef<HTMLInputElement>(null);
  const rootRef = useRef<HTMLDivElement>(null);
  const activeRow = useRef<HTMLButtonElement>(null);
  /**
   * Bumped by a completed reveal, and read by nothing but the effect that
   * scrolls. Scrolling cannot happen inside `reveal` itself: the row for a file
   * in a directory that was still collapsed does not exist until React has
   * committed the expansion, so `activeRow` is null at that moment. A token
   * rather than a boolean because revealing the same file twice must scroll
   * twice — the user may well have scrolled away in between.
   */
  const [revealToken, setRevealToken] = useState(0);

  async function load(dir: string) {
    try {
      const entries = await api.fsListDir(dir);
      setListings((previous) => new Map(previous).set(dir, entries));
      setError(null);
    } catch (e) {
      setError(api.errorMessage(e));
    }
  }

  useEffect(() => {
    // Refresh what is already on screen; unexpanded directories are re-read
    // when they are next opened anyway. `listings` is read via the setter's
    // callback form elsewhere, but here a stale snapshot only means an extra
    // refresh, so reading it directly is fine.
    void load("");
    for (const dir of listings.keys()) {
      if (dir !== "") void load(dir);
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [refreshToken]);

  useEffect(() => {
    if (prompt) nameInput.current?.focus();
  }, [prompt]);

  /**
   * Open every directory between the root and the file the editor is showing,
   * fetching the listings that are missing on the way down.
   *
   * The walk is sequential and outermost-first because the tree is lazy: a
   * directory's listing is what proves its children exist, so fetching
   * `src/views` before `src` would render a row with no parent to hang under.
   * A failure stops the walk and surfaces through the same `error` banner every
   * other backend call here uses — half an expansion with no explanation reads
   * as the button doing nothing.
   */
  async function reveal() {
    // A stale `listings` snapshot can only *under*-report what is cached, which
    // costs a redundant fetch and never a wrong tree, so reading it directly is
    // fine here just as it is in the refresh effect.
    const plan = revealPlan(activePath, listings.keys());

    try {
      const fetched = new Map<string, DirEntry[]>();
      for (const dir of plan.load) {
        fetched.set(dir, await api.fsListDir(dir));
      }
      if (fetched.size > 0) {
        setListings((previous) => {
          const next = new Map(previous);
          for (const [dir, entries] of fetched) next.set(dir, entries);
          return next;
        });
      }
      if (plan.expand.length > 0) {
        setExpanded((previous) => {
          const next = new Set(previous);
          for (const dir of plan.expand) next.add(dir);
          return next;
        });
      }
      setError(null);
      // Bumped even when the plan was empty: a top-level file needs no
      // expansion but still deserves to be scrolled to.
      setRevealToken((previous) => previous + 1);
    } catch (e) {
      setError(api.errorMessage(e));
    }
  }

  // `reveal` closes over `activePath` and `listings`, both of which change far
  // more often than the registration should. Holding the latest one in a ref
  // keeps the command registered exactly once for the life of the component.
  const revealRef = useRef(reveal);
  revealRef.current = reveal;
  const activePathRef = useRef(activePath);
  activePathRef.current = activePath;

  useEffect(() => {
    return registerCommand("tree.reveal", () => {
      // The Run tab stays mounted while hidden and there is one tree per open
      // codebase, so several are registered at once. Only the visible one
      // answers; returning false lets the dispatcher try the next.
      if (rootRef.current === null || rootRef.current.offsetParent === null) return false;
      if (activePathRef.current === null) return false;
      void revealRef.current();
      return true;
    });
  }, []);

  useEffect(() => {
    if (revealToken === 0) return;
    activeRow.current?.scrollIntoView({ block: "nearest" });
  }, [revealToken]);

  /**
   * Close every folder, keeping the listings.
   *
   * They are a cache of what the backend said, not a record of what is open, so
   * discarding them would make the next expansion pay for a round trip it has
   * already made.
   */
  function collapseAll() {
    setExpanded(new Set());
  }

  function toggle(entry: DirEntry) {
    setExpanded((previous) => {
      const next = new Set(previous);
      if (next.has(entry.path)) {
        next.delete(entry.path);
      } else {
        next.add(entry.path);
        if (!listings.has(entry.path)) void load(entry.path);
      }
      return next;
    });
  }

  /** Re-read one directory, but only if it is one we are already showing. */
  function reload(dir: string) {
    if (dir === "" || listings.has(dir)) void load(dir);
  }

  /**
   * Re-read every loaded directory at or under `dir`.
   *
   * A create can add folders several levels down (a nested name creates its
   * parents) and a delete removes a whole subtree, so refreshing only the
   * immediate parent leaves rows on screen that no longer exist.
   */
  function reloadUnder(dir: string) {
    reload(dir);
    for (const loaded of listings.keys()) {
      if (loaded !== dir && loaded.startsWith(dir === "" ? "" : `${dir}/`)) void load(loaded);
    }
  }

  function openPrompt(next: Prompt) {
    setMenu(null);
    setError(null);
    setPrompt(next);
  }

  async function submitPrompt() {
    if (!prompt) return;
    const reason = validateName(prompt.value);
    if (reason !== null) {
      setError(reason);
      return;
    }

    try {
      if (prompt.kind === "rename") {
        if (!isRenameWorthSending(prompt.path, prompt.value)) {
          setPrompt(null);
          return;
        }
        const to = renamePath(prompt.path, prompt.value);
        if (to === null) return;
        await api.fsRename(prompt.path, to);
        onPathGone?.(prompt.path);
        setPrompt(null);
        reloadUnder(parentDir(prompt.path));
        reload(parentDir(to));
        return;
      }

      const path = createPath(prompt.dir, prompt.value);
      if (path === null) return;
      if (prompt.kind === "newFile") {
        await api.fsCreateFile(path);
      } else {
        await api.fsCreateDir(path);
      }
      setPrompt(null);
      // Show what was just made: the containing folder has to be open for the
      // new row to be visible at all.
      if (prompt.dir !== "") setExpanded((previous) => new Set(previous).add(prompt.dir));
      reloadUnder(prompt.dir);
      if (prompt.kind === "newFile") onOpenFile(path, baseName(path));
    } catch (e) {
      setError(api.errorMessage(e));
    }
  }

  async function doDelete(target: MenuTarget) {
    setConfirmDelete(null);
    try {
      await api.fsDelete(target.path);
      onPathGone?.(target.path);
      reloadUnder(parentDir(target.path));
      setExpanded((previous) => {
        const next = new Set(previous);
        next.delete(target.path);
        return next;
      });
    } catch (e) {
      setError(api.errorMessage(e));
    }
  }

  function openMenu(event: React.MouseEvent, target: MenuTarget | null) {
    event.preventDefault();
    event.stopPropagation();
    setMenu({ x: event.clientX, y: event.clientY, target });
  }

  function renderDir(dir: string, depth: number) {
    const entries = listings.get(dir);
    if (!entries) {
      return (
        <div className="muted" style={{ paddingLeft: 8 + depth * 14, fontSize: 12 }}>
          Loading…
        </div>
      );
    }
    if (entries.length === 0) {
      return (
        <div className="faint" style={{ paddingLeft: 20 + depth * 14, fontSize: 12 }}>
          (empty)
        </div>
      );
    }

    return entries.map((entry) =>
      entry.isDir ? (
        <div key={entry.path}>
          <button
            className="row tree-row"
            style={{ paddingLeft: 6 + depth * 14 }}
            onClick={() => toggle(entry)}
            onContextMenu={(e) => openMenu(e, { path: entry.path, isDir: true })}
            title={`${entry.path} — right-click to add, rename or delete`}
          >
            <span className="twisty">{expanded.has(entry.path) ? "▾" : "▸"}</span>
            <FileIcon name={entry.name} isDir expanded={expanded.has(entry.path)} />
            <span className="tree-name">{entry.name}</span>
          </button>
          {expanded.has(entry.path) && renderDir(entry.path, depth + 1)}
        </div>
      ) : (
        <button
          key={entry.path}
          // Only the open file carries the ref, so it is the one row the reveal
          // effect can scroll to — and there is never more than one.
          ref={entry.path === activePath ? activeRow : undefined}
          className={`row tree-row ${entry.path === activePath ? "selected" : ""}`}
          style={{ paddingLeft: 6 + depth * 14 }}
          onClick={() => onOpenFile(entry.path, entry.name)}
          onContextMenu={(e) => openMenu(e, { path: entry.path, isDir: false })}
          title={`${entry.path} — right-click to add, rename or delete`}
        >
          {/* The empty twisty is kept as a spacer: it is what lines a file row
              up under its siblings' arrows. */}
          <span className="twisty" />
          <FileIcon name={entry.name} />
          <span className="tree-name">{entry.name}</span>
        </button>
      ),
    );
  }

  const promptLabel =
    prompt?.kind === "rename"
      ? `Rename ${baseName(prompt.path)}`
      : prompt?.kind === "newFolder"
        ? `New folder in ${prompt.dir === "" ? "the workspace root" : prompt.dir}`
        : prompt
          ? `New file in ${prompt.dir === "" ? "the workspace root" : prompt.dir}`
          : "";

  return (
    <div
      ref={rootRef}
      className="file-tree"
      // Right-clicking the empty space below the rows targets the root, so a
      // workspace with nothing in it can still have its first file made.
      onContextMenu={(e) => openMenu(e, null)}
    >
      {/* Above the rows rather than below them: the tree is as long as the
          repository, and a bar under it would scroll out of reach. Both are
          glyph-only, so both carry an `aria-label` as well as the sentence in
          the tooltip. */}
      <div className="tree-actions">
        <button
          data-command="tree.reveal"
          className="tree-action"
          onClick={() => void reveal()}
          disabled={activePath === null}
          title={
            activePath === null
              ? "Open a file first — there is nothing to select in the tree."
              : "Expand the tree down to the file open in the editor and scroll to it."
          }
          aria-label="Select opened file"
        >
          <LocateFixed />
        </button>
        <button
          data-command="tree.collapse"
          className="tree-action"
          onClick={collapseAll}
          title="Close every folder in the tree."
          aria-label="Collapse file tree"
        >
          <ChevronsDownUp />
        </button>
      </div>

      {error && (
        <div className="error" style={{ fontSize: 12 }}>
          {error}
        </div>
      )}

      {prompt && (
        <div style={{ padding: "4px 6px" }}>
          <div className="faint" style={{ fontSize: 11, marginBottom: 2 }}>
            {promptLabel}
          </div>
          <input
            ref={nameInput}
            value={prompt.value}
            placeholder={prompt.kind === "newFolder" ? "folder name" : "file name"}
            onChange={(e) => setPrompt({ ...prompt, value: e.target.value })}
            onKeyDown={(e) => {
              if (e.key === "Enter") {
                e.preventDefault();
                void submitPrompt();
              } else if (e.key === "Escape") {
                e.preventDefault();
                setPrompt(null);
                setError(null);
              }
            }}
            style={{ width: "100%" }}
          />
        </div>
      )}

      {renderDir("", 0)}

      {menu && (
        <ContextMenu x={menu.x} y={menu.y} onClose={() => setMenu(null)}>
          <div
            className="dropdown-item"
            onClick={() =>
              openPrompt({ kind: "newFile", dir: targetDir(menu.target), value: "" })
            }
          >
            New file…
          </div>
          <div
            className="dropdown-item"
            onClick={() =>
              openPrompt({ kind: "newFolder", dir: targetDir(menu.target), value: "" })
            }
          >
            New folder…
          </div>
          {((target) =>
            target && (
              <>
                <div className="dropdown-separator" />
                <div
                  className="dropdown-item"
                  onClick={() =>
                    openPrompt({
                      kind: "rename",
                      path: target.path,
                      value: baseName(target.path),
                    })
                  }
                >
                  Rename…
                </div>
                <div
                  className="dropdown-item"
                  onClick={() => {
                    setMenu(null);
                    setConfirmDelete(target);
                  }}
                >
                  Delete…
                </div>
              </>
            ))(menu.target)}
        </ContextMenu>
      )}

      {confirmDelete && (
        <div className="modal-backdrop" onClick={() => setConfirmDelete(null)}>
          <div
            className="modal"
            role="dialog"
            aria-modal="true"
            aria-label="Delete?"
            onClick={(e) => e.stopPropagation()}
          >
            <div className="modal-body">
              <h3 style={{ marginTop: 0 }}>Delete {baseName(confirmDelete.path)}?</h3>
              <p>
                {confirmDelete.isDir ? (
                  <>
                    <code>{confirmDelete.path}</code> <strong>and everything inside it</strong>{" "}
                    will be deleted.
                  </>
                ) : (
                  <>
                    <code>{confirmDelete.path}</code> will be deleted.
                  </>
                )}
              </p>
              <p>
                This is permanent: it does not go to the recycle bin, and nothing here can undo
                it. A file git already knows about can be restored from the Changes tab; one that
                was never committed cannot.
              </p>
              <div
                className="actions"
                style={{ display: "flex", justifyContent: "flex-end", gap: 8, marginTop: 16 }}
              >
                <button onClick={() => setConfirmDelete(null)}>Cancel</button>
                <button className="primary" onClick={() => void doDelete(confirmDelete)}>
                  Delete
                </button>
              </div>
            </div>
          </div>
        </div>
      )}
    </div>
  );
}

import { useEffect, useState } from "react";
import * as api from "../ipc/api";
import type { DirEntry } from "../ipc/types";

/**
 * A lazy directory tree of the workspace.
 *
 * One backend call per directory, made the first time it is expanded, so
 * opening a huge workspace never pays for walking the whole tree. The listing
 * is filtered like the project scan (`node_modules`, `bin`, `obj`, … hidden).
 */
export function FileTree({
  /** Re-list already-loaded directories when this changes (Rescan). */
  refreshToken,
  onOpenFile,
  /** The file currently open in the editor, highlighted in the tree. */
  activePath,
}: {
  refreshToken?: unknown;
  onOpenFile: (path: string, name: string) => void;
  activePath: string | null;
}) {
  /** Loaded listings, keyed by workspace-relative directory path ("" = root). */
  const [listings, setListings] = useState<Map<string, DirEntry[]>>(new Map());
  const [expanded, setExpanded] = useState<Set<string>>(new Set());
  const [error, setError] = useState<string | null>(null);

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
          >
            <span className="twisty">{expanded.has(entry.path) ? "▾" : "▸"}</span>
            <span className="tree-name">{entry.name}</span>
          </button>
          {expanded.has(entry.path) && renderDir(entry.path, depth + 1)}
        </div>
      ) : (
        <button
          key={entry.path}
          className={`row tree-row ${entry.path === activePath ? "selected" : ""}`}
          style={{ paddingLeft: 6 + depth * 14 }}
          onClick={() => onOpenFile(entry.path, entry.name)}
          title={entry.path}
        >
          <span className="twisty" />
          <span className="tree-name">{entry.name}</span>
        </button>
      ),
    );
  }

  return (
    <div className="file-tree">
      {error && (
        <div className="error" style={{ fontSize: 12 }}>
          {error}
        </div>
      )}
      {renderDir("", 0)}
    </div>
  );
}

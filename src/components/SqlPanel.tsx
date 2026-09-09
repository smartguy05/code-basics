import { useCallback, useEffect, useRef, useState } from "react";
import { SqlView } from "../views/SqlView";
import { useDockEntry } from "./DockContext";
import { dockId } from "./dockLogic";
import type { Workspace } from "../ipc/types";
import {
  clampPanelPosition,
  clampPanelSize,
  createResizeGate,
  loadPanelLayout,
  savePanelLayout,
  type PanelLayout,
  type PanelSize,
} from "./reviewLayoutLogic";
import { SQL_LAYOUT_KEY } from "./sqlPanelLogic";

/**
 * The SQL console as a floating window rather than a top-level tab.
 *
 * Modelled on **`NotesPanel`** rather than `TerminalPanel`, which is the closer
 * of the two here: there is one SQL console per codebase, so none of the
 * terminal machinery applies — no cascade offset (nothing to cascade against),
 * no per-panel stacking order, no bell. What it does share with Notes is the
 * shape of the thing: a single panel, minimizing to a labelled bar instead of
 * closing, and re-opened from the chrome by a request token rather than by a
 * boolean the caller cannot toggle twice.
 *
 * The five-part `reviewLayoutLogic` recipe is used unchanged (clamped drag,
 * clamped size, the resize gate, load, save) under the key `cb.sql.layout`.
 *
 * # What is deliberately *not* here
 *
 * {@link SqlView} is wrapped verbatim. Every decision the console makes — the
 * connections, the read-only enforcement, the streaming query — stays inside it,
 * so this file is a shell that owns a rectangle and nothing else.
 *
 * # Minimized means hidden, not unmounted
 *
 * `hidden` on the panel keeps the console mounted while it is out of sight, for
 * the reasons the old tab mount carried: it owns live database connections, a
 * CodeMirror document, and a query that may still be streaming rows — none of
 * which survives an unmount. Switching the *feature* off is the other case and
 * is the caller's: `WorkspaceTab` stops rendering this component entirely
 * (`sqlPanelMounted`), because a disabled console must actually stop running.
 */
export function SqlPanel({
  workspace,
  onClose,
  restoreRequest,
}: {
  workspace: Workspace;
  onClose: () => void;
  /**
   * Changes on every request to open the console (the titlebar action, or the
   * `view.sql` command). A minimized panel restores itself when it changes — a
   * boolean could not say "open it again" about a panel that is already open.
   */
  restoreRequest: number;
}) {
  const panelRef = useRef<HTMLDivElement>(null);
  const [minimized, setMinimized] = useState(false);

  useEffect(() => setMinimized(false), [restoreRequest]);

  const restore = useCallback(() => setMinimized(false), []);
  useDockEntry(
    minimized
      ? {
          id: dockId(workspace.root, "sql"),
          scope: workspace.root,
          label: "SQL",
          order: 0,
          status: workspace.name,
          onRestore: restore,
        }
      : null,
  );

  const [pos, setPos] = useState<PanelLayout | undefined>(() => {
    const saved = loadPanelLayout(localStorage, SQL_LAYOUT_KEY);
    return saved.left !== undefined && saved.top !== undefined ? saved : undefined;
  });
  const [size] = useState<PanelSize | undefined>(() => {
    const saved = loadPanelLayout(localStorage, SQL_LAYOUT_KEY);
    return saved.width !== undefined && saved.height !== undefined
      ? { width: saved.width, height: saved.height }
      : undefined;
  });

  // Persist the size the user drags the native grip to. The gate is what stops
  // the mount default and the 0×0 measurement a minimize produces from being
  // written over a real one (see `createResizeGate`).
  useEffect(() => {
    const panel = panelRef.current;
    if (!panel || typeof ResizeObserver !== "function") return;
    const gate = createResizeGate();
    let timer: ReturnType<typeof setTimeout> | undefined;
    const observer = new ResizeObserver(() => {
      const width = panel.offsetWidth;
      const height = panel.offsetHeight;
      if (!gate.persist({ width, height })) return;
      if (timer) clearTimeout(timer);
      timer = setTimeout(() => {
        const clamped = clampPanelSize(
          { width, height },
          { width: window.innerWidth, height: window.innerHeight },
        );
        const saved = loadPanelLayout(localStorage, SQL_LAYOUT_KEY);
        savePanelLayout(localStorage, { ...saved, ...clamped }, SQL_LAYOUT_KEY);
      }, 200);
    });
    observer.observe(panel);
    return () => {
      if (timer) clearTimeout(timer);
      observer.disconnect();
    };
  }, []);

  // Drag by the header — identical to NotesPanel/TerminalPanel; the clamp is
  // pure. A press that never moves stays a click, so the header buttons work.
  const onHeaderPointerDown = (e: React.PointerEvent<HTMLDivElement>) => {
    if (e.button !== 0) return;
    if ((e.target as HTMLElement).closest("button, input, textarea, select")) return;
    const panel = panelRef.current;
    if (!panel) return;

    const rect = panel.getBoundingClientRect();
    const grabX = e.clientX - rect.left;
    const grabY = e.clientY - rect.top;
    const header = e.currentTarget;
    header.setPointerCapture(e.pointerId);

    let latest: PanelLayout = { left: rect.left, top: rect.top };
    let moved = false;
    const onMove = (ev: PointerEvent) => {
      moved = true;
      const s = { width: panel.offsetWidth, height: panel.offsetHeight };
      const viewport = { width: window.innerWidth, height: window.innerHeight };
      latest = clampPanelPosition(
        { left: ev.clientX - grabX, top: ev.clientY - grabY },
        s,
        viewport,
      );
      setPos(latest);
    };
    const onUp = () => {
      header.releasePointerCapture(e.pointerId);
      header.removeEventListener("pointermove", onMove);
      header.removeEventListener("pointerup", onUp);
      if (moved) savePanelLayout(localStorage, latest, SQL_LAYOUT_KEY);
    };
    header.addEventListener("pointermove", onMove);
    header.addEventListener("pointerup", onUp);
  };

  return (
    <>
      {/* Minimized pill lives in the shared dock now (see `useDockEntry` above). */}
      {/* `.sql-panel` is the positioned, clipping ancestor the two overlays
          `SqlView` renders — the connection picker and the writes confirmation —
          are scoped to in the stylesheet. Both are `position: fixed` there,
          which is right for a full-screen tab and wrong inside a window, where
          they would cover the whole app from a band above every terminal. */}
      <div
        className="review-panel sql-panel"
        hidden={minimized}
        ref={panelRef}
        style={{
          ...(pos ? { left: pos.left, top: pos.top, right: "auto", bottom: "auto" } : {}),
          ...(size ? { width: size.width, height: size.height } : {}),
        }}
      >
        <div className="review-header" onPointerDown={onHeaderPointerDown}>
          <strong>SQL</strong>
          <span className="faint" style={{ fontSize: 12 }}>
            {workspace.name}
          </span>
          <span style={{ flex: 1 }} />
          <button onClick={() => setMinimized(true)} title="Minimize (keeps the connection)">
            —
          </button>
          <button onClick={onClose} title="Close (ends the session)">
            ✕
          </button>
        </div>

        <SqlView workspace={workspace} />
      </div>
    </>
  );
}

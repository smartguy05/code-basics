import { useEffect, useRef, useState } from "react";
import type { RunningReport } from "../ipc/types";
import {
  clampPanelPosition,
  clampPanelSize,
  createResizeGate,
  loadPanelLayout,
  savePanelLayout,
  type PanelLayout,
  type PanelSize,
} from "./reviewLayoutLogic";
import {
  formatAge,
  hasOutput,
  isEmpty,
  killRequest,
  kindIcon,
  kindLabel,
  rootBasename,
  type KillRequest,
} from "./runningLogic";

/** The layout key for the Running panel's remembered position/size. */
const RUNNING_LAYOUT_KEY = "cb.running.layout";

/**
 * The global "what is running" panel.
 *
 * A floating panel modelled on `NotesPanel`/`ReviewPanel` (draggable header,
 * native resize, layout persisted via `reviewLayoutLogic`) — but **open/close
 * only, never minimized to a pill**, so it does not contend for the bottom-right
 * pill slots the Notes bar and terminal pills partition. It renders the
 * `RunningReport` the app polls for; every decision (labels, icons, the codebase
 * name, ages, the kill request) lives in the pure, tested `runningLogic`.
 */
export function RunningPanel({
  report,
  onKill,
  onRefresh,
  onViewOutput,
  onClose,
}: {
  report: RunningReport | null;
  /** Kill one process; the app forwards to `api.killRunning` and refreshes. */
  onKill: (req: KillRequest) => void;
  /** Ask the app to refresh the list now (the manual Refresh button). */
  onRefresh: () => void;
  /**
   * Focus a launched app's output tab, by the record's `key`. Offered only for
   * the rows `hasOutput` admits — every other kind's output lives somewhere this
   * panel cannot reach, and a button that did nothing would be worse than none.
   */
  onViewOutput: (key: string) => void;
  onClose: () => void;
}) {
  const panelRef = useRef<HTMLDivElement>(null);
  const now = Date.now();

  const [pos, setPos] = useState<PanelLayout | undefined>(() => {
    const saved = loadPanelLayout(localStorage, RUNNING_LAYOUT_KEY);
    return saved.left !== undefined && saved.top !== undefined ? saved : undefined;
  });
  const [size] = useState<PanelSize | undefined>(() => {
    const saved = loadPanelLayout(localStorage, RUNNING_LAYOUT_KEY);
    return saved.width !== undefined && saved.height !== undefined
      ? { width: saved.width, height: saved.height }
      : undefined;
  });

  // Persist the size the user drags the grip to (see ReviewPanel for the gate
  // reasoning). Shared key, so the panel reopens at its last size.
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
        const saved = loadPanelLayout(localStorage, RUNNING_LAYOUT_KEY);
        savePanelLayout(localStorage, { ...saved, ...clamped }, RUNNING_LAYOUT_KEY);
      }, 200);
    });
    observer.observe(panel);
    return () => {
      if (timer) clearTimeout(timer);
      observer.disconnect();
    };
  }, []);

  // Drag by the header — identical clamp to the other floating panels.
  const onHeaderPointerDown = (e: React.PointerEvent<HTMLDivElement>) => {
    if (e.button !== 0) return;
    if ((e.target as HTMLElement).closest("button")) return;
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
      latest = clampPanelPosition({ left: ev.clientX - grabX, top: ev.clientY - grabY }, s, viewport);
      setPos(latest);
    };
    const onUp = () => {
      header.releasePointerCapture(e.pointerId);
      header.removeEventListener("pointermove", onMove);
      header.removeEventListener("pointerup", onUp);
      if (moved) savePanelLayout(localStorage, latest, RUNNING_LAYOUT_KEY);
    };
    header.addEventListener("pointermove", onMove);
    header.addEventListener("pointerup", onUp);
  };

  const live = report?.live ?? [];
  const orphans = report?.orphans ?? [];
  const warnings = report?.warnings ?? [];

  return (
    <div
      className="review-panel running-panel"
      ref={panelRef}
      style={{
        ...(pos ? { left: pos.left, top: pos.top, right: "auto", bottom: "auto" } : {}),
        ...(size ? { width: size.width, height: size.height } : {}),
      }}
    >
      <div className="review-header" onPointerDown={onHeaderPointerDown}>
        <strong>Running</strong>
        <span className="faint" style={{ fontSize: 12 }}>
          {live.length} active
        </span>
        <span style={{ flex: 1 }} />
        <button onClick={onRefresh} title="Refresh now">
          ↻
        </button>
        <button onClick={onClose} title="Close">
          ✕
        </button>
      </div>

      <div className="running-body">
        {isEmpty(report) ? (
          <div className="running-empty">Nothing is running.</div>
        ) : (
          <>
            {live.length > 0 && (
              <div className="running-section">
                <div className="running-section-title">Running ({live.length})</div>
                {live.map((r) => (
                  <div className="running-row" key={`live:${r.root}:${r.key}`}>
                    <span className="running-icon" title={kindLabel(r.kind)}>
                      {kindIcon(r.kind)}
                    </span>
                    <span className="running-label" title={`${r.program} · pid ${r.pid}`}>
                      {r.label}
                    </span>
                    <span className="running-meta">{rootBasename(r.root)}</span>
                    <span className="running-meta">pid {r.pid}</span>
                    <span className="running-meta">{formatAge(r.startedAtMs, now)}</span>
                    {hasOutput(r) && (
                      <button
                        className="running-view"
                        title="Show this app's output"
                        onClick={() => onViewOutput(r.key)}
                      >
                        View
                      </button>
                    )}
                    <button className="running-kill" onClick={() => onKill(killRequest(r, false))}>
                      Kill
                    </button>
                  </div>
                ))}
              </div>
            )}

            {orphans.length > 0 && (
              <div className="running-section">
                <div className="running-section-title">Possible orphans ({orphans.length})</div>
                <div className="running-note">
                  Still running from a previous session — the app did not start these this time.
                </div>
                {orphans.map((r) => (
                  <div className="running-row orphan" key={`orphan:${r.pid}`}>
                    <span className="running-icon" title={kindLabel(r.kind)}>
                      ☠
                    </span>
                    <span className="running-label" title={`${r.program} · pid ${r.pid}`}>
                      {r.label}
                    </span>
                    <span className="running-meta">{rootBasename(r.root)}</span>
                    <span className="running-meta">pid {r.pid}</span>
                    <button
                      className="running-kill"
                      onClick={() => {
                        if (
                          window.confirm(
                            `Kill "${r.label}" (pid ${r.pid})? It was verified as the recorded process.`,
                          )
                        ) {
                          onKill(killRequest(r, true));
                        }
                      }}
                    >
                      Kill
                    </button>
                  </div>
                ))}
              </div>
            )}

            {warnings.length > 0 && (
              <div className="running-section">
                {warnings.map((w, i) => (
                  <div className="running-warning" key={i}>
                    {w}
                  </div>
                ))}
              </div>
            )}
          </>
        )}
      </div>
    </div>
  );
}

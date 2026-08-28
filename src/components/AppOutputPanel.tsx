import { useEffect, useRef, useState } from "react";
import { OutputConsole, type ConsoleHandle } from "./OutputConsole";
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
  APP_OUTPUT_LAYOUT_KEY,
  canStop,
  liveTabCount,
  statusText,
  tabTitle,
  type AppTab,
} from "./appOutputLogic";
import type { Severity } from "./consoleLogic";

/**
 * The output of everything the launcher has started: one floating panel with a
 * tab per launched app.
 *
 * One panel rather than one per app, because several background services running
 * at once is the normal case and a dozen floating windows is not. Hosted at the
 * app level (like `NotesPanel`) since a launched app belongs to no codebase.
 *
 * Two lifetime rules it exists to keep:
 *
 * * **Every console stays mounted** while its tab exists — hidden tabs included,
 *   which `OutputConsole` already handles (it skips fitting when it has no
 *   `offsetParent`). Unmounting one would throw away the scrollback of a process
 *   that is still running.
 * * **A tab outlives its process.** The Running panel drops a row the instant a
 *   process exits, so after an exit this is the only place the exit code and the
 *   output survive; nothing here closes a tab on exit.
 *
 * The toolbar's severity picker narrows the active tab's console to lines at or
 * above a level, **per tab** — two services running at once are usually being
 * watched for different reasons. The threshold lives on the tab rather than
 * inside the console so it survives the panel being hidden, and the ranking
 * itself is `consoleLogic`'s: a level the tool wrote wins, and only an unmarked
 * line falls back to the stream it came from.
 *
 * Every decision (tab titles, status wording, whether Stop applies) lives in the
 * pure, tested `appOutputLogic`.
 */
export function AppOutputPanel({
  tabs,
  activeKey,
  hidden,
  onSelect,
  onCloseTab,
  onStop,
  onClose,
  onSeverityChange,
  registerConsole,
}: {
  tabs: AppTab[];
  activeKey: string | null;
  /** Kept mounted but out of sight when the panel is closed (see above). */
  hidden: boolean;
  onSelect: (key: string) => void;
  onCloseTab: (key: string) => void;
  onStop: (key: string) => void;
  onClose: () => void;
  onSeverityChange: (key: string, severity: Severity) => void;
  /** Hand each tab's console to the app, which routes process output into it. */
  registerConsole: (key: string, handle: ConsoleHandle | null) => void;
}) {
  const panelRef = useRef<HTMLDivElement>(null);

  const [pos, setPos] = useState<PanelLayout | undefined>(() => {
    const saved = loadPanelLayout(localStorage, APP_OUTPUT_LAYOUT_KEY);
    return saved.left !== undefined && saved.top !== undefined ? saved : undefined;
  });
  const [size] = useState<PanelSize | undefined>(() => {
    const saved = loadPanelLayout(localStorage, APP_OUTPUT_LAYOUT_KEY);
    return saved.width !== undefined && saved.height !== undefined
      ? { width: saved.width, height: saved.height }
      : undefined;
  });

  // Persist the dragged size (the gate reasoning is in ReviewPanel).
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
        const saved = loadPanelLayout(localStorage, APP_OUTPUT_LAYOUT_KEY);
        savePanelLayout(localStorage, { ...saved, ...clamped }, APP_OUTPUT_LAYOUT_KEY);
      }, 200);
    });
    observer.observe(panel);
    return () => {
      if (timer) clearTimeout(timer);
      observer.disconnect();
    };
  }, []);

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
      if (moved) savePanelLayout(localStorage, latest, APP_OUTPUT_LAYOUT_KEY);
    };
    header.addEventListener("pointermove", onMove);
    header.addEventListener("pointerup", onUp);
  };

  const active = tabs.find((t) => t.key === activeKey) ?? null;
  const live = liveTabCount(tabs);

  return (
    <div
      className="review-panel app-output-panel"
      ref={panelRef}
      style={{
        ...(hidden ? { display: "none" } : {}),
        ...(pos ? { left: pos.left, top: pos.top, right: "auto", bottom: "auto" } : {}),
        ...(size ? { width: size.width, height: size.height } : {}),
      }}
    >
      <div className="review-header" onPointerDown={onHeaderPointerDown}>
        <strong>Apps</strong>
        <span className="faint" style={{ fontSize: 12 }}>
          {live} running
        </span>
        <span style={{ flex: 1 }} />
        <button onClick={onClose} title="Close (the apps keep running)">
          ✕
        </button>
      </div>

      <div className="app-output-tabs">
        {tabs.map((tab) => (
          <div
            className={`app-output-tab${tab.key === activeKey ? " active" : ""}`}
            key={tab.key}
            onClick={() => onSelect(tab.key)}
            title={`${tab.cwd} · ${statusText(tab.status)}`}
          >
            <span className={`app-output-dot ${tab.status.kind}`} />
            <span className="app-output-title">{tabTitle(tabs, tab)}</span>
            <button
              className="app-output-close"
              title={canStop(tab) ? "Stop and close this tab" : "Close this tab"}
              onClick={(e) => {
                e.stopPropagation();
                onCloseTab(tab.key);
              }}
            >
              ✕
            </button>
          </div>
        ))}
      </div>

      {active !== null && (
        <div className="app-output-toolbar">
          <button disabled={!canStop(active)} onClick={() => onStop(active.key)}>
            Stop
          </button>
          <select
            value={active.severity}
            onChange={(e) => onSeverityChange(active.key, e.target.value as Severity)}
            title="Hide output below this severity"
          >
            <option value="all">All levels</option>
            <option value="info">Info+</option>
            <option value="warn">Warn+</option>
            <option value="error">Errors</option>
          </select>
          <span className="app-output-meta">{statusText(active.status)}</span>
          {active.pid !== null && <span className="app-output-meta">pid {active.pid}</span>}
          <span className="app-output-meta" title={active.cwd}>
            {active.cwd}
          </span>
        </div>
      )}

      <div className="app-output-body">
        {tabs.map((tab) => (
          <div
            className="app-output-console"
            key={tab.key}
            // Hidden, never unmounted: the scrollback of a running process must
            // survive a tab switch.
            style={tab.key === activeKey ? undefined : { display: "none" }}
          >
            <OutputConsole
              ref={(handle) => registerConsole(tab.key, handle)}
              severity={tab.severity}
              onSeverityChange={(severity) => onSeverityChange(tab.key, severity)}
            />
          </div>
        ))}
        {tabs.length === 0 && <div className="running-empty">Nothing launched yet.</div>}
      </div>
    </div>
  );
}

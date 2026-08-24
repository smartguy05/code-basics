import { useEffect, useRef, useState } from "react";
import { TerminalView, type TerminalViewHandle } from "./TerminalView";
import * as api from "../ipc/api";
import type { TerminalEvent } from "../ipc/types";
import {
  clampPanelPosition,
  clampPanelSize,
  createResizeGate,
  loadPanelLayout,
  savePanelLayout,
  type PanelLayout,
  type PanelSize,
} from "./reviewLayoutLogic";
import { cascadeShift, outputNeedsAttention, TERMINAL_LAYOUT_KEY } from "./terminalLogic";

/**
 * One floating, interactive terminal.
 *
 * A DOM floating panel modelled on `ReviewPanel` — draggable by its header,
 * resizable by the native grip, and minimizing to a pill rather than closing —
 * hosting a raw {@link TerminalView} over a PTY session. Hosted at app level so
 * it survives tab switches, and its console keeps streaming while minimized.
 *
 * The reusable layout arithmetic (clamp, persist, resize-gate) is shared with
 * `ReviewPanel` via `reviewLayoutLogic`; only the persistence **key** differs,
 * so several terminals remember one shared size/position and a cascade offset
 * keeps freshly opened ones from landing exactly on top of each other.
 */
export function TerminalPanel({
  title,
  index,
  onClose,
}: {
  title: string;
  /** Position among the currently open terminals, for the cascade offset. */
  index: number;
  onClose: () => void;
}) {
  const viewRef = useRef<TerminalViewHandle>(null);
  const panelRef = useRef<HTMLDivElement>(null);

  // The PTY session id, once `terminal_open` resolves. Keystrokes and resizes
  // produced before then are dropped — xterm fires neither until the view is
  // focused, which is after open completes.
  const sessionRef = useRef<string | null>(null);
  // The latest measured size, so `open` can start the PTY matched to the view
  // even though the measurement arrives (from a child effect) before open runs.
  const sizeRef = useRef<{ cols: number; rows: number }>({ cols: 80, rows: 24 });
  // Read inside the stable output handler without re-subscribing.
  const minimizedRef = useRef(false);

  const [pos, setPos] = useState<PanelLayout | undefined>(() => {
    const saved = loadPanelLayout(localStorage, TERMINAL_LAYOUT_KEY);
    return saved.left !== undefined && saved.top !== undefined ? saved : undefined;
  });
  const [size] = useState<PanelSize | undefined>(() => {
    const saved = loadPanelLayout(localStorage, TERMINAL_LAYOUT_KEY);
    return saved.width !== undefined && saved.height !== undefined
      ? { width: saved.width, height: saved.height }
      : undefined;
  });
  const [minimized, setMinimized] = useState(false);
  const [attention, setAttention] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [exited, setExited] = useState(false);

  minimizedRef.current = minimized;

  // Open the PTY once, and stream its output straight into the view. The
  // effect runs after the child TerminalView has mounted (child effects run
  // first), so `viewRef` and the initial size are already set.
  useEffect(() => {
    let alive = true;
    const { cols, rows } = sizeRef.current;
    void api
      .terminalOpen(cols, rows, (event: TerminalEvent) => {
        if (!alive) return;
        handleEvent(event);
      })
      .then((id) => {
        if (!alive) {
          // Unmounted before open resolved — do not leak the session.
          void api.terminalClose(id).catch(() => {});
          return;
        }
        sessionRef.current = id;
        viewRef.current?.focus();
      })
      .catch((e) => {
        if (alive) setError(String(e));
      });

    return () => {
      alive = false;
      const id = sessionRef.current;
      if (id) void api.terminalClose(id).catch(() => {});
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  function handleEvent(event: TerminalEvent) {
    switch (event.type) {
      case "output":
        viewRef.current?.write(event.text);
        if (outputNeedsAttention(minimizedRef.current, event.text)) setAttention(true);
        break;
      case "exited": {
        const note =
          event.code === null
            ? "\r\n\x1b[33m[terminal ended]\x1b[0m\r\n"
            : `\r\n\x1b[${event.success ? 32 : 31}m[terminal exited: ${event.code}]\x1b[0m\r\n`;
        viewRef.current?.write(note);
        setExited(true);
        // The exit is worth noticing if the pane is tucked away.
        if (minimizedRef.current) setAttention(true);
        break;
      }
      case "failed":
        viewRef.current?.write(`\r\n\x1b[31m${event.message}\x1b[0m\r\n`);
        setError(event.message);
        if (minimizedRef.current) setAttention(true);
        break;
    }
  }

  const onData = (data: string) => {
    const id = sessionRef.current;
    if (id) void api.terminalWrite(id, data).catch(() => {});
  };

  const onResize = (cols: number, rows: number) => {
    sizeRef.current = { cols, rows };
    const id = sessionRef.current;
    if (id) void api.terminalResize(id, cols, rows).catch(() => {});
  };

  // Persist the size the user drags the native grip to (see ReviewPanel for the
  // ResizeObserver reasoning). Shared key, so all terminals adopt the last size.
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
        const saved = loadPanelLayout(localStorage, TERMINAL_LAYOUT_KEY);
        savePanelLayout(localStorage, { ...saved, ...clamped }, TERMINAL_LAYOUT_KEY);
      }, 200);
    });
    observer.observe(panel);
    return () => {
      if (timer) clearTimeout(timer);
      observer.disconnect();
    };
  }, []);

  // Restoring the panel acknowledges the flash, and re-fits the terminal to the
  // size it now has.
  const restore = () => {
    setMinimized(false);
    setAttention(false);
    // The fit must wait for the panel to become visible again this frame.
    setTimeout(() => {
      viewRef.current?.fit();
      viewRef.current?.focus();
    }, 0);
  };

  const close = () => {
    const id = sessionRef.current;
    if (id) void api.terminalClose(id).catch(() => {});
    onClose();
  };

  // Drag by the header. Identical to ReviewPanel: a press that never moves is a
  // click, so the minimize/close buttons still work; the clamp is pure.
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
      const size = { width: panel.offsetWidth, height: panel.offsetHeight };
      const viewport = { width: window.innerWidth, height: window.innerHeight };
      latest = clampPanelPosition(
        { left: ev.clientX - grabX, top: ev.clientY - grabY },
        size,
        viewport,
      );
      setPos(latest);
    };
    const onUp = () => {
      header.releasePointerCapture(e.pointerId);
      header.removeEventListener("pointermove", onMove);
      header.removeEventListener("pointerup", onUp);
      if (moved) savePanelLayout(localStorage, latest, TERMINAL_LAYOUT_KEY);
    };
    header.addEventListener("pointermove", onMove);
    header.addEventListener("pointerup", onUp);
  };

  // A fresh terminal with no remembered position is nudged by the cascade so it
  // does not land exactly on the previous one; a dragged/stored position wins.
  const shift = cascadeShift(index);
  const status = error ? "error" : exited ? "exited" : "running";

  return (
    <>
      {minimized && (
        <button
          className={`review-pill${attention ? " attention" : ""}`}
          onClick={restore}
          title={attention ? "The terminal needs your attention" : "Restore the terminal"}
          // Stack pills upward so several minimized terminals don't land on the
          // same spot (the CSS anchors every pill bottom-right by default).
          style={{ bottom: 16 + index * 48 }}
        >
          <span>
            {title} — {attention ? "needs attention" : status}
          </span>
        </button>
      )}

      <div
        className="review-panel terminal-panel"
        hidden={minimized}
        ref={panelRef}
        style={{
          ...(pos
            ? { left: pos.left, top: pos.top, right: "auto", bottom: "auto" }
            : shift
              ? { transform: `translate(${-shift}px, ${-shift}px)` }
              : {}),
          ...(size ? { width: size.width, height: size.height } : {}),
        }}
      >
        <div
          className={`review-header${attention ? " attention" : ""}`}
          onPointerDown={onHeaderPointerDown}
        >
          <strong>{title}</strong>
          <span className="faint" style={{ fontSize: 12 }}>
            {status}
          </span>
          <span style={{ flex: 1 }} />
          <button onClick={() => setMinimized(true)} title="Minimize (keeps running)">
            —
          </button>
          <button onClick={close} title="Close (ends the terminal)">
            ✕
          </button>
        </div>

        {error && <div className="warning">{error}</div>}

        <div className="terminal-body">
          <TerminalView ref={viewRef} onData={onData} onResize={onResize} />
        </div>
      </div>
    </>
  );
}

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
import {
  acceptedTerminalTitle,
  cascadeShift,
  outputNeedsAttention,
  pillBottom,
  terminalLayoutKey,
} from "./terminalLogic";
import { MAX_LABEL_LENGTH } from "./workspaceRenameLogic";
import { ContextMenu } from "./ContextMenu";
import { PillColorMenu } from "./PillColorMenu";

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
  cwd,
  command,
  index,
  stackOffset,
  color,
  workspaceActive,
  onClose,
  onRaise,
  onAttentionChange,
  onCompleted,
  onRename,
  onRecolor,
}: {
  title: string;
  /**
   * The workspace root this terminal runs in — its PTY cwd, and the scope for
   * its saved layout. Passed explicitly so the terminal stays in its own
   * repository even after its tab is backgrounded.
   */
  cwd: string;
  /**
   * What to run instead of the default shell — an interactive agent seeded with
   * a question, for "Ask the codebase". Omitted for an ordinary terminal.
   *
   * Passed as a program plus an argv, never as a command line to be parsed: the
   * PTY spawns through `CommandBuilder` with these arguments as they stand, so
   * through a real executable a question with a quote, a newline or a `&` in it
   * arrives intact. A Windows `.cmd`/`.bat` shim is the exception — `cmd.exe`
   * re-parses the command line there, so the backend refuses an argument
   * carrying `&`, `|`, `<`, `>`, `^`, `"` or `%` before spawning and the open
   * fails with that reason (surfaced by the open effect below).
   *
   * The question is an **argument, not typed keystrokes**, for two further
   * reasons. Typing a multi-line question into an agent's TUI would submit it at
   * the first `
`, asking a fragment; and this panel resolves its session id
   * asynchronously (see `sessionRef`), so anything written before
   * `terminal_open` resolves is dropped on the floor.
   *
   * Read once, by the mount-once open effect below — changing it later has no
   * effect, which is why the host mints a new terminal per ask rather than
   * re-pointing an existing one.
   */
  command?: { program: string; args: string[] };
  /** Position among the currently open terminals, for the cascade offset. */
  index: number;
  /**
   * This terminal's step within the terminal stacking band — how far in front of
   * the other terminals it sits. Separate from `index` on purpose: `index` is
   * positional (it places the pill and the cascade) and this is temporal (it
   * records what was clicked last). See `terminalLogic.stackOffset`.
   */
  stackOffset: number;
  /** User-chosen minimized-pill background, or undefined for the theme default. */
  color?: string;
  /** Whether this terminal's workspace is currently in the foreground. */
  workspaceActive: boolean;
  onClose: () => void;
  /** Bring this terminal in front of the others. Idempotent when already top. */
  onRaise?: () => void;
  /**
   * Report this terminal's attention flag upward so its (possibly hidden)
   * workspace tab can flash. Fired whenever the flag changes, and cleared to
   * `false` when the terminal closes.
   */
  onAttentionChange?: (attention: boolean) => void;
  /**
   * Report how this terminal's process finished while it was out of sight.
   *
   * Minimized only, for the same reason the bell is: a terminal you are looking
   * at has already told you it finished, in words, on its own last line. And
   * unlike the bell this is a one-shot — "it finished" is a fact about a moment,
   * not a state the terminal is in, so there is nothing to clear and the tab
   * signal it raises expires on its own.
   */
  onCompleted?: (success: boolean) => void;
  /**
   * Commit a new title. The panel deliberately sends the raw text: the host's
   * `renameTerminal` owns the cleaning and the refusal (control/bidi stripping,
   * whitespace collapsing, the code-point cap) so one rule decides what a
   * terminal may be called, wherever the rename came from.
   */
  onRename?: (title: string) => void;
  /** Set/clear the minimized-pill colour. */
  onRecolor?: (color: string | undefined) => void;
}) {
  const viewRef = useRef<TerminalViewHandle>(null);
  const panelRef = useRef<HTMLDivElement>(null);
  // Layout is remembered per workspace, so a terminal in one codebase does not
  // inherit the geometry of one in another.
  const layoutKey = terminalLayoutKey(cwd);

  // The PTY session id, once `terminal_open` resolves. Keystrokes and resizes
  // produced before then are dropped — xterm fires neither until the view is
  // focused, which is after open completes.
  const sessionRef = useRef<string | null>(null);
  // The latest measured size, so `open` can start the PTY matched to the view
  // even though the measurement arrives (from a child effect) before open runs.
  const sizeRef = useRef<{ cols: number; rows: number }>({ cols: 80, rows: 24 });
  // Read inside the stable output handler without re-subscribing.
  const minimizedRef = useRef(false);
  const workspaceActiveRef = useRef(workspaceActive);

  const [pos, setPos] = useState<PanelLayout | undefined>(() => {
    const saved = loadPanelLayout(localStorage, layoutKey);
    return saved.left !== undefined && saved.top !== undefined ? saved : undefined;
  });
  const [size] = useState<PanelSize | undefined>(() => {
    const saved = loadPanelLayout(localStorage, layoutKey);
    return saved.width !== undefined && saved.height !== undefined
      ? { width: saved.width, height: saved.height }
      : undefined;
  });
  const [minimized, setMinimized] = useState(false);
  const [attention, setAttention] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [exited, setExited] = useState(false);
  // True while the header title is being edited inline (double-click, or the
  // header's right-click menu). The draft is held here rather than read off the
  // input on commit, so the `maxLength` cap is something the user can watch stop
  // them rather than a silent truncation applied afterwards.
  const [editing, setEditing] = useState(false);
  const [draft, setDraft] = useState("");
  /**
   * Set while Escape is abandoning an edit.
   *
   * Escape unmounts a focused input, which fires `onBlur` on the way out — so
   * without this flag the cancel path commits the very value it was cancelling,
   * and the rename cannot be backed out of at all. The same guard, for the same
   * reason, as the codebase-tab rename in `App.tsx`.
   */
  const abandoning = useRef(false);
  // Where the header's right-click menu is, or null when closed. It exists for
  // discoverability: nobody finds a double-click on a title by looking at it.
  const [menu, setMenu] = useState<{ x: number; y: number } | null>(null);

  minimizedRef.current = minimized;
  workspaceActiveRef.current = workspaceActive;

  // A workspace switch hides an otherwise-visible terminal. If its bell rang
  // while hidden, returning to the workspace acknowledges it immediately when
  // the panel itself is visible; a manually minimized terminal remains pending
  // until its pill is restored.
  useEffect(() => {
    if (workspaceActive && !minimized) setAttention(false);
  }, [workspaceActive, minimized]);

  // Report the attention flag up so this terminal's workspace tab can flash
  // even while the whole tab (and this pill) is hidden in the background.
  useEffect(() => {
    onAttentionChange?.(attention);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [attention]);
  // Closing the terminal clears its contribution to the tab's flash.
  useEffect(() => {
    return () => onAttentionChange?.(false);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  // Open the PTY once, and stream its output straight into the view. The
  // effect runs after the child TerminalView has mounted (child effects run
  // first), so `viewRef` and the initial size are already set.
  useEffect(() => {
    let alive = true;
    const { cols, rows } = sizeRef.current;
    void api
      .terminalOpen(
        cols,
        rows,
        (event: TerminalEvent) => {
          if (!alive) return;
          handleEvent(event);
        },
        cwd,
        title,
        command?.program,
        command?.args,
      )
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
        if (alive) {
          setError(String(e));
          if (minimizedRef.current || !workspaceActiveRef.current) onCompleted?.(false);
        }
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
        if (outputNeedsAttention(minimizedRef.current || !workspaceActiveRef.current, event.text)) {
          setAttention(true);
        }
        break;
      case "exited": {
        const note =
          event.code === null
            ? "\r\n\x1b[33m[terminal ended]\x1b[0m\r\n"
            : `\r\n\x1b[${event.success ? 32 : 31}m[terminal exited: ${event.code}]\x1b[0m\r\n`;
        viewRef.current?.write(note);
        setExited(true);
        // Exit does not flash *this* panel: only the bell (handled in the output
        // case) counts as the terminal asking for you, and a finished terminal
        // is not asking for anything. It is still worth saying one floor up,
        // where a whole codebase is out of sight — hence the one-shot upward
        // report, which the tab renders as a signal that expires by itself.
        if (minimizedRef.current || !workspaceActiveRef.current) onCompleted?.(event.success);
        break;
      }
      case "failed":
        viewRef.current?.write(`\r\n\x1b[31m${event.message}\x1b[0m\r\n`);
        setError(event.message);
        if (minimizedRef.current || !workspaceActiveRef.current) onCompleted?.(false);
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
        const saved = loadPanelLayout(localStorage, layoutKey);
        savePanelLayout(localStorage, { ...saved, ...clamped }, layoutKey);
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
    // Restoring is an explicit "I want this one now". The pill is a sibling of
    // the panel, so the panel's own pointer handler never sees this click.
    onRaise?.();
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

  // Begin editing the title. The draft starts as the current title, so Enter
  // with nothing typed is a no-op rather than a blank commit.
  const startRename = () => {
    if (!onRename) return;
    abandoning.current = false;
    setDraft(title);
    setEditing(true);
  };

  // Commit a rename: update the local descriptor (the host applies the cleaning
  // and the refusal, `renameTerminal`) and keep the backend running-registry
  // record in step so the Running panel shows the new title too.
  //
  // Both sides ask `acceptedTerminalTitle`, so the registry receives the title
  // that was *accepted* rather than the raw text. Gating this on `value.trim()`
  // instead let the two diverge on precisely the inputs the cleaning exists
  // for: `trim` strips neither U+0000 nor a bidi override, so a title the header
  // refused still renamed the Running panel row, and `"a	b"` left the header
  // reading `a b` while the Running panel read `a	b`.
  const commitRename = (value: string) => {
    // Escape got here first. Committing now would store the edit that was just
    // abandoned, so this path does nothing but close the editor.
    if (abandoning.current) {
      abandoning.current = false;
      setEditing(false);
      return;
    }
    onRename?.(value);
    const id = sessionRef.current;
    const clean = acceptedTerminalTitle(value);
    if (id && clean !== null) {
      void api.terminalSetLabel(id, cwd, clean).catch(() => {});
    }
    setEditing(false);
  };

  // Drag by the header. Identical to ReviewPanel: a press that never moves is a
  // click, so the minimize/close buttons still work; the clamp is pure.
  const onHeaderPointerDown = (e: React.PointerEvent<HTMLDivElement>) => {
    if (e.button !== 0) return;
    // A press on the title, on a header button, or in the rename input is not a
    // drag. The title matters beyond the drag: the handler below takes **pointer
    // capture** on the header, and while that is active the browser dispatches
    // `click`/`dblclick` to the capturing element — so the `dblclick` that starts
    // a rename never reached the `<strong>` inside at all, and the rename UI was
    // dead. Bailing out here is what makes it reachable; dragging by the rest of
    // the header is unchanged. It must also come *before* the focus below, or the
    // rename input loses focus the moment it mounts.
    if ((e.target as HTMLElement).closest("button, strong, input")) return;
    const panel = panelRef.current;
    if (!panel) return;

    // Clicking the title bar (to focus or to drag) puts the caret in the
    // terminal, so you can type straight away — the header is not xterm, so a
    // click here would otherwise leave focus wherever it was.
    //
    // Deferred by a frame, and that deferral is the whole fix: this runs during
    // `pointerdown`, and the browser's default mousedown action then moves focus
    // to the pressed element, blurring xterm's hidden textarea a moment after we
    // focused it — so the click appeared to do nothing. `restore()` above defers
    // for this same ordering reason. `preventDefault()` would also stop the
    // shift, but it risks suppressing the `dblclick` the rename depends on.
    setTimeout(() => viewRef.current?.focus(), 0);

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
      if (moved) savePanelLayout(localStorage, latest, layoutKey);
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
          // Stack pills upward, starting one slot above the base (which is
          // reserved for the global Notes bar) so they never overlap it or each
          // other. The custom colour tints the pill; while it flashes for
          // attention the flash keyframes take over the background (transient).
          style={{ bottom: pillBottom(index), ...(color && !attention ? { background: color } : {}) }}
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
        // Capture phase, on the root rather than the header, so clicking into the
        // terminal *body* raises it too — and so this runs before the header's
        // drag handler and before xterm sees the press. It deliberately neither
        // preventDefaults nor stops propagation, which is what leaves xterm's
        // text selection, the drag, and the header buttons untouched.
        onPointerDownCapture={() => onRaise?.()}
        style={{
          ...({ "--cb-stack": stackOffset } as React.CSSProperties),
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
          // Right-click is the discoverable route to the rename. The
          // double-click stays; this is what tells anyone it is there.
          onContextMenu={
            onRename && !editing
              ? (e) => {
                  e.preventDefault();
                  setMenu({ x: e.clientX, y: e.clientY });
                }
              : undefined
          }
        >
          {editing ? (
            <input
              className="terminal-title-edit"
              autoFocus
              // Controlled, with the cap enforced by the field: a box that takes
              // a pasted paragraph and silently keeps 40 characters of it reads
              // as a bug. `renameTerminal` still cleans and caps what arrives,
              // since this input is not the only way a title can be set.
              value={draft}
              maxLength={MAX_LABEL_LENGTH}
              onChange={(e) => setDraft(e.target.value)}
              onBlur={() => commitRename(draft)}
              onKeyDown={(e) => {
                if (e.key === "Enter") {
                  commitRename(draft);
                } else if (e.key === "Escape") {
                  // Flag first: unmounting the input fires `onBlur`, and without
                  // this the cancel would commit the edit it is cancelling.
                  abandoning.current = true;
                  setEditing(false);
                }
              }}
            />
          ) : (
            <strong
              onDoubleClick={startRename}
              title={onRename ? "Double-click to rename" : undefined}
              style={onRename ? { cursor: "text" } : undefined}
            >
              {title}
            </strong>
          )}
          <span className="faint" style={{ fontSize: 12 }}>
            {status}
          </span>
          <span style={{ flex: 1 }} />
          {onRecolor && (
            <PillColorMenu color={color} onPick={onRecolor} title="Set the minimized pill colour" />
          )}
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

      {/* The header's right-click menu, through the shared ContextMenu shell, so
          Escape-to-close and viewport clamping come with it rather than being a
          third hand-rolled copy. `elevated` because the default band sits *below*
          the floating panels: at 46 the menu — and its click-catching backdrop —
          would hide behind the very terminal the menu was opened from. */}
      {menu && (
        <ContextMenu x={menu.x} y={menu.y} elevated onClose={() => setMenu(null)}>
          <div
            className="dropdown-item"
            onClick={() => {
              setMenu(null);
              startRename();
            }}
          >
            Rename…
          </div>
        </ContextMenu>
      )}
    </>
  );
}

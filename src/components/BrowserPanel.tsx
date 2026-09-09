import { useCallback, useEffect, useRef, useState } from "react";
import { getCurrentWindow } from "@tauri-apps/api/window";
import * as api from "../ipc/api";
import { BrowserMcpPanel } from "./BrowserMcpPanel";
import type { BrowserSnapshot } from "../ipc/types";
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
  consentBanner,
  READ_CONSENT_ACTION,
  WRITE_CONSENT_ACTION,
  BROWSER_LAYOUT_KEY,
  hiddenPageReason,
  pageRect,
  pageVisible,
  pillLabel,
  urlBarValue,
  type PanelGeometry,
} from "./browserPanelLogic";

/**
 * The embedded browser as a floating window.
 *
 * The `SqlPanel` recipe over `reviewLayoutLogic` — clamped drag, clamped size,
 * the resize gate, load, save — under the key `cb.browser.layout`, minimizing to
 * a labelled pill rather than closing. Every decision it makes is in the tested
 * `browserPanelLogic.ts`; this file is a rendering shell plus the DOM plumbing
 * that feeds it.
 *
 * # What is genuinely different from every other panel here
 *
 * **The page is not in the DOM.** It is a WebView2 child HWND created by the
 * Rust host (`src-tauri/src/browser/mod.rs`), and it composites **above** the
 * DOM: it ignores `--z-panel`, `--z-notes` and `--z-overlay`, and `hidden` on a
 * React div does not hide it. So `.browser-page` below is a *placeholder* whose
 * only job is to be measured; the host is told its rect and paints there. That
 * also means a terminal, Notes or Search Everywhere dragged over this panel is
 * covered by the page — **accepted**, not a bug to work around here.
 *
 * Three things do hide it, and all three go through the host:
 *
 *  1. minimized → `browser_set_visible(false)`, the panel staying mounted so the
 *     page, its session and its running SPA survive (the `SqlPanel` rule),
 *  2. unmounted → `browser_close`, which **drops** the webview,
 *  3. an unusable rect → left hidden with `hiddenPageReason` shown in its place.
 *
 * # Two CLAUDE.md gotchas that apply directly, and did cost a feature once
 *
 * The URL bar is an `<input>` **inside the draggable header**. Every floating
 * panel here drags by its header via `setPointerCapture`, and while capture is
 * active the browser dispatches `click` to the *capturing element* — so a
 * control inside the header receives no clicks at all unless the press is
 * exempted from the drag. That is exactly the bug that shipped the terminal's
 * double-click-to-rename inert, fully wired, for months. Hence the
 * `closest("button, input, ...")` guard in `onHeaderPointerDown` — which
 * deliberately does *not* list `strong`, for the reason recorded there.
 *
 * And focusing the field on `pointerdown` must happen inside a
 * `setTimeout(…, 0)`: the browser's default mousedown action moves focus
 * **after** the handler runs, so a synchronous `focus()` is silently undone.
 */
export function BrowserPanel({
  restoreRequest,
  enabled,
  onClose,
}: {
  /**
   * Changes on every request to open the browser. A minimized panel restores
   * itself when it changes — a boolean could not say "open it again" about a
   * panel that is already open.
   */
  restoreRequest: number;
  /**
   * Whether the browser plugin is still on. Read **only** to attribute the
   * close: the caller unmounts this component either way, and
   * `browser_close(pluginDisabled)` is what keeps `pluginDisabled` and
   * `panelClosed` two answers rather than one. Held in a ref so the unmount
   * cleanup sees the value at unmount rather than at mount.
   */
  enabled: boolean;
  onClose: () => void;
}) {
  const panelRef = useRef<HTMLDivElement>(null);
  const pageRef = useRef<HTMLDivElement>(null);
  const urlRef = useRef<HTMLInputElement>(null);
  const [minimized, setMinimized] = useState(false);
  /**
   * Which `sync` run is the newest. See `sync` for why a stale one must not
   * write: it can re-show an OS webview over whatever the user is reading.
   */
  const syncGeneration = useRef(0);
  const [snapshot, setSnapshot] = useState<BrowserSnapshot | null>(null);
  /**
   * Whether the agent-setup modal is open.
   *
   * A transient modal, so it is mounted only while open — there is no live
   * state in it worth keeping, unlike the page itself.
   */
  const [setupOpen, setSetupOpen] = useState(false);
  const [draft, setDraft] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [hidden, setHidden] = useState<string | null>(null);

  const enabledRef = useRef(enabled);
  enabledRef.current = enabled;

  useEffect(() => setMinimized(false), [restoreRequest]);

  const [pos, setPos] = useState<PanelLayout | undefined>(() => {
    const saved = loadPanelLayout(localStorage, BROWSER_LAYOUT_KEY);
    return saved.left !== undefined && saved.top !== undefined ? saved : undefined;
  });
  const [size] = useState<PanelSize | undefined>(() => {
    const saved = loadPanelLayout(localStorage, BROWSER_LAYOUT_KEY);
    return saved.width !== undefined && saved.height !== undefined
      ? { width: saved.width, height: saved.height }
      : undefined;
  });

  /**
   * Measure the placeholder and decide, through the pure `pageRect`, whether
   * there is a rect worth handing the host.
   *
   * `scaleFactor` is read per call rather than cached: a window dragged to
   * another monitor changes it, and a cached value would place the page against
   * the old display's scaling.
   */
  const measure = useCallback(async (): Promise<
    { ok: true; rect: PanelGeometry } | { ok: false; reason: string }
  > => {
    const element = pageRef.current;
    if (!element) return { ok: false, reason: "the panel is not on screen" };
    const box = element.getBoundingClientRect();
    const scaleFactor = await getCurrentWindow().scaleFactor();
    // Viewport coordinates are relative to the main webview's client area, and
    // the host's bounds are relative to the window's — the same rectangle,
    // because the main webview fills it.
    return pageRect(
      { left: box.left, top: box.top, width: box.width, height: box.height },
      { devicePixelRatio: window.devicePixelRatio, scaleFactor },
    );
  }, []);

  /**
   * Push the current rect and visibility to the host.
   *
   * One function for both because they are one decision: the page is shown at a
   * rect, or it is hidden *because* there is no usable rect. Splitting them
   * would allow the state where it is visible at a stale position.
   */
  const sync = useCallback(async () => {
    // Every `sync` is stamped, and a stamped run that is no longer the newest
    // abandons its writes.
    //
    // Without this the page can reappear **over the user's work** after they
    // minimize it. `sync` awaits `scaleFactor()` and then two IPC calls, and it
    // captured `minimized` when it started: a run that began while the panel was
    // open computes `visible: true`, and if the user minimizes during those
    // awaits it can land *after* the minimize's own run has already set false.
    // The last write wins, and the last write is the stale one.
    //
    // The webview is an OS surface that composites above the DOM, so this is not
    // a cosmetic race — it is a window the user cannot get rid of, painted over
    // whatever they are reading. The check goes before **each** write rather
    // than once at the top, because the awaits between them are exactly where
    // the state moves.
    const stamp = ++syncGeneration.current;
    const superseded = () => stamp !== syncGeneration.current;

    const decision = await measure();
    if (superseded()) return;
    const visible = pageVisible({
      state: { open: true, restoreToken: 0 },
      enabled: true,
      minimized,
      rect: decision,
    });
    setHidden(
      hiddenPageReason({
        state: { open: true, restoreToken: 0 },
        enabled: true,
        minimized,
        rect: decision,
      }),
    );
    try {
      if (decision.ok) {
        await api.browserSetBounds(decision.rect);
        if (superseded()) return;
      }
      await api.browserSetVisible(visible);
    } catch (e) {
      if (superseded()) return;
      setError(String(e));
    }
  }, [measure, minimized]);

  // Create the page on mount, and drop it on unmount. `browser_close` really
  // does take the WebView2 process tree with it (verified in the Phase 4
  // spike), which is the point: a closed or switched-off browser must not keep a
  // process, a cookie jar or a connection alive.
  useEffect(() => {
    let live = true;
    void (async () => {
      const decision = await measure();
      if (!live) return;
      if (!decision.ok) {
        // No usable rect yet — the ordinary first frame. The ResizeObserver
        // below fires with a real one and `sync` opens nothing; so open at a
        // minimal off-screen-safe rect and let `sync` place it. Refusing to
        // create at all would mean the page never appears if the observer's
        // first measurement is also its last.
        setHidden(decision.reason);
      }
      try {
        const state = await api.browserOpen(
          decision.ok ? decision.rect : { left: 0, top: 0, width: 1, height: 1 },
          null,
        );
        if (live) setSnapshot(state);
        if (live && !decision.ok) await api.browserSetVisible(false);
      } catch (e) {
        if (live) setError(String(e));
      }
    })();
    return () => {
      live = false;
      // `enabledRef` and not `enabled`: the caller flips the feature off and
      // then unmounts, so the value at unmount is the one that says why.
      void api.browserClose(!enabledRef.current).catch(() => {
        // Nothing to report to — the panel is gone. The host has already
        // recorded the state change.
      });
    };
    // Mount/unmount only, deliberately with no dependencies: re-running this
    // effect would drop the page and create another one mid-session.
  }, []);

  // Poll the host's data half. Cheap by construction — `browser_state` never
  // touches the main thread — and necessary because there is no event channel
  // for the url, the title or the availability.
  useEffect(() => {
    let live = true;
    const read = async () => {
      try {
        const state = await api.browserState();
        if (live) setSnapshot(state);
      } catch {
        // A failed poll is not worth a message: the next one is 700ms away and
        // the panel is still showing the last known state.
      }
    };
    void read();
    const timer = setInterval(() => void read(), 700);
    return () => {
      live = false;
      clearInterval(timer);
    };
  }, []);

  // Follow the panel: its own resize, the window's, and a scale change.
  //
  // Debounced with an **immediate leading call** so the page does not lag a
  // drag by a frame — a child HWND that trails the panel it lives in reads as a
  // broken window rather than as a slow one.
  useEffect(() => {
    let timer: ReturnType<typeof setTimeout> | undefined;
    let last = 0;
    const schedule = () => {
      const now = Date.now();
      if (now - last > 200) {
        last = now;
        void sync();
        return;
      }
      if (timer) clearTimeout(timer);
      timer = setTimeout(() => {
        last = Date.now();
        void sync();
      }, 200);
    };

    schedule();
    window.addEventListener("resize", schedule);
    const observer =
      typeof ResizeObserver === "function" ? new ResizeObserver(schedule) : null;
    if (observer && pageRef.current) observer.observe(pageRef.current);
    // The window's scale factor changes on a DPI boundary and on a move between
    // monitors. `pageRect` refuses a mismatch rather than correcting it, so
    // without this listener a moved window would leave the page hidden with a
    // stale reason and no way back.
    const unlisten = getCurrentWindow().onScaleChanged(() => schedule());
    return () => {
      if (timer) clearTimeout(timer);
      window.removeEventListener("resize", schedule);
      observer?.disconnect();
      void unlisten.then((off) => off()).catch(() => {});
    };
  }, [sync]);

  // Persist the size the user drags the native grip to. The gate is what stops
  // the mount default and the 0×0 measurement a minimize produces being written
  // over a real one (see `createResizeGate`).
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
        const saved = loadPanelLayout(localStorage, BROWSER_LAYOUT_KEY);
        savePanelLayout(localStorage, { ...saved, ...clamped }, BROWSER_LAYOUT_KEY);
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
    // The drag exemption. Without `input` in this list the URL bar would receive
    // no clicks at all: the header takes pointer capture and the browser then
    // dispatches `click` to the capturing element, not to the child under the
    // cursor. `preventDefault()` is deliberately **not** used — it would also
    // suppress the focus the field needs.
    //
    // `strong` is deliberately **not** on this list, unlike `TerminalPanel`'s.
    // There the `<strong>` is the double-click-to-rename target and must take
    // its own clicks; here it is a plain label, and it is the *only* part of
    // this header that is not a control — the URL bar is `flex: 1` and eats
    // everything else. Exempting it made the panel undraggable, which is how
    // this was found: the page's child HWND never moved. `.browser-header
    // strong` in `styles.css` is padded to give that handle a real width.
    if ((e.target as HTMLElement).closest("button, input, textarea, select")) {
      return;
    }
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
      // The OS surface must move with the panel *during* the drag, not after
      // it: it is a separate window, so it would otherwise sit still while the
      // frame around it moved.
      void sync();
    };
    const onUp = () => {
      header.releasePointerCapture(e.pointerId);
      header.removeEventListener("pointermove", onMove);
      header.removeEventListener("pointerup", onUp);
      if (moved) savePanelLayout(localStorage, latest, BROWSER_LAYOUT_KEY);
      void sync();
    };
    header.addEventListener("pointermove", onMove);
    header.addEventListener("pointerup", onUp);
  };

  const submit = async () => {
    const typed = draft;
    if (typed === null) return;
    setError(null);
    try {
      const state = await api.browserNavigate(typed);
      setSnapshot(state);
      // Only now: while a draft exists the field shows it, and clearing it
      // before the navigation lands would flick the user's text away and then
      // back (see `urlBarValue`).
      setDraft(null);
    } catch (e) {
      setError(String(e));
    }
  };

  /**
   * Record a consent grant, or withdraw one.
   *
   * Separate from `act` because the answer matters: the backend refuses a grant
   * it cannot scope (no origin, a page still loading) and returns the snapshot
   * it actually holds, so the banner must re-render from that rather than from
   * what was clicked.
   */
  const grant = async (reads: boolean, writes: boolean) => {
    setError(null);
    try {
      setSnapshot(await api.browserSetAutomationConsent(reads, writes));
    } catch (e) {
      setError(String(e));
    }
  };

  const act = async (run: () => Promise<void>) => {
    setError(null);
    try {
      await run();
    } catch (e) {
      setError(String(e));
    }
  };

  const url = snapshot?.url ?? null;
  const title = snapshot?.title ?? null;
  // `null` for every state that is not a loaded page: consent is scoped to an
  // origin and the backend refuses a grant without one, so a control there
  // would be a button that always errors.
  const banner = consentBanner(snapshot);

  return (
    <>
      {minimized && (
        <button
          className="review-pill browser-pill"
          onClick={() => setMinimized(false)}
          title="Restore the browser (the page keeps running)"
        >
          <span>{pillLabel(title, url)}</span>
        </button>
      )}

      <div
        className="review-panel browser-panel"
        hidden={minimized}
        ref={panelRef}
        style={{
          ...(pos ? { left: pos.left, top: pos.top, right: "auto", bottom: "auto" } : {}),
          ...(size ? { width: size.width, height: size.height } : {}),
        }}
      >
        <div className="review-header browser-header" onPointerDown={onHeaderPointerDown}>
          <strong>Web</strong>
          <button
            onClick={() => void act(api.browserBack)}
            title="Back (the page's own history)"
            aria-label="Back"
          >
            ←
          </button>
          <button
            onClick={() => void act(api.browserForward)}
            title="Forward (the page's own history)"
            aria-label="Forward"
          >
            →
          </button>
          <button onClick={() => void act(api.browserReload)} title="Reload" aria-label="Reload">
            ⟳
          </button>
          <input
            ref={urlRef}
            className="browser-url"
            value={urlBarValue(draft, url)}
            placeholder="example.com"
            spellCheck={false}
            onChange={(e) => setDraft(e.target.value)}
            onKeyDown={(e) => {
              if (e.key === "Enter") void submit();
              if (e.key === "Escape") setDraft(null);
              // The page is a separate OS surface and the app's own shortcut
              // dispatcher is a window-level capture listener, so a chord typed
              // here would otherwise also reach it. Typing is not a command.
              e.stopPropagation();
            }}
            onPointerDown={() => {
              // Inside a timeout because the default mousedown action moves
              // focus *after* this handler runs — a synchronous `focus()` here
              // looks right and is silently undone.
              setTimeout(() => urlRef.current?.focus(), 0);
            }}
            title="Type an address and press Enter. This is not a search box: a phrase is refused rather than sent to a search engine."
          />
          <button onClick={() => void submit()} title="Go" aria-label="Go">
            Go
          </button>
          <button
            onClick={() => setSetupOpen(true)}
            title="Give a coding agent access to this panel (installs an MCP server; grants nothing on its own)"
          >
            Agents
          </button>
          <button onClick={() => setMinimized(true)} title="Minimize (the page keeps running)">
            —
          </button>
          <button onClick={onClose} title="Close (drops the page and its browser process)">
            ✕
          </button>
        </div>

        {error && <div className="browser-error">{error}</div>}

        {/* The automation consent banner. This is the control every refusal an
            agent reads points at, by name — `cb_core::browser::consent` quotes
            these two labels verbatim inside its refusal sentences — so the
            buttons are labelled from the shared constants rather than from
            literals typed here. It is present whenever there is a page rather
            than only when something has asked, because a request can arrive
            while the user is looking at another window and the refusal tells
            them to come here and click. */}
        {banner && (
          <div className={`browser-consent browser-consent-${banner.kind}`}>
            <span className="browser-consent-message">{banner.message}</span>
            {/* The controls are their own row rather than wrapping after the
                message. A wrapped row landed in the strip the page's OS surface
                paints over, which left these invisible and still clickable -
                see the comment on `.browser-consent` in `styles.css`. */}
            <div className="browser-consent-actions">
              {banner.offerRead && (
                <button
                  onClick={() => void grant(true, false)}
                  title="Reading only: the address, the page text, its elements, its console and the requests it made. Applies to this page only."
                >
                  {READ_CONSENT_ACTION}
                </button>
              )}
              {banner.offerWrite && (
                <button
                  onClick={() => void grant(true, true)}
                  title="Reading, plus navigating, clicking and typing in this page. Applies to this page only."
                >
                  {WRITE_CONSENT_ACTION}
                </button>
              )}
              {banner.offerRevoke && (
                <button
                  onClick={() => void grant(false, false)}
                  title="Withdraw agent access to this page"
                >
                  Withdraw
                </button>
              )}
            </div>
          </div>
        )}

        {/* The placeholder the host paints over. Nothing is rendered inside it
            except the reason the page is *not* there — a blank rectangle with no
            explanation is the failure this app refuses everywhere else. */}
        <div className="browser-page" ref={pageRef}>
          {hidden && <div className="browser-page-note">{hidden}</div>}
        </div>
      </div>

      {setupOpen && <BrowserMcpPanel onClose={() => setSetupOpen(false)} />}
    </>
  );
}

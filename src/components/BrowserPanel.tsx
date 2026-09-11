import { useCallback, useEffect, useRef, useState } from "react";
import { createPortal } from "react-dom";
import { getCurrentWindow } from "@tauri-apps/api/window";
import * as api from "../ipc/api";
import { BrowserMcpPanel } from "./BrowserMcpPanel";
import { slotKey, useRegions } from "./RegionContext";
import type { Dockable } from "./regionLayoutLogic";
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
  browserLayoutKey,
  clampBrowserTop,
  hiddenPageReason,
  occludedByAbovePanels,
  occludedByPanels,
  pageRect,
  pageVisible,
  pillLabel,
  urlBarValue,
  type PanelGeometry,
} from "./browserPanelLogic";
import { resizeFromHandle, type ResizeEdge } from "./reviewLayoutLogic";
import { useDockEntry } from "./DockContext";
import { dockId } from "./dockLogic";
import { useOcclusionCount } from "./occlusionContext";
import { useFocusEntry, useFocusOffset, useFocusOrder } from "./focusOrderContext";

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
 * only job is to be measured; the host is told its rect and paints there.
 *
 * Because it composites above the DOM, anything DOM that must appear over it is
 * made visible by *hiding the page* — the six cases in `pageVisible`, all routed
 * through the host: minimized, feature-off (→ `browser_close`, dropping the
 * webview), an unusable rect (left hidden with `hiddenPageReason` in its place),
 * a backgrounded codebase, the panel's own setup modal (bug 2), and an occluding
 * surface over its rect (bug 3) — an open menu/modal (counted via
 * `occlusionContext`) or a peer floating panel that is stacked *above* this one in
 * the shared focus order and overlaps the page (`occludedByAbovePanels`, measured
 * in `sync`). A peer the browser was raised over no longer hides the page.
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
  root,
  focusId,
  active,
  restoreRequest,
  enabled,
  onClose,
}: {
  /**
   * The codebase this page belongs to. Every host call is scoped by it, so a
   * page never reaches into another codebase's webview, and the layout is
   * remembered per codebase (`browserLayoutKey`).
   */
  root: string;
  /**
   * This panel's id in the app-wide focus order (`focusOrderContext`), namespaced
   * by codebase. Clicking the panel raises it over terminals and Notes; the same
   * order drives both its chrome z-index and which peers may occlude its page.
   */
  focusId: string;
  /**
   * Whether this codebase is the foreground tab. Drives the OS webview's
   * visibility through `pageVisible`/`sync`: a backgrounded codebase's page is
   * hidden, so it cannot composite over the codebase the user switched to.
   */
  active: boolean;
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

  // Docking. Unlike the DOM panels the page is an OS surface that lives in the
  // Rust host keyed by root, so docking never moves the page itself — it moves
  // only the stateless chrome (header, URL bar, consent, the `.browser-page`
  // placeholder) into the region slot, and the host repaints the page at the
  // placeholder's new rect via the ordinary `sync`. The page-lifecycle effects
  // stay at this component's top level, so docking never remounts them.
  const regions = useRegions();
  const dockable: Dockable = { kind: "browser", id: "browser" };
  const regionKey = slotKey(dockable);
  const wantsDock = regions?.dockedPanels.has("browser") ?? false;
  const slotNode = wantsDock ? (regions?.slot(regionKey) ?? null) : null;
  const isDocked = wantsDock && slotNode !== null;

  useEffect(() => setMinimized(false), [restoreRequest]);

  // While docked, the region tab strip is the panel's header.
  useEffect(() => {
    if (!regions || !isDocked) return;
    regions.registerMeta(regionKey, { label: "Web", onClose });
    return () => regions.registerMeta(regionKey, null);
  }, [regions, isDocked, regionKey, onClose]);

  const [pos, setPos] = useState<PanelLayout | undefined>(() => {
    const saved = loadPanelLayout(localStorage, browserLayoutKey(root));
    return saved.left !== undefined && saved.top !== undefined ? saved : undefined;
  });
  const [size, setSize] = useState<PanelSize | undefined>(() => {
    const saved = loadPanelLayout(localStorage, browserLayoutKey(root));
    return saved.width !== undefined && saved.height !== undefined
      ? { width: saved.width, height: saved.height }
      : undefined;
  });

  // How many DOM overlays (menus, modals, Search Everywhere) are open. Each would
  // otherwise be painted over by the page (bug 3); the page hides while any is up.
  // Peer floating panels are handled geometrically in `sync`, not counted here.
  const occludedByOverlay = useOcclusionCount() > 0;

  // This panel's place in the app-wide focus order. `useFocusEntry` raises it on
  // mount and releases it on unmount; `raiseBrowser` (on pointerdown) and the
  // restore effect below bring it forward on click/restore. `myOffset` is both its
  // chrome `--cb-stack` and the threshold peers must beat to occlude its page.
  const raiseBrowser = useFocusEntry(focusId);
  const myOffset = useFocusOffset(focusId);
  const { order: focusOrder } = useFocusOrder();
  // Restoring a minimized panel is a click's worth of intent — bring it forward.
  useEffect(() => {
    if (!minimized) raiseBrowser();
  }, [minimized, raiseBrowser]);

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
    // Never let the page rect rise above the app's own title/tab chrome: a panel
    // dragged to the very top would otherwise push the page under the titlebar,
    // where a titlebar menu drops *into* it (bug 3). The height shrinks so the
    // bottom edge stays put.
    const clamped = clampBrowserTop(
      { left: box.left, top: box.top, width: box.width, height: box.height },
      measureChromeBottom(),
    );
    // Viewport coordinates are relative to the main webview's client area, and
    // the host's bounds are relative to the window's — the same rectangle,
    // because the main webview fills it.
    return pageRect(clamped, { devicePixelRatio: window.devicePixelRatio, scaleFactor });
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
    // Occluded if an overlay/menu/modal is open (counted, always on top) or a peer
    // floating panel that is stacked *above* this one in the focus order overlaps
    // the page's rect. Raise-aware: a terminal/Notes the browser was raised over no
    // longer blanks the page — clicking the browser really does bring it forward.
    // Docked: the page sits at the workspace layer, below every floating panel,
    // so *any* overlapping floating peer occludes it (raise-unaware). Floating:
    // only a peer stacked above it in the focus order does (raise-aware), which
    // is what lets a click bring it forward.
    const peers = peerPanelRects(panelRef.current);
    const occluded =
      occludedByOverlay ||
      (decision.ok &&
        (isDocked
          ? occludedByPanels(decision.rect, peers.map((p) => p.rect))
          : occludedByAbovePanels(decision.rect, myOffset, peers)));
    const input = {
      state: { open: true, restoreToken: 0 },
      enabled: true,
      // A docked panel is never minimized — the region tab strip replaces the
      // minimize control — so a non-active region tab hides the page through its
      // 0×0 slot rect (a refused rect) rather than through `minimized`.
      minimized: isDocked ? false : minimized,
      active,
      setupOpen,
      occluded,
      rect: decision,
    };
    const visible = pageVisible(input);
    setHidden(hiddenPageReason(input));
    try {
      if (decision.ok) {
        await api.browserSetBounds(root, decision.rect);
        if (superseded()) return;
      }
      await api.browserSetVisible(root, visible);
    } catch (e) {
      if (superseded()) return;
      setError(String(e));
    }
  }, [measure, minimized, active, root, setupOpen, occludedByOverlay, myOffset, isDocked]);

  // Re-place the page the instant it docks or undocks, or when the region slot
  // node changes. `sync` is generation-stamped, so a fast dock→undock abandons
  // the superseded write rather than leaving the page at a stale rect.
  useEffect(() => {
    void sync();
  }, [isDocked, slotNode, sync]);

  // Re-run `sync` when this codebase moves between foreground and background.
  // A switch-away must call `set_visible(false)` deterministically, and a
  // switch-back must place and show the page again. `sync`'s generation stamp
  // abandons any superseded write, which is what stops a stale run re-showing
  // the page over the codebase the user switched to.
  useEffect(() => {
    void sync();
  }, [active, sync]);

  // Re-check occlusion whenever the shared focus order changes. A peer raised
  // *above* this panel must blank the page, and a peer this panel was raised over
  // must reveal it — but a raise that leaves this panel's own offset unchanged (it
  // was already at the band floor and a terminal appeared above it) would not move
  // `sync`'s deps, so trigger explicitly. The generation stamp abandons stale runs.
  useEffect(() => {
    void sync();
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [focusOrder]);

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
          root,
          decision.ok ? decision.rect : { left: 0, top: 0, width: 1, height: 1 },
          null,
        );
        if (live) setSnapshot(state);
        if (live && !decision.ok) await api.browserSetVisible(root, false);
      } catch (e) {
        if (live) setError(String(e));
      }
    })();
    return () => {
      live = false;
      // `enabledRef` and not `enabled`: the caller flips the feature off and
      // then unmounts, so the value at unmount is the one that says why.
      void api.browserClose(root, !enabledRef.current).catch(() => {
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
        const state = await api.browserState(root);
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
    // A peer floating panel dragged over the page changes no React state here, so
    // re-check occlusion when any drag ends. Cheap: `schedule` is debounced and a
    // superseded `sync` abandons its writes.
    window.addEventListener("pointerup", schedule);
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
      window.removeEventListener("pointerup", schedule);
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
        const saved = loadPanelLayout(localStorage, browserLayoutKey(root));
        savePanelLayout(localStorage, { ...saved, ...clamped }, browserLayoutKey(root));
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
      if (moved) savePanelLayout(localStorage, latest, browserLayoutKey(root));
      void sync();
    };
    header.addEventListener("pointermove", onMove);
    header.addEventListener("pointerup", onUp);
  };

  // Resize by an explicit handle rather than the native `resize: both` grip: that
  // grip sits in the bottom-right corner *inside* `.browser-page`, which the OS
  // webview composites over and so swallows the press — the page could not be
  // resized at all (bug 1). The handles live in a gutter the webview's rect never
  // covers. Same pointer plumbing as the header drag: capture, `sync` on every
  // move so the page tracks the frame live, persist on release. The arithmetic is
  // the pure, tested `resizeFromHandle`.
  const onResizePointerDown = (edge: ResizeEdge) => (e: React.PointerEvent<HTMLDivElement>) => {
    if (e.button !== 0) return;
    e.preventDefault();
    e.stopPropagation();
    const panel = panelRef.current;
    if (!panel) return;
    const start = { width: panel.offsetWidth, height: panel.offsetHeight };
    const originX = e.clientX;
    const originY = e.clientY;
    const handle = e.currentTarget;
    handle.setPointerCapture(e.pointerId);

    let latest: PanelSize = start;
    const onMove = (ev: PointerEvent) => {
      const viewport = { width: window.innerWidth, height: window.innerHeight };
      latest = resizeFromHandle(
        edge,
        start,
        { dx: ev.clientX - originX, dy: ev.clientY - originY },
        viewport,
      );
      setSize(latest);
      void sync();
    };
    const onUp = () => {
      handle.releasePointerCapture(e.pointerId);
      handle.removeEventListener("pointermove", onMove);
      handle.removeEventListener("pointerup", onUp);
      const saved = loadPanelLayout(localStorage, browserLayoutKey(root));
      savePanelLayout(localStorage, { ...saved, ...latest }, browserLayoutKey(root));
      void sync();
    };
    handle.addEventListener("pointermove", onMove);
    handle.addEventListener("pointerup", onUp);
  };

  const submit = async () => {
    const typed = draft;
    if (typed === null) return;
    setError(null);
    try {
      const state = await api.browserNavigate(root, typed);
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
      setSnapshot(await api.browserSetAutomationConsent(root, reads, writes));
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

  const restore = useCallback(() => setMinimized(false), []);
  useDockEntry(
    // A docked browser has no minimize pill — the region tab strip replaces it.
    minimized && !isDocked
      ? {
          id: dockId(root, "browser"),
          scope: root,
          label: pillLabel(title, url),
          order: 0,
          onRestore: restore,
        }
      : null,
  );

  const shell = (
      <div
        className={`review-panel browser-panel${isDocked ? " browser-docked" : ""}`}
        hidden={minimized && !isDocked}
        ref={panelRef}
        // Capture phase on the root so clicking anywhere in the panel raises it,
        // before the header drag and without preventing the default (drag/URL-bar
        // focus intact). This is the click that finally brings the page forward.
        onPointerDownCapture={raiseBrowser}
        style={
          isDocked
            ? undefined
            : {
                ...({ "--cb-stack": myOffset } as React.CSSProperties),
                ...(pos ? { left: pos.left, top: pos.top, right: "auto", bottom: "auto" } : {}),
                ...(size ? { width: size.width, height: size.height } : {}),
              }
        }
      >
        <div className="review-header browser-header" onPointerDown={onHeaderPointerDown}>
          <strong>Web</strong>
          <button
            onClick={() => void act(() => api.browserBack(root))}
            title="Back (the page's own history)"
            aria-label="Back"
          >
            ←
          </button>
          <button
            onClick={() => void act(() => api.browserForward(root))}
            title="Forward (the page's own history)"
            aria-label="Forward"
          >
            →
          </button>
          <button
            onClick={() => void act(() => api.browserReload(root))}
            title="Reload"
            aria-label="Reload"
          >
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
          {regions && !isDocked && (
            <button onClick={() => regions.dock(dockable, "right")} title="Dock to the side">
              ⊟
            </button>
          )}
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

        {/* Explicit resize handles in the gutter around `.browser-page`, since the
            OS webview covers the native corner grip. E/S/SE only. Docked, the
            region splitter resizes instead, so these are dropped. */}
        {!isDocked && (
          <>
            <div
              className="browser-resize browser-resize-e"
              onPointerDown={onResizePointerDown("e")}
              aria-hidden
            />
            <div
              className="browser-resize browser-resize-s"
              onPointerDown={onResizePointerDown("s")}
              aria-hidden
            />
            <div
              className="browser-resize browser-resize-se"
              onPointerDown={onResizePointerDown("se")}
              aria-hidden
            />
          </>
        )}
      </div>
  );

  return (
    <>
      {/* Docked, the stateless shell is portaled into the region slot; the OS
          page follows the placeholder's rect through `sync`. Floating, it renders
          in place. The page-lifecycle effects live above and never remount. */}
      {isDocked && slotNode ? createPortal(shell, slotNode) : shell}
      {setupOpen && <BrowserMcpPanel onClose={() => setSetupOpen(false)} />}
    </>
  );
}

// --- Occlusion measurement (impure; the decisions are in browserPanelLogic) ---

/**
 * The bottom edge (in CSS px) of the app's own title/tab chrome, so the page rect
 * can be kept below it. Prefers the tab strip, falls back to the titlebar, and to
 * 0 when neither is found (no clamp) — a missing bar must not push the page down.
 */
function measureChromeBottom(): number {
  const tabs = document.querySelector(".ws-tabs") ?? document.querySelector(".titlebar");
  return tabs ? tabs.getBoundingClientRect().bottom : 0;
}

/**
 * The on-screen rects of the floating panels that could cover the page, each with
 * its focus-order offset, excluding the browser panel itself (`self`). Every
 * floating panel shares the `.review-panel` base and writes its focus ordinal into
 * the `--cb-stack` CSS variable (the dock is `.dock`, which sits in its own higher
 * band and carries no `--cb-stack`, so it reads 0 — harmless, since a peer must be
 * strictly *above* the browser to occlude and the dock never overlaps the page).
 * A hidden (minimized) panel measures 0×0 and so overlaps nothing. Returned to
 * `occludedByAbovePanels`, which decides — this only gathers.
 */
function peerPanelRects(self: HTMLElement | null): { rect: PanelGeometry; offset: number }[] {
  const nodes = document.querySelectorAll<HTMLElement>(".review-panel, .dock");
  const peers: { rect: PanelGeometry; offset: number }[] = [];
  nodes.forEach((node) => {
    if (node === self) return;
    const box = node.getBoundingClientRect();
    if (box.width <= 0 || box.height <= 0) return;
    const offset = Number.parseInt(node.style.getPropertyValue("--cb-stack"), 10) || 0;
    peers.push({
      rect: { left: box.left, top: box.top, width: box.width, height: box.height },
      offset,
    });
  });
  return peers;
}

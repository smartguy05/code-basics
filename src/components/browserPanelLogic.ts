// Pure decisions for the floating browser panel: when it is mounted, when a
// fresh open should restore a minimized one, where its layout is remembered,
// and — the part with no precedent in this app — when the OS webview may be
// shown and where it is placed.
//
// Extracted so they are testable in the node environment. `BrowserPanel.tsx` is
// a rendering shell that decides nothing, and the Rust host applies what these
// return.
//
// ## An OS webview is not a DOM layer
//
// The page is a WebView2 child HWND, so it composites **above** the DOM. It
// ignores `--z-panel`, `--z-notes` and `--z-overlay` in `styles.css` entirely,
// and `hidden` on a React div does not hide it — confirmed by image in the
// Phase 4 spike, where both Notes and Search Everywhere were clipped by the
// page. So the only way to make DOM chrome appear *over* the page is to **hide
// the page**, which is what `pageVisible` decides.
//
// It is false for six things, and each is a real "there is nowhere, or nothing,
// to show right now" rather than a z-order tweak:
//
//   1. the panel is minimized,
//   2. the plugin was switched off (and the host then *drops* the webview, so a
//      disabled browser keeps no WebView2 process, no cookie jar and no
//      connection),
//   3. the measured rect is unusable — the `createResizeGate` 0×0 lesson,
//      extended to the DPI case below,
//   4. the codebase this page belongs to is **backgrounded** — bugs 6+7. The
//      browser is per-codebase now; only the active codebase's page may show, or
//      a background one paints over the foreground codebase the user switched to.
//   5. the panel's own agent-setup modal is open (bug 2) — it is DOM, so the page
//      would otherwise open in front of it,
//   6. an on-screen surface actually covers the page's rect (bug 3): an app-level
//      menu/modal that overlaps it, or a peer floating panel dragged over it. The
//      caller scopes this to a real overlap, so a panel *beside* the page does
//      not blank it — `active` is which codebase is foreground, `occluded` is
//      what is on top within it.

// --- Where the layout lives -------------------------------------------------

// Type-only, so it is erased at compile time and this module still imports
// nothing at runtime — which is what keeps it runnable under vitest's node
// environment with no DOM.
import type { BrowserAgentRequest, BrowserSnapshot } from "../ipc/types";

/**
 * The localStorage key one codebase's browser panel persists its position and
 * size under.
 *
 * Follows the `cb.<thing>.layout` convention shared with the agent panel
 * (`cb.agentPanel.layout`), Notes (`cb.notes.layout`), SQL (`cb.sql.layout`),
 * Running (`cb.running.layout`) and the launcher (`cb.launcher.layout`).
 *
 * **Scoped per codebase**, like the terminals' `cb.terminal.layout:<root>` and
 * unlike SQL's: the browser is per-codebase now (each open codebase keeps its
 * own live page), so each codebase remembers its own geometry — a fresh browser
 * in one codebase must not adopt another's position.
 */
export function browserLayoutKey(root: string): string {
  return `cb.browser.layout:${root}`;
}

// --- Open / restore ---------------------------------------------------------

/**
 * Whether the browser panel is open, and a token that changes on each *request*
 * to open it.
 *
 * The token exists because "open the browser" has two meanings once the panel
 * can be minimized: mount it, or bring the already-mounted one back. The
 * boolean cannot express the second — re-opening an open panel changes no field
 * a child could compare — so the token carries it, the same request-and-consume
 * shape `App` uses for `openRequest`/`selectRequest` and `sqlPanelLogic` uses
 * for the SQL console.
 */
export interface BrowserPanelState {
  open: boolean;
  restoreToken: number;
}

/** The initial state: no panel, no request yet. */
export const CLOSED_BROWSER_PANEL: BrowserPanelState = {
  open: false,
  restoreToken: 0,
};

/**
 * Ask for the browser panel. Always advances the token, so a request that finds
 * the panel already open still reaches it as a restore.
 */
export function openBrowserPanel(state: BrowserPanelState): BrowserPanelState {
  return { open: true, restoreToken: state.restoreToken + 1 };
}

/**
 * Close the panel (the ✕ on its header), unmounting it.
 *
 * The token is *kept*, not reset: it counts requests, and a later open must
 * still differ from every value the panel has already seen. Returns the same
 * object when there was nothing open, so a stray close costs no render.
 */
export function closeBrowserPanel(state: BrowserPanelState): BrowserPanelState {
  if (!state.open) return state;
  return { open: false, restoreToken: state.restoreToken };
}

/**
 * The panel state after the optional-feature gate has answered.
 *
 * Switching the browser plugin **off** must unmount it, not merely hide it —
 * the `sqlPanelAfterFeatureChange` rule, with more at stake. Left mounted, the
 * page would keep a WebView2 process alive, keep its cookie jar, keep polling
 * whatever endpoint it polls, and keep painting over the DOM, with no route to
 * a URL bar and no route to Stop. The user switched the browser off and the
 * browser kept browsing.
 *
 * Returns the same object whenever nothing changes (enabled, or already
 * closed), so the effect that applies it cannot loop.
 */
export function browserPanelAfterFeatureChange(
  state: BrowserPanelState,
  enabled: boolean,
): BrowserPanelState {
  if (enabled) return state;
  return closeBrowserPanel(state);
}

/**
 * Whether the browser panel should be rendered at all.
 *
 * Two independent conditions and not one: the user has asked for it, *and* the
 * feature is on. Asked separately so the mount gate and the opener's gate can
 * never disagree about a feature that was switched off while the panel was up.
 */
export function browserPanelMounted(
  state: BrowserPanelState,
  enabled: boolean,
): boolean {
  return state.open && enabled;
}

// --- Where the page is painted ---------------------------------------------

/** A rect in CSS pixels, relative to the window's client area. */
export interface PanelGeometry {
  left: number;
  top: number;
  width: number;
  height: number;
}

/**
 * The two scale readings that must agree before a rect can be trusted.
 *
 * `devicePixelRatio` is the browser's, and includes the main webview's own zoom;
 * `scaleFactor` is the one the window runtime reports. wry's bounds are
 * **logical** pixels, and CSS pixels equal logical pixels only while the two
 * match. The spike measured this at scale factor 1.0 only, so the scaled case
 * is genuinely untested — which is why a disagreement refuses rather than
 * applying a correction factor nobody has verified.
 */
export interface ScaleReading {
  devicePixelRatio: number;
  scaleFactor: number;
}

/**
 * Either a rect to hand the host, or the reason there is none.
 *
 * A discriminated result rather than `null`, so the panel can *say* why the
 * page is not painted. A blank panel with no explanation is the failure mode
 * this app refuses everywhere else.
 */
export type PageRectDecision =
  | { ok: true; rect: PanelGeometry }
  | { ok: false; reason: string };

/** How far the two scale readings may differ before the rect is refused. */
const SCALE_TOLERANCE = 0.01;

/**
 * Validate a measured panel rect into bounds for the OS webview.
 *
 * Refuses, rather than guessing, in three cases:
 *
 * - **Anything non-finite.** A `NaN` reaches `set_bounds` as an unpredictable
 *   number, and an OS surface placed at an unpredictable position is worse than
 *   one that is not placed.
 * - **A degenerate size.** The `createResizeGate` lesson: a `ResizeObserver`
 *   reports 0×0 while an element is hidden, and it fires once on `observe()`
 *   before layout. A 0×0 child webview is not harmless — it is a real surface
 *   with no size, and on the first frame it is also the *usual* measurement.
 * - **The two scale readings disagree.** Then CSS pixels are not logical
 *   pixels, and the rect would place the page somewhere other than where the
 *   panel is. Untested territory per the spike, so it is refused with both
 *   numbers named rather than corrected by a guessed factor.
 *
 * Values are rounded to whole pixels: the bounds cross to an OS surface that
 * cannot occupy a fraction of one, so the rounding happens here where it is
 * visible and tested rather than inside the platform.
 */
export function pageRect(
  measured: PanelGeometry,
  scale: ScaleReading,
): PageRectDecision {
  const values = [
    measured.left,
    measured.top,
    measured.width,
    measured.height,
    scale.devicePixelRatio,
    scale.scaleFactor,
  ];
  if (values.some((value) => !Number.isFinite(value))) {
    return {
      ok: false,
      reason: "the panel's position could not be measured",
    };
  }
  if (measured.width <= 0 || measured.height <= 0) {
    return {
      ok: false,
      reason: "the panel has no size on screen yet",
    };
  }
  if (Math.abs(scale.devicePixelRatio - scale.scaleFactor) > SCALE_TOLERANCE) {
    return {
      ok: false,
      reason:
        `the page is not shown because this window's zoom or display scaling ` +
        `(browser ${scale.devicePixelRatio}, window ${scale.scaleFactor}) would ` +
        `place it away from the panel`,
    };
  }
  return {
    ok: true,
    rect: {
      left: Math.round(measured.left),
      top: Math.round(measured.top),
      width: Math.round(measured.width),
      height: Math.round(measured.height),
    },
  };
}

/** Everything `pageVisible` needs, so no caller has to remember the order. */
export interface PageVisibilityInput {
  state: BrowserPanelState;
  enabled: boolean;
  minimized: boolean;
  /**
   * Whether the codebase this page belongs to is the foreground tab. The
   * stacking fix (bugs 6+7): only the active codebase's page may show, or a
   * background one composites over the foreground codebase.
   */
  active: boolean;
  /**
   * Whether the panel's own agent-setup modal (`BrowserMcpPanel`) is open. That
   * modal is DOM and the OS webview composites above it, so the page must hide
   * while it is up or the modal opens *behind* the page (bug 2). A deliberate
   * user action with its own surface on screen — like `minimized`, not a reason
   * to explain.
   */
  setupOpen: boolean;
  /**
   * Whether some other on-screen surface actually covers the page's rect: an
   * app-level menu/modal that overlaps it, or a peer floating panel dragged over
   * it (bug 3). The page hides so that DOM chrome the user summoned is not
   * painted over by the OS webview. Overlap-scoped by the caller (only a surface
   * that really overlaps sets this), so a side-by-side panel does not blank the
   * page.
   */
  occluded: boolean;
  rect: PageRectDecision;
}

/**
 * Whether the OS webview should be visible right now.
 *
 * The page is a WebView2 child HWND that composites above the DOM and ignores
 * every z-band, so anything DOM that must appear over it is made visible by
 * *hiding the page*. It is hidden for six things, each a real "there is nowhere,
 * or nothing, to show right now" rather than a z-order tweak: not mounted,
 * minimized, backgrounded codebase, an unusable rect, the panel's own setup modal
 * (bug 2), and an occluding surface over its rect (bug 3). `active` is which
 * *codebase* is foreground; `occluded` is what is on top *within* it.
 */
export function pageVisible(input: PageVisibilityInput): boolean {
  const { state, enabled, minimized, active, setupOpen, occluded, rect } = input;
  return (
    browserPanelMounted(state, enabled) &&
    !minimized &&
    active &&
    !setupOpen &&
    !occluded &&
    rect.ok
  );
}

/**
 * Why the page is not on screen even though the panel is, or `null` when it is.
 *
 * Not-mounted, minimized, backgrounded, the setup modal **and** an occluding
 * surface are deliberately not reasons: each is something the user did or
 * summoned, and the covering surface is itself the explanation — a scary note
 * under it would flash on every menu open. What needs explaining is the case
 * where the panel is the foreground codebase's, open, expanded, unobscured, and
 * the page still is not there (a refused rect).
 */
export function hiddenPageReason(input: PageVisibilityInput): string | null {
  const { state, enabled, minimized, active, setupOpen, occluded, rect } = input;
  if (!browserPanelMounted(state, enabled) || minimized || !active || setupOpen || occluded) {
    return null;
  }
  return rect.ok ? null : rect.reason;
}

/**
 * Whether two rects overlap. Edge-touching is **not** an overlap: a panel whose
 * left edge sits exactly on the page's right edge covers none of it, and treating
 * that as occlusion would blank the page for a panel flush beside it.
 */
export function rectsOverlap(a: PanelGeometry, b: PanelGeometry): boolean {
  return (
    a.left < b.left + b.width &&
    b.left < a.left + a.width &&
    a.top < b.top + b.height &&
    b.top < a.top + a.height
  );
}

/**
 * Whether any of `others` overlaps the page rect. The caller collects the peer
 * floating panels' rects (a terminal, Notes, a review panel dragged over the
 * page); an empty list is not occluded.
 *
 * Kept for reference and simpler callers; the browser now uses the raise-aware
 * {@link occludedByAbovePanels} so a panel it was raised over does not blank it.
 */
export function occludedByPanels(page: PanelGeometry, others: PanelGeometry[]): boolean {
  return others.some((other) => rectsOverlap(page, other));
}

/**
 * Whether any peer that is stacked **strictly above** the page overlaps it.
 *
 * This is what makes "click the browser to bring it forward" real. The page is a
 * WebView2 surface hidden only by occlusion; before this, *any* overlapping peer
 * blanked it, so no click could win. Now a peer occludes only when its focus-order
 * offset is greater than the browser's own — a terminal or Notes the browser was
 * raised over sits below it and leaves the page visible, while one clicked *after*
 * the browser rises above it and blanks the page again. Equal offset is not
 * "above" (a tie cannot claim the front), so it does not occlude.
 */
export function occludedByAbovePanels(
  page: PanelGeometry,
  pageOffset: number,
  peers: { rect: PanelGeometry; offset: number }[],
): boolean {
  return peers.some((p) => p.offset > pageOffset && rectsOverlap(page, p.rect));
}

/**
 * Clamp a measured page rect so its top never rises above `chromeBottom` — the
 * bottom edge of the app's own title/tab chrome. The page is below the panel
 * header already, but a panel dragged to the top of the window can push that
 * region up under the titlebar, where a titlebar menu then drops *into* the page.
 * The height shrinks by whatever the top moved, so the bottom edge stays put.
 * A rect already below the chrome is returned unchanged.
 */
export function clampBrowserTop(rect: PanelGeometry, chromeBottom: number): PanelGeometry {
  if (rect.top >= chromeBottom) return rect;
  const delta = chromeBottom - rect.top;
  return {
    left: rect.left,
    top: chromeBottom,
    width: rect.width,
    height: Math.max(0, rect.height - delta),
  };
}

// --- The URL bar ------------------------------------------------------------

/**
 * What the URL bar input should display.
 *
 * `draft` is what the user has typed and not yet submitted, `null` when they
 * are not editing. A draft always wins, including an **empty** one: a user who
 * has selected all and deleted is mid-edit, and re-filling the field from the
 * page under their caret is the bug every address bar that syncs too eagerly
 * has.
 *
 * `about:blank` shows as empty rather than as itself. It is this panel's own
 * resting state, not an address the user typed, and showing it means the first
 * thing they must do is clear it.
 */
export function urlBarValue(
  draft: string | null,
  pageUrl: string | null,
): string {
  if (draft !== null) return draft;
  if (pageUrl === null) return "";
  const trimmed = pageUrl.trim();
  return trimmed.toLowerCase() === "about:blank" ? "" : trimmed;
}

/** How long a pill label may be before it is cut. */
const PILL_LABEL_LIMIT = 40;

/**
 * The label on the minimized panel's bar.
 *
 * Prefers the page's own title, because that is what the user recognises. Falls
 * back to the host — **not** the full URL, which on a real application is a
 * paragraph of path and query, may carry a token, and would be unreadable at
 * pill width anyway. Falls back finally to a fixed word, never to an empty bar
 * the user cannot identify.
 */
export function pillLabel(
  title: string | null,
  pageUrl: string | null,
): string {
  const trimmedTitle = (title ?? "").trim();
  if (trimmedTitle) return cut(trimmedTitle);
  const host = hostOf(pageUrl);
  if (host) return cut(host);
  return "Browser";
}

function cut(text: string): string {
  return text.length <= PILL_LABEL_LIMIT
    ? text
    : `${text.slice(0, PILL_LABEL_LIMIT - 1)}…`;
}

/**
 * The host of a URL, or `""` when there is not one.
 *
 * Hand-parsed rather than via `URL`, and the reason is worth keeping: this
 * module is tested in the node environment, and while `URL` exists there it
 * *throws* on the malformed input this function is most likely to be handed —
 * a half-typed address, or an empty string. Returning `""` for anything
 * unparseable keeps the caller's fallback chain intact.
 */
function hostOf(pageUrl: string | null): string {
  const trimmed = (pageUrl ?? "").trim();
  const at = trimmed.indexOf("://");
  if (at <= 0) return "";
  const authority = trimmed.slice(at + 3).split(/[/?#]/)[0] ?? "";
  const host = authority.includes("@")
    ? authority.slice(authority.lastIndexOf("@") + 1)
    : authority;
  return host;
}

// --- The automation consent banner -----------------------------------------
//
// The panel is the only place agent access to a page can be granted, so this
// is the control every refusal an agent reads points at. Three rules shape it:
//
// 1. **The labels are quoted, not paraphrased.** `cb_core::browser::consent`
//    builds every refusal sentence around these exact words ("Click
//    \"Allow agents to read this page\" in the browser panel"), so a reworded
//    button sends the user hunting for a control that does not exist. The two
//    constants below are those two Rust constants, and
//    `the_typescript_banner_quotes_the_consent_labels` in
//    `crates/core/src/browser/consent_tests.rs` reads this file and fails if
//    they drift.
// 2. **The asker is named.** A user granting access to their own logged-in
//    session is entitled to know which program gets it. The name comes from the
//    OS (`GetNamedPipeClientProcessId`, then the process image), never from
//    anything the caller sent.
// 3. **The control is present before anything asks.** The refusal text tells
//    the user to click it, and an agent's request may arrive while they are
//    looking at another window — so the banner is not a transient prompt.

/** `cb_core::browser::consent::READ_CONSENT_ACTION`, verbatim. */
export const READ_CONSENT_ACTION = "Allow agents to read this page";

/** `cb_core::browser::consent::WRITE_CONSENT_ACTION`, verbatim. */
export const WRITE_CONSENT_ACTION = "Allow agents to read and control this page";

export interface ConsentBanner {
  /**
   * `asked` — something was refused and is waiting on the user; `active` —
   * a grant is in force and a program has used it; `granted` — in force,
   * unused so far; `idle` — nothing granted and nothing has asked.
   *
   * Four rather than two because they are four different things to say, and
   * because `asked` is the only one that should draw the eye.
   */
  kind: "asked" | "active" | "granted" | "idle";
  message: string;
  /** Offer the read grant. */
  offerRead: boolean;
  /** Offer the stronger grant. Shown alongside `offerRead` when nothing is
   * granted yet, because a user who means to let an agent drive the page should
   * not have to click twice. */
  offerWrite: boolean;
  /** Offer to withdraw. Only ever shown while something is granted. */
  offerRevoke: boolean;
}

/**
 * What the banner should say, or `null` when there is nothing it could grant.
 *
 * `null` for every state that is not a loaded page: consent is scoped to an
 * origin and `grant_consent` refuses without one, so offering the control there
 * would be a button that always errors.
 */
export function consentBanner(snapshot: BrowserSnapshot | null): ConsentBanner | null {
  const origin = snapshot?.origin ?? null;
  if (!snapshot || snapshot.availability !== "ready" || !origin) return null;

  const request = snapshot.lastAgentRequest;
  const granted = snapshot.consent.reads && snapshot.consent.origin === origin;
  const writes = granted && snapshot.consent.writes;

  if (request && request.refused) {
    const verb = request.needsWrites ? "read and control" : "read";
    return {
      kind: "asked",
      message: `${describeAsker(request)} wants to ${verb} ${origin} — it called ${request.tool} and was refused.`,
      // Only the grant it actually needs is offered as the obvious click, but
      // the weaker one stays available: a user asked for control may well want
      // to allow reading and nothing more.
      offerRead: !granted,
      offerWrite: !writes,
      offerRevoke: granted,
    };
  }

  if (granted) {
    const scope = writes ? "read and control" : "read";
    return {
      kind: request ? "active" : "granted",
      message: request
        ? `${describeAsker(request)} can ${scope} ${origin} — last called ${request.tool}.`
        : `Agents may ${scope} ${origin}. This applies to this page only and is forgotten when it changes.`,
      offerRead: false,
      offerWrite: !writes,
      offerRevoke: true,
    };
  }

  return {
    kind: "idle",
    message: `No agent can read ${origin}.`,
    offerRead: true,
    offerWrite: true,
    offerRevoke: false,
  };
}

/**
 * How to name the program that asked.
 *
 * A pid of 0 means the OS would not say who it was, and that is reported rather
 * than dressed up: a banner that invented a plausible name would be the one
 * place this feature lied to the person granting access.
 */
function describeAsker(request: BrowserAgentRequest): string {
  return request.pid === 0 ? request.program : `${request.program} (pid ${request.pid})`;
}

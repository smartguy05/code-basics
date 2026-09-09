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
// page. **That is accepted.** `pageVisible` is deliberately *not* an occlusion
// mechanism: it does not consult which other panels are open, and adding that
// would be a redesign rather than a fix.
//
// It is false for exactly three things, and each is a real absence of a place to
// paint rather than something being in front:
//
//   1. the panel is minimized,
//   2. the plugin was switched off (and the host then *drops* the webview, so a
//      disabled browser keeps no WebView2 process, no cookie jar and no
//      connection),
//   3. the measured rect is unusable — the `createResizeGate` 0×0 lesson,
//      extended to the DPI case below.

// --- Where the layout lives -------------------------------------------------

/**
 * The localStorage key the browser panel persists its position and size under.
 *
 * Follows the `cb.<thing>.layout` convention shared with the agent panel
 * (`cb.agentPanel.layout`), Notes (`cb.notes.layout`), SQL (`cb.sql.layout`),
 * Running (`cb.running.layout`), the launcher (`cb.launcher.layout`) and the
 * terminals (`cb.terminal.layout:<root>`).
 *
 * Unscoped by workspace, like SQL's and unlike the terminals': there is one
 * browser panel for the whole application — "verify my deployment" is not
 * repo-specific — so there is nothing to scope it to.
 */
// Type-only, so it is erased at compile time and this module still imports
// nothing at runtime — which is what keeps it runnable under vitest's node
// environment with no DOM.
import type { BrowserAgentRequest, BrowserSnapshot } from "../ipc/types";

export const BROWSER_LAYOUT_KEY = "cb.browser.layout";

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
  rect: PageRectDecision;
}

/**
 * Whether the OS webview should be visible right now.
 *
 * **Not an occlusion mechanism** — see the module header. It consults no other
 * panel, and the page painting over Notes, a terminal or Search Everywhere is
 * an accepted cost of the engine rather than a bug for this function to work
 * around.
 */
export function pageVisible(input: PageVisibilityInput): boolean {
  const { state, enabled, minimized, rect } = input;
  return (
    browserPanelMounted(state, enabled) && !minimized && rect.ok
  );
}

/**
 * Why the page is not on screen even though the panel is, or `null` when it is.
 *
 * Minimized is deliberately **not** a reason: the user did that, they know, and
 * the pill already says so. What needs explaining is the case where the panel is
 * open and expanded and the page still is not there.
 */
export function hiddenPageReason(input: PageVisibilityInput): string | null {
  const { state, enabled, minimized, rect } = input;
  if (!browserPanelMounted(state, enabled) || minimized) return null;
  return rect.ok ? null : rect.reason;
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

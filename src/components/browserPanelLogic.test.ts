import { describe, expect, it } from "vitest";
import type { BrowserAgentRequest, BrowserSnapshot } from "../ipc/types";
import {
  browserLayoutKey,
  consentBanner,
  READ_CONSENT_ACTION,
  WRITE_CONSENT_ACTION,
  browserPanelAfterFeatureChange,
  browserPanelMounted,
  CLOSED_BROWSER_PANEL,
  clampBrowserTop,
  closeBrowserPanel,
  hiddenPageReason,
  occludedByAbovePanels,
  occludedByPanels,
  openBrowserPanel,
  pageRect,
  pageVisible,
  pillLabel,
  rectsOverlap,
  urlBarValue,
  type BrowserPanelState,
  type PageRectDecision,
  type PanelGeometry,
} from "./browserPanelLogic";

const SCALE_1 = { devicePixelRatio: 1, scaleFactor: 1 };
const RECT: PanelGeometry = { left: 40, top: 60, width: 800, height: 500 };
const GOOD: PageRectDecision = { ok: true, rect: RECT };

function open(): BrowserPanelState {
  return openBrowserPanel(CLOSED_BROWSER_PANEL);
}

describe("browserLayoutKey", () => {
  it("is scoped per codebase, like the terminal key", () => {
    // Each open codebase keeps its own live page now, so its geometry is its
    // own too — a fresh browser in one codebase must not adopt another's
    // remembered position.
    expect(browserLayoutKey("C:/x")).toBe("cb.browser.layout:C:/x");
  });

  it("differs for two different roots", () => {
    expect(browserLayoutKey("C:/a")).not.toBe(browserLayoutKey("C:/b"));
  });

  it("does not collide with any other panel's key", () => {
    // Every unscoped key in the app today. A collision would make two panels
    // overwrite each other's remembered position, which reads as "my window
    // keeps jumping" and is close to impossible to attribute.
    for (const other of [
      "cb.sql.layout",
      "cb.notes.layout",
      "cb.agentPanel.layout",
      "cb.running.layout",
      "cb.launcher.layout",
    ]) {
      expect(browserLayoutKey("C:/x")).not.toBe(other);
    }
  });
});

describe("openBrowserPanel", () => {
  it("opens a closed panel", () => {
    expect(open().open).toBe(true);
  });

  it("advances the token even when the panel is already open", () => {
    // "Open the browser" means restore, not nothing, once it can be minimized —
    // and a boolean cannot express that.
    const first = open();
    const second = openBrowserPanel(first);
    expect(second.open).toBe(true);
    expect(second.restoreToken).not.toBe(first.restoreToken);
  });

  it("advances the token on every request, so a restore is always observable", () => {
    let state = CLOSED_BROWSER_PANEL;
    const seen = new Set<number>();
    for (let i = 0; i < 5; i += 1) {
      state = openBrowserPanel(state);
      seen.add(state.restoreToken);
    }
    expect(seen.size).toBe(5);
  });
});

describe("closeBrowserPanel", () => {
  it("closes an open panel", () => {
    expect(closeBrowserPanel(open()).open).toBe(false);
  });

  it("keeps the token, so a later open still differs from every value seen", () => {
    const opened = open();
    const closed = closeBrowserPanel(opened);
    expect(closed.restoreToken).toBe(opened.restoreToken);
    expect(openBrowserPanel(closed).restoreToken).not.toBe(opened.restoreToken);
  });

  it("returns the same object when nothing was open", () => {
    // The same-reference-on-no-op discipline: a stray close must cost no render.
    expect(closeBrowserPanel(CLOSED_BROWSER_PANEL)).toBe(CLOSED_BROWSER_PANEL);
  });
});

describe("browserPanelAfterFeatureChange", () => {
  it("closes the panel when the plugin is switched off", () => {
    // Hidden-but-mounted would keep a WebView2 process, a cookie jar and a
    // polling page alive with no URL bar and no Stop.
    expect(browserPanelAfterFeatureChange(open(), false).open).toBe(false);
  });

  it("returns the same object when the feature is enabled", () => {
    const state = open();
    expect(browserPanelAfterFeatureChange(state, true)).toBe(state);
  });

  it("returns the same object when the panel is already closed", () => {
    // Both no-op paths, so the effect that applies this cannot loop.
    expect(browserPanelAfterFeatureChange(CLOSED_BROWSER_PANEL, false)).toBe(
      CLOSED_BROWSER_PANEL,
    );
  });
});

describe("browserPanelMounted", () => {
  it("needs both the request and the feature", () => {
    expect(browserPanelMounted(open(), true)).toBe(true);
    expect(browserPanelMounted(open(), false)).toBe(false);
    expect(browserPanelMounted(CLOSED_BROWSER_PANEL, true)).toBe(false);
  });
});

describe("pageRect", () => {
  it("returns rounded whole-pixel bounds for an ordinary measurement", () => {
    const decision = pageRect(
      { left: 40.4, top: 60.6, width: 800.5, height: 499.4 },
      SCALE_1,
    );
    expect(decision).toEqual({
      ok: true,
      rect: { left: 40, top: 61, width: 801, height: 499 },
    });
  });

  it("refuses a 0x0 measurement", () => {
    // The createResizeGate lesson: a ResizeObserver reports 0x0 while an
    // element is hidden, and fires once on observe() before layout — so this is
    // the *usual* first measurement, not an exotic one.
    for (const size of [
      { width: 0, height: 500 },
      { width: 800, height: 0 },
      { width: 0, height: 0 },
    ]) {
      const decision = pageRect({ left: 0, top: 0, ...size }, SCALE_1);
      expect(decision.ok).toBe(false);
    }
  });

  it("refuses a negative size rather than taking its absolute value", () => {
    expect(pageRect({ ...RECT, width: -10 }, SCALE_1).ok).toBe(false);
  });

  it("says the panel has no size, so the panel can explain itself", () => {
    const decision = pageRect({ ...RECT, height: 0 }, SCALE_1);
    expect(decision.ok).toBe(false);
    if (!decision.ok) expect(decision.reason).toContain("no size");
  });

  it("refuses anything non-finite", () => {
    // A NaN reaches set_bounds as an unpredictable number, and an OS surface at
    // an unpredictable position is worse than one that is not placed.
    const bad = [
      { ...RECT, left: Number.NaN },
      { ...RECT, top: Number.POSITIVE_INFINITY },
      { ...RECT, width: Number.NaN },
      { ...RECT, height: Number.NEGATIVE_INFINITY },
    ];
    for (const geometry of bad) {
      expect(pageRect(geometry, SCALE_1).ok).toBe(false);
    }
    expect(pageRect(RECT, { devicePixelRatio: Number.NaN, scaleFactor: 1 }).ok).toBe(
      false,
    );
  });

  it("refuses when the two scale readings disagree, naming both", () => {
    // wry's bounds are logical pixels, and CSS pixels equal logical pixels only
    // while these match. The spike measured scale factor 1.0 only, so the
    // scaled case is untested — a correction factor here would be a guess that
    // paints an OS surface somewhere the user is not looking.
    const decision = pageRect(RECT, { devicePixelRatio: 1.5, scaleFactor: 1 });
    expect(decision.ok).toBe(false);
    if (!decision.ok) {
      expect(decision.reason).toContain("1.5");
      expect(decision.reason).toContain("scaling");
    }
  });

  it("accepts a matched non-unit scale", () => {
    // A high-DPI display where the two agree is the ordinary case, not the
    // refused one.
    expect(pageRect(RECT, { devicePixelRatio: 2, scaleFactor: 2 }).ok).toBe(true);
  });

  it("tolerates floating-point noise between the two readings", () => {
    // 1.25 arrives from two different sources; refusing on the last bit would
    // hide the page on every 125% display.
    expect(
      pageRect(RECT, { devicePixelRatio: 1.25, scaleFactor: 1.2500001 }).ok,
    ).toBe(true);
  });

  it("accepts a rect at the window origin", () => {
    // 0 is a legal position and must not be mistaken for a degenerate size.
    expect(pageRect({ left: 0, top: 0, width: 10, height: 10 }, SCALE_1).ok).toBe(
      true,
    );
  });
});

describe("pageVisible", () => {
  const base = {
    state: open(),
    enabled: true,
    minimized: false,
    active: true,
    setupOpen: false,
    occluded: false,
    rect: GOOD,
  };

  it("shows the page when the panel is open, enabled, expanded, foreground, unobscured and measured", () => {
    expect(pageVisible(base)).toBe(true);
  });

  it("hides it when minimized", () => {
    // A React `hidden` cannot hide an OS surface; only set_visible(false) can.
    expect(pageVisible({ ...base, minimized: true })).toBe(false);
  });

  it("hides it when the plugin is switched off", () => {
    expect(pageVisible({ ...base, enabled: false })).toBe(false);
  });

  it("hides it when the panel is closed", () => {
    expect(pageVisible({ ...base, state: CLOSED_BROWSER_PANEL })).toBe(false);
  });

  it("hides it when the rect was refused", () => {
    expect(
      pageVisible({ ...base, rect: { ok: false, reason: "no size" } }),
    ).toBe(false);
  });

  it("hides it when the workspace is backgrounded", () => {
    // The stacking fix: an OS webview composites above the DOM, so a background
    // workspace's page would paint over the foreground one. Only the active
    // workspace's page may show.
    expect(pageVisible({ ...base, active: false })).toBe(false);
  });

  it("shows only the active workspace's page", () => {
    expect(pageVisible({ ...base, active: true })).toBe(true);
    expect(pageVisible({ ...base, active: false })).toBe(false);
  });

  it("hides it while the agent-setup modal is open (bug 2)", () => {
    // The modal is DOM and the webview composites above it, so it would open
    // behind the page unless the page hides.
    expect(pageVisible({ ...base, setupOpen: true })).toBe(false);
  });

  it("hides it while an occluding surface covers the page (bug 3)", () => {
    expect(pageVisible({ ...base, occluded: true })).toBe(false);
  });
});

describe("hiddenPageReason", () => {
  const base = {
    state: open(),
    enabled: true,
    minimized: false,
    active: true,
    setupOpen: false,
    occluded: false,
    rect: GOOD,
  };

  it("is null when the page is on screen", () => {
    expect(hiddenPageReason(base)).toBeNull();
  });

  it("explains a refused rect, because that is the case the user cannot see the cause of", () => {
    const reason = hiddenPageReason({
      ...base,
      rect: { ok: false, reason: "the panel has no size on screen yet" },
    });
    expect(reason).toBe("the panel has no size on screen yet");
  });

  it("says nothing about being minimized", () => {
    // The user did that, and the pill already says so. Explaining it would be
    // noise in the one place the panel is not even rendering a body.
    expect(
      hiddenPageReason({
        ...base,
        minimized: true,
        rect: { ok: false, reason: "the panel has no size on screen yet" },
      }),
    ).toBeNull();
  });

  it("is null when the workspace is backgrounded, even with a refused rect", () => {
    // The user switched tabs; a scary reason under a hidden panel would be
    // noise, exactly like the minimized case.
    expect(
      hiddenPageReason({
        ...base,
        active: false,
        rect: { ok: false, reason: "the panel has no size on screen yet" },
      }),
    ).toBeNull();
  });

  it("says nothing when the panel is not mounted at all", () => {
    expect(
      hiddenPageReason({ ...base, enabled: false, rect: { ok: false, reason: "x" } }),
    ).toBeNull();
  });

  it("says nothing while the setup modal is open, even with a refused rect", () => {
    expect(
      hiddenPageReason({
        ...base,
        setupOpen: true,
        rect: { ok: false, reason: "the panel has no size on screen yet" },
      }),
    ).toBeNull();
  });

  it("says nothing while an occluding surface covers it — the surface is the explanation", () => {
    expect(
      hiddenPageReason({
        ...base,
        occluded: true,
        rect: { ok: false, reason: "the panel has no size on screen yet" },
      }),
    ).toBeNull();
  });
});

describe("rectsOverlap", () => {
  const page: PanelGeometry = { left: 100, top: 100, width: 200, height: 200 };

  it("is true when two rects overlap", () => {
    expect(rectsOverlap(page, { left: 250, top: 250, width: 100, height: 100 })).toBe(true);
  });

  it("is false for edge-touching rects (a panel flush beside the page)", () => {
    // b starts exactly at page's right edge (300) — covering no pixel of it.
    expect(rectsOverlap(page, { left: 300, top: 100, width: 100, height: 100 })).toBe(false);
  });

  it("is false for fully disjoint rects", () => {
    expect(rectsOverlap(page, { left: 500, top: 500, width: 50, height: 50 })).toBe(false);
  });

  it("is true when one rect is entirely inside the other", () => {
    expect(rectsOverlap(page, { left: 150, top: 150, width: 20, height: 20 })).toBe(true);
  });
});

describe("occludedByPanels", () => {
  const page: PanelGeometry = { left: 100, top: 100, width: 200, height: 200 };

  it("is false for an empty list", () => {
    expect(occludedByPanels(page, [])).toBe(false);
  });

  it("is true when any panel overlaps the page", () => {
    expect(
      occludedByPanels(page, [
        { left: 500, top: 500, width: 50, height: 50 },
        { left: 150, top: 150, width: 40, height: 40 },
      ]),
    ).toBe(true);
  });

  it("is false when every panel is beside or clear of the page", () => {
    expect(
      occludedByPanels(page, [
        { left: 300, top: 100, width: 100, height: 100 },
        { left: 100, top: 400, width: 100, height: 100 },
      ]),
    ).toBe(false);
  });
});

describe("occludedByAbovePanels", () => {
  const page: PanelGeometry = { left: 100, top: 100, width: 200, height: 200 };
  const overlapping: PanelGeometry = { left: 150, top: 150, width: 40, height: 40 };
  const clear: PanelGeometry = { left: 500, top: 500, width: 50, height: 50 };

  it("is false for an empty list", () => {
    expect(occludedByAbovePanels(page, 5, [])).toBe(false);
  });

  it("is true when a higher-offset peer overlaps the page", () => {
    expect(occludedByAbovePanels(page, 3, [{ rect: overlapping, offset: 7 }])).toBe(true);
  });

  it("is false when a higher-offset peer does not overlap", () => {
    expect(occludedByAbovePanels(page, 3, [{ rect: clear, offset: 7 }])).toBe(false);
  });

  it("is false when a LOWER-offset peer overlaps (the raised browser wins)", () => {
    // The regression this fix is about: a terminal the browser was raised over
    // must not blank the page.
    expect(occludedByAbovePanels(page, 7, [{ rect: overlapping, offset: 3 }])).toBe(false);
  });

  it("is false when an EQUAL-offset peer overlaps (a tie is not 'above')", () => {
    expect(occludedByAbovePanels(page, 5, [{ rect: overlapping, offset: 5 }])).toBe(false);
  });

  it("occludes when any above-peer overlaps, even amid below/clear peers", () => {
    expect(
      occludedByAbovePanels(page, 4, [
        { rect: overlapping, offset: 2 }, // below — ignored
        { rect: clear, offset: 9 }, // above but clear — ignored
        { rect: overlapping, offset: 6 }, // above and overlapping — occludes
      ]),
    ).toBe(true);
  });
});

describe("clampBrowserTop", () => {
  it("pushes a rect whose top is above the app chrome down to it, shrinking height", () => {
    // top 10 under a chrome bottom of 80: page must start at 80 and lose 70 of
    // its height so the bottom edge stays put.
    expect(
      clampBrowserTop({ left: 40, top: 10, width: 800, height: 500 }, 80),
    ).toEqual({ left: 40, top: 80, width: 800, height: 430 });
  });

  it("leaves a rect already below the chrome unchanged", () => {
    const rect = { left: 40, top: 120, width: 800, height: 500 };
    expect(clampBrowserTop(rect, 80)).toEqual(rect);
  });

  it("never yields a negative height", () => {
    const clamped = clampBrowserTop({ left: 0, top: 0, width: 100, height: 30 }, 80);
    expect(clamped.height).toBe(0);
  });
});

describe("urlBarValue", () => {
  it("shows the page's URL when the user is not editing", () => {
    expect(urlBarValue(null, "https://app.example.com/orders")).toBe(
      "https://app.example.com/orders",
    );
  });

  it("shows the draft while the user is editing", () => {
    expect(urlBarValue("https://ne", "https://app.example.com/")).toBe(
      "https://ne",
    );
  });

  it("shows an empty draft rather than re-filling from the page", () => {
    // A user who selected all and deleted is mid-edit. Re-filling under their
    // caret is the bug every address bar that syncs too eagerly has.
    expect(urlBarValue("", "https://app.example.com/")).toBe("");
  });

  it("shows about:blank as empty", () => {
    // It is the panel's own resting state, not an address anyone typed, and
    // showing it means the first thing the user must do is clear it.
    expect(urlBarValue(null, "about:blank")).toBe("");
    expect(urlBarValue(null, "ABOUT:BLANK")).toBe("");
  });

  it("shows nothing when there is no page", () => {
    expect(urlBarValue(null, null)).toBe("");
  });

  it("trims a URL but leaves its case alone", () => {
    // A path is case-sensitive on most servers; lowercasing it would show a
    // different address from the one loaded.
    expect(urlBarValue(null, "  https://Example.com/Path  ")).toBe(
      "https://Example.com/Path",
    );
  });
});

describe("pillLabel", () => {
  it("prefers the page title, which is what the user recognises", () => {
    expect(pillLabel("Orders — Acme", "https://app.example.com/orders")).toBe(
      "Orders — Acme",
    );
  });

  it("falls back to the host, never the full URL", () => {
    // A real application's URL is a paragraph of path and query, may carry a
    // token, and is unreadable at pill width.
    expect(
      pillLabel(null, "https://app.example.com/orders/12345?token=secret#x"),
    ).toBe("app.example.com");
  });

  it("keeps a port, because that is how two local servers are told apart", () => {
    expect(pillLabel("", "http://localhost:5173/")).toBe("localhost:5173");
  });

  it("drops userinfo from the host", () => {
    expect(pillLabel(null, "https://user:pass@app.example.com/x")).toBe(
      "app.example.com",
    );
  });

  it("falls back to a fixed word rather than an unidentifiable empty bar", () => {
    for (const url of [null, "", "about:blank", "not a url", "://x"]) {
      expect(pillLabel(null, url)).toBe("Browser");
    }
  });

  it("treats a whitespace-only title as absent", () => {
    expect(pillLabel("   \t ", "https://app.example.com/")).toBe(
      "app.example.com",
    );
  });

  it("cuts a very long title with an ellipsis", () => {
    const label = pillLabel("x".repeat(200), null);
    expect(label.length).toBeLessThanOrEqual(40);
    expect(label.endsWith("…")).toBe(true);
  });

  it("leaves a title exactly at the limit alone", () => {
    const exact = "y".repeat(40);
    expect(pillLabel(exact, null)).toBe(exact);
  });
});

// --- The automation consent banner -----------------------------------------

function snapshot(over: Partial<BrowserSnapshot> = {}): BrowserSnapshot {
  return {
    availability: "ready",
    url: "https://app.example.com/orders",
    title: "Orders",
    origin: "https://app.example.com",
    consent: { reads: false, writes: false, origin: null },
    refusedNavigations: 0,
    rejectedMessages: 0,
    lastRefusal: null,
    lastAgentRequest: null,
    ...over,
  };
}

function request(over: Partial<BrowserAgentRequest> = {}): BrowserAgentRequest {
  return {
    pid: 12345,
    program: "codex.cmd",
    tool: "browser_page_text",
    needsWrites: false,
    refused: true,
    ...over,
  };
}

describe("consentBanner", () => {
  it("offers nothing when there is no page to grant against", () => {
    // Consent is scoped to an origin and `grant_consent` refuses without one,
    // so a control here would be a button that always errors.
    expect(consentBanner(null)).toBe(null);
    expect(consentBanner(snapshot({ availability: "blank", origin: null, url: null }))).toBe(null);
    expect(consentBanner(snapshot({ availability: "loading" }))).toBe(null);
    expect(consentBanner(snapshot({ origin: null }))).toBe(null);
  });

  it("is present before anything asks, because the refusal text points at it", () => {
    const banner = consentBanner(snapshot());
    expect(banner?.kind).toBe("idle");
    expect(banner?.offerRead).toBe(true);
    expect(banner?.offerWrite).toBe(true);
    expect(banner?.offerRevoke).toBe(false);
    expect(banner?.message).toContain("https://app.example.com");
  });

  it("names the program and the pid that asked", () => {
    // A user granting access to their own logged-in session is entitled to
    // know which program gets it.
    const banner = consentBanner(snapshot({ lastAgentRequest: request() }));
    expect(banner?.kind).toBe("asked");
    expect(banner?.message).toContain("codex.cmd");
    expect(banner?.message).toContain("12345");
    expect(banner?.message).toContain("browser_page_text");
  });

  it("says what was asked for: reading, or reading and controlling", () => {
    const read = consentBanner(snapshot({ lastAgentRequest: request() }));
    expect(read?.message).toContain("wants to read https://app.example.com");
    const control = consentBanner(
      snapshot({ lastAgentRequest: request({ needsWrites: true, tool: "browser_click" }) }),
    );
    expect(control?.message).toContain("wants to read and control");
  });

  it("reports an unidentifiable caller rather than inventing a name", () => {
    // The one place this feature could lie to the person granting access.
    const banner = consentBanner(
      snapshot({ lastAgentRequest: request({ pid: 0, program: "an unidentified program" }) }),
    );
    expect(banner?.message).toContain("an unidentified program");
    expect(banner?.message).not.toContain("pid 0");
  });

  it("distinguishes a granted-and-used grant from a granted-and-unused one", () => {
    const consent = { reads: true, writes: false, origin: "https://app.example.com" };
    const unused = consentBanner(snapshot({ consent }));
    expect(unused?.kind).toBe("granted");
    expect(unused?.message).toContain("forgotten when it changes");

    const used = consentBanner(
      snapshot({ consent, lastAgentRequest: request({ refused: false }) }),
    );
    expect(used?.kind).toBe("active");
    expect(used?.message).toContain("codex.cmd");
    expect(used?.message).toContain("can read");
  });

  it("keeps offering the stronger grant while only reading is allowed", () => {
    const banner = consentBanner(
      snapshot({ consent: { reads: true, writes: false, origin: "https://app.example.com" } }),
    );
    expect(banner?.offerRead).toBe(false);
    expect(banner?.offerWrite).toBe(true);
    expect(banner?.offerRevoke).toBe(true);
  });

  it("offers only withdrawal once everything is granted", () => {
    const banner = consentBanner(
      snapshot({ consent: { reads: true, writes: true, origin: "https://app.example.com" } }),
    );
    expect(banner?.offerRead).toBe(false);
    expect(banner?.offerWrite).toBe(false);
    expect(banner?.offerRevoke).toBe(true);
  });

  it("treats a grant for another page as no grant at all", () => {
    // The rule the whole consent model rests on: consent is a statement about
    // a page. A banner reading "agents may read this" over a different page
    // would be the wording that gets a grant given for the wrong thing.
    const banner = consentBanner(
      snapshot({ consent: { reads: true, writes: true, origin: "https://other.example.com" } }),
    );
    expect(banner?.kind).toBe("idle");
    expect(banner?.offerRead).toBe(true);
    expect(banner?.offerRevoke).toBe(false);
  });

  it("quotes the Rust consent labels verbatim", () => {
    // Every refusal an agent reads is built around these exact words, so a
    // reworded button sends the user hunting for a control that does not
    // exist. `consent_tests.rs` reads this file and fails if they drift.
    expect(READ_CONSENT_ACTION).toBe("Allow agents to read this page");
    expect(WRITE_CONSENT_ACTION).toBe("Allow agents to read and control this page");
  });
});

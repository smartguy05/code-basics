import {
  useCallback,
  useEffect,
  useRef,
  useState,
  type PointerEvent as ReactPointerEvent,
} from "react";
import type { ArchGraph } from "../../ipc/types";
import { derivationLabel, warningSummary } from "./architectureLogic";
import { targetsByDomId, type Target } from "./nodeTargets";
import { fit, IDENTITY, panBy, zoomAt, type View } from "./panZoomLogic";

/**
 * One rendered diagram: the mermaid text turned into an SVG you can pan, zoom
 * and click, with the two things a picture cannot say for itself — where it
 * came from, and what it could not draw — stated beside it.
 *
 * This file is a rendering shell and nothing else. Every decision it appears to
 * make is imported: the transform arithmetic is `panZoomLogic`, where a click
 * goes is `nodeTargets`, and both labels are `architectureLogic`. What is left
 * here is DOM plumbing, which the vitest suite (node environment, no jsdom)
 * cannot reach and therefore must not contain a decision.
 *
 * # Four things here are load-bearing and were each established the hard way
 *
 * **The import is dynamic.** `mermaid` is a large package and five other tabs
 * must not pay for it, so it arrives through `await import("mermaid")` inside
 * an effect and is cached at module scope so switching diagrams costs nothing
 * after the first. The import *failing* is a real state under this app's strict
 * CSP, not a theoretical one, and it is reported as itself — "the diagram is
 * blank" is not an acceptable way to express a renderer that never loaded.
 *
 * **`htmlLabels: false` at the top level, not just under `flowchart`.** The
 * Phase 0 spike served a production bundle under the byte-identical policy from
 * `tauri.conf.json` and measured it: with only the per-diagram key, class, ER
 * and state diagrams still emit `foreignObject`, which the CSP refuses. The
 * per-diagram keys are kept beside it because they are what older diagram
 * types read. Do not trim this to the root key alone on the grounds that the
 * type definitions call the others deprecated.
 *
 * **Clicks are delegated, never mermaid's `click … call`.** That directive
 * requires `securityLevel: "loose"`, i.e. arbitrary callbacks named by a file
 * an agent or a user wrote — an authored diagram is untrusted input here. So
 * one listener sits on the container, walks up to the nearest box, and resolves
 * the element's DOM id through {@link targetsByDomId}. A node that resolves to
 * nothing is not clickable **and does not look clickable**: no pointer cursor
 * and no hover outline, because an affordance that does nothing is worse than
 * none.
 *
 * **The viewBox is removed.** Mermaid ships one, and it silently scales the
 * whole drawing — leaving it in place means the fit arithmetic is computing in
 * one space and the browser is drawing in another, which looks like a zoom that
 * is subtly wrong rather than like a bug. With it gone, one SVG user unit is
 * one CSS pixel and the single `transform` on the wrapper `<g>` is the only
 * thing between content coordinates and the screen.
 */

/** What the canvas needs. `graph` is `null` for a diagram we did not derive. */
export interface DiagramCanvasProps {
  /** Mermaid text. Blank renders an empty state rather than an error. */
  source: string;
  /**
   * The graph the source was rendered from, when there is one. `null` for an
   * authored file: its node ids were typed by a person or an agent and mean
   * nothing to us, so **no box in it is clickable**.
   *
   * That is the state today and not a description of a fallback. Matching an
   * authored diagram's ids against the symbol index is `targetsForAuthored`'s
   * job, it belongs to a caller that has the index, and **no caller passes one
   * yet** — `ArchitectureView` loads a saved diagram with `graph: null` and
   * nothing more. This component is given a graph or given nothing.
   */
  graph: ArchGraph | null;
  warnings: string[];
  onOpenNode: (target: { path: string; line?: number }) => void;
  onError: (message: string | null) => void;
  /**
   * Where the diagram came from, for the badge.
   *
   * **Added to the agreed props contract, and optional so nothing that predates
   * it breaks.** The badge is required to be drawn by the view rather than read
   * out of the source, because a `%%` comment is something a user can delete
   * and a caption nobody can delete is the entire point. For a derived graph
   * the origin is `graph.derivation`, which is already here — but for a saved
   * file it lives on the `DiagramFile`, which is not, and a canvas that could
   * only caption half the diagrams it renders would not be doing the job. Pass
   * `file.derivation` (and `file.edited`) for a saved diagram; omit both for a
   * derived one and `graph.derivation` is used.
   */
  derivation?: Parameters<typeof derivationLabel>[0] | null;
  /** An inferred diagram a person has since changed; appended to the badge. */
  edited?: boolean;
  /**
   * Where this diagram was last being looked at, when that is known.
   *
   * Optional, and `null`/omitted means "fit it", which is what this component
   * does with no answer at all — so a caller that keeps no memory is unchanged.
   * It exists because the Architecture tab mounts conditionally: leaving it
   * destroys the canvas, and re-deriving a thirty-project diagram back at the
   * fit view every time is a tax on looking at anything else. Validation of the
   * stored value is the caller's, in `viewportLogic`.
   */
  initialView?: View | null;
  /**
   * Told where the diagram has been moved to, so the caller can remember it.
   *
   * Debounced inside this component, not by the caller: pan fires per pointer
   * frame, and a listener that writes to storage on each one would do so
   * hundreds of times a drag.
   */
  onViewChange?: (view: View) => void;
}

/** Room left around the drawing when fitting, in screen pixels. */
const FIT_PADDING = 24;

/**
 * One notch of the zoom buttons, as a synthetic wheel delta.
 *
 * Expressed in the wheel's own units and pushed through the same {@link zoomAt}
 * the wheel uses, so the buttons and the wheel cannot drift apart — two zoom
 * paths with two step sizes is a difference nobody can articulate and everybody
 * notices. 200 is a little under two mouse notches: about 27% a press.
 */
const BUTTON_ZOOM_DELTA = 200;

/**
 * How far the pointer may travel and still count as a click, in pixels.
 *
 * A drag that ends on a box is a pan, not a click, and opening a file at the
 * end of every pan that happens to finish over a project would be maddening. A
 * few pixels of slop because a mouse moves while a button is going down.
 */
const CLICK_SLOP = 4;

/**
 * How long the view must sit still before it is reported, in milliseconds.
 *
 * A drag emits a new view per pointer frame; a wheel emits one per notch. Both
 * settle well inside this, so a gesture is reported once, at the place it
 * finished, rather than at every place it passed through.
 */
const VIEW_SETTLE_MS = 250;

/**
 * Every box a click could land on, node and container alike.
 *
 * Keyed on the DOM `id`, because **mermaid 11.16.1 does not put `data-id` on a
 * node**: its flowchart shapes set `id` and `data-look`, and `data-id` is on
 * edge paths only. This selector previously read `g.node[data-id]`, which
 * matched nothing at all — the diagram rendered, every box looked inert because
 * it *was* inert, and no error was reported anywhere because "no box resolved"
 * is a legitimate state this component is built to express.
 *
 * `g.cluster` is here beside `g.node` because Mermaid draws a `subgraph` as a
 * cluster, so a solution and a Cargo/npm workspace can never match a node
 * selector however it is written — and both carry the path of the file that
 * declares them, which is a real destination and the only place that file
 * appears in the picture. Whether either kind is *clickable* is still decided
 * one box at a time by `nodeTargets`.
 */
const BOX_SELECTOR = "g.node[id], g.cluster[id]";

/** Marks the boxes that resolved. Only these get a cursor and a hover. */
const OPENABLE_ATTR = "data-cb-open";

const SVG_NS = "http://www.w3.org/2000/svg";

/**
 * What is passed to {@link derivationLabel} when nobody told us the origin.
 *
 * A shape it does not recognise is the case that function already answers, with
 * "Origin unknown" — so the sentinel goes through the same door as a malformed
 * value rather than this file keeping a second copy of that wording, which
 * could then disagree with the first. The badge is always drawn: a diagram with
 * no origin stated reads as one whose origin was checked and found
 * unremarkable, which is the misreading the badge exists to prevent.
 */
const UNSTATED = {} as Parameters<typeof derivationLabel>[0];

/**
 * Exactly the configuration the spike verified against this app's CSP.
 *
 * The cast is the one in this file and it is deliberate. `StateDiagramConfig`
 * and `ErDiagramConfig` declare no `htmlLabels` in mermaid 11.16.1's published
 * types — they are all-optional, so TypeScript's weak-type check rejects an
 * object with nothing in common with them — yet the spike *measured* both still
 * emitting `foreignObject` when the key is absent, and `foreignObject` is what
 * this app's CSP refuses. Dropping the keys to satisfy the checker would trade a
 * type definition's opinion for a policy violation the user sees as an empty
 * tab. `securityLevel: "strict"` is the other half of the same rule: `"loose"`
 * is what mermaid's `click … call` needs, and these diagrams are written by
 * agents and users.
 */
const MERMAID_CONFIG = {
  startOnLoad: false,
  securityLevel: "strict",
  htmlLabels: false,
  flowchart: { htmlLabels: false },
  class: { htmlLabels: false },
  state: { htmlLabels: false },
  er: { htmlLabels: false },
} as unknown as Parameters<Mermaid["initialize"]>[0];

type Mermaid = (typeof import("mermaid"))["default"];

/**
 * The renderer, imported at most once per session.
 *
 * Cached as the promise rather than the module so two canvases mounting
 * together share one import, and cleared on failure so a transient one can be
 * retried rather than poisoning the tab for the rest of the session.
 */
let mermaidPromise: Promise<Mermaid> | null = null;

function loadMermaid(): Promise<Mermaid> {
  if (mermaidPromise === null) {
    mermaidPromise = import("mermaid")
      .then((module) => {
        const mermaid = module.default;
        mermaid.initialize(MERMAID_CONFIG);
        return mermaid;
      })
      .catch((error: unknown) => {
        mermaidPromise = null;
        throw error;
      });
  }
  return mermaidPromise;
}

/** Ids given to mermaid must be unique per render and are never reused. */
let renderSeq = 0;

/**
 * Hover and cursor for the boxes that resolved, and nothing for the ones that
 * did not.
 *
 * Scoped to this component's host so it cannot reach any other SVG in the app,
 * and written only in the palette's own custom properties — no colour literal
 * enters here. Inline because the mermaid output is inserted as markup and has
 * no class of ours to hang a stylesheet rule on from `styles.css`.
 */
const CANVAS_CSS = `
.cb-diagram-host { position: relative; overflow: hidden; }
.cb-diagram-host svg { display: block; width: 100%; height: 100%; max-width: none; }
.cb-diagram-host [${OPENABLE_ATTR}] { cursor: pointer; }
.cb-diagram-host [${OPENABLE_ATTR}]:hover > rect,
.cb-diagram-host [${OPENABLE_ATTR}]:hover > polygon,
.cb-diagram-host [${OPENABLE_ATTR}]:hover > circle,
.cb-diagram-host [${OPENABLE_ATTR}]:hover > ellipse,
.cb-diagram-host [${OPENABLE_ATTR}]:hover > path {
  stroke: var(--accent);
  stroke-width: 2px;
}
`;

/** What the renderer is doing, as far as the user needs to know. */
type Phase = "loading" | "ready" | "unavailable";

export function DiagramCanvas({
  source,
  graph,
  warnings,
  onOpenNode,
  onError,
  derivation,
  edited = false,
  initialView = null,
  onViewChange,
}: DiagramCanvasProps) {
  const hostRef = useRef<HTMLDivElement | null>(null);
  /** The one `<g>` the transform is written to. Ours, not mermaid's. */
  const panRef = useRef<SVGGElement | null>(null);
  const svgRef = useRef<SVGSVGElement | null>(null);
  /**
   * The id handed to mermaid for the drawing currently mounted.
   *
   * Kept rather than read back off the SVG because it is half of every DOM id
   * in there — mermaid prefixes it onto each element — and the marking pass
   * cannot anchor a match without it. A ref, not state: it changes with the
   * drawing, and re-rendering the component on it would gain nothing.
   */
  const renderIdRef = useRef<string | null>(null);
  /** DOM element id → destination, for the boxes that resolved. */
  const targetsRef = useRef<Map<string, Target>>(new Map());
  const draggingRef = useRef<{ pointer: number; x: number; y: number } | null>(null);
  const movedRef = useRef(false);
  /**
   * The openable box the pointer went down on, resolved before the drag takes
   * pointer capture.
   *
   * `click` cannot be trusted to say what was pressed here. `onPointerDown`
   * calls `setPointerCapture` on the host so a drag that leaves the window
   * still pans, and Chromium then retargets the compatibility mouse events —
   * `click` included — to the capture element. So `event.target` on the click
   * is the host `<div>`, `closest()` finds nothing, and every node silently
   * refuses to open. Verified in the running app: neutering `setPointerCapture`
   * made the identical click work.
   *
   * Reading the box at `pointerdown`, while the event still names the `<tspan>`
   * or `<rect>` under the cursor, is what makes the click honest. It is cleared
   * on every press so a press on empty canvas cannot leave the previous box
   * armed.
   */
  const pressedBoxRef = useRef<string | null>(null);

  const [phase, setPhase] = useState<Phase>("loading");
  const [loadError, setLoadError] = useState<string | null>(null);
  const [renderError, setRenderError] = useState<string | null>(null);
  const [view, setView] = useState<View>(initialView ?? IDENTITY);
  /**
   * Whether a restored view is still waiting to survive the first render.
   *
   * The render effect fits the diagram to the host as soon as mermaid returns,
   * which would immediately overwrite whatever was restored — the user would
   * see their view for one frame and then lose it, which is worse than never
   * having had it. Read and cleared exactly once, so every later render (an
   * edit being saved, a source change) still fits as it always did.
   */
  const restoringRef = useRef(initialView !== null);
  const [warningsOpen, setWarningsOpen] = useState(false);
  /**
   * Bumped once per successful render, and the only thing that tells the
   * marking pass below that there is fresh SVG to walk. It exists so that
   * *rendering* can depend on the source alone: a parent that recomputes an
   * equal-but-new `graph` object on every keystroke would otherwise re-run
   * mermaid and throw away wherever the user had panned to.
   */
  const [drawnAt, setDrawnAt] = useState(0);

  // The callbacks are held in refs so a caller that passes a fresh closure on
  // every render does not re-run the render effect — re-rendering a diagram
  // resets the user's pan and zoom, which reads as the picture jumping.
  const onErrorRef = useRef(onError);
  const onOpenNodeRef = useRef(onOpenNode);
  const onViewChangeRef = useRef(onViewChange);
  useEffect(() => {
    onErrorRef.current = onError;
    onOpenNodeRef.current = onOpenNode;
    onViewChangeRef.current = onViewChange;
  }, [onError, onOpenNode, onViewChange]);

  // Report the view once it has settled. The cleanup cancels the pending
  // report, so a gesture in progress replaces its own timer and only the place
  // it came to rest is ever announced.
  useEffect(() => {
    if (!onViewChangeRef.current) return;
    const timer = setTimeout(() => onViewChangeRef.current?.(view), VIEW_SETTLE_MS);
    return () => clearTimeout(timer);
  }, [view]);

  // ---- the renderer -------------------------------------------------------

  useEffect(() => {
    let live = true;
    loadMermaid().then(
      () => {
        if (live) setPhase("ready");
      },
      (error: unknown) => {
        if (!live) return;
        setLoadError(messageOf(error));
        setPhase("unavailable");
      },
    );
    return () => {
      live = false;
    };
  }, []);

  // ---- rendering ----------------------------------------------------------

  const fitToHost = useCallback(() => {
    const host = hostRef.current;
    const pan = panRef.current;
    if (!host || !pan) return;
    setView(
      fit(
        contentBox(pan),
        { width: host.clientWidth, height: host.clientHeight },
        FIT_PADDING,
      ),
    );
  }, []);

  useEffect(() => {
    if (phase !== "ready") return;
    const host = hostRef.current;
    if (!host) return;

    if (source.trim() === "") {
      host.replaceChildren();
      panRef.current = null;
      svgRef.current = null;
      renderIdRef.current = null;
      targetsRef.current = new Map();
      setRenderError(null);
      onErrorRef.current(null);
      return;
    }

    let live = true;
    renderSeq += 1;
    const id = `cb-diagram-${renderSeq}`;

    loadMermaid()
      .then((mermaid) => mermaid.render(id, source))
      .then(({ svg }) => {
        if (!live) return;
        const root = mount(host, svg);
        if (!root) throw new Error("The renderer produced no diagram.");
        panRef.current = root.pan;
        svgRef.current = root.svg;
        renderIdRef.current = id;
        setRenderError(null);
        onErrorRef.current(null);
        if (restoringRef.current) restoringRef.current = false;
        else fitToHost();
        setDrawnAt((count) => count + 1);
      })
      .catch((error: unknown) => {
        // A diagram may be hand-written or agent-written, so failing to parse is
        // an ordinary outcome and mermaid's own words are the useful ones. The
        // stale drawing is cleared: an error above a picture reads as a picture
        // that is still true.
        if (!live) return;
        host.replaceChildren();
        panRef.current = null;
        svgRef.current = null;
        renderIdRef.current = null;
        targetsRef.current = new Map();
        const message = messageOf(error);
        setRenderError(message);
        onErrorRef.current(message);
      })
      .finally(() => {
        // Mermaid leaves its scratch element behind when a render throws.
        document.getElementById(`d${id}`)?.remove();
      });

    return () => {
      live = false;
    };
  }, [phase, source, fitToHost]);

  // Which boxes lead somewhere is a property of the graph, not of the drawing,
  // so it is re-decided when either changes and never re-renders the diagram to
  // do it. Old marks are cleared first: a box that stopped resolving must also
  // stop looking as though it resolves.
  useEffect(() => {
    const svg = svgRef.current;
    if (!svg) return;
    for (const marked of svg.querySelectorAll(`[${OPENABLE_ATTR}]`)) {
      marked.removeAttribute(OPENABLE_ATTR);
    }
    targetsRef.current = markOpenable(svg, graph, renderIdRef.current);
  }, [drawnAt, graph]);

  /** One transform, written straight to the DOM: React does not own the SVG. */
  useEffect(() => {
    panRef.current?.setAttribute(
      "transform",
      `translate(${view.x} ${view.y}) scale(${view.k})`,
    );
  });

  useEffect(() => () => hostRef.current?.replaceChildren(), []);

  // ---- wheel --------------------------------------------------------------

  // Native and non-passive: React's onWheel is registered passively, where
  // preventDefault is ignored with only a console warning, and the whole page
  // scrolls while the user is trying to zoom.
  useEffect(() => {
    const host = hostRef.current;
    if (!host) return;
    const onWheel = (event: WheelEvent) => {
      event.preventDefault();
      const rect = host.getBoundingClientRect();
      setView((current) =>
        zoomAt(
          current,
          { x: event.clientX - rect.left, y: event.clientY - rect.top },
          event.deltaY,
        ),
      );
    };
    host.addEventListener("wheel", onWheel, { passive: false });
    return () => host.removeEventListener("wheel", onWheel);
  }, []);

  // ---- clicking -----------------------------------------------------------

  // One delegated listener for the whole diagram, so a re-render cannot leave
  // listeners attached to elements that no longer exist.
  useEffect(() => {
    const host = hostRef.current;
    if (!host) return;
    const onClick = () => {
      const id = pressedBoxRef.current;
      pressedBoxRef.current = null;
      if (movedRef.current) return;
      if (id === null) return;
      const destination = targetsRef.current.get(id);
      if (destination) onOpenNodeRef.current(destination);
    };
    host.addEventListener("click", onClick);
    return () => host.removeEventListener("click", onClick);
  }, []);

  // ---- dragging -----------------------------------------------------------

  const onPointerDown = useCallback((event: ReactPointerEvent<HTMLDivElement>) => {
    if (event.button !== 0) return;
    // Read the box first: setPointerCapture below retargets the click that
    // follows, so this is the last moment the event still names what is under
    // the cursor. See `pressedBoxRef`.
    const target = event.target;
    pressedBoxRef.current =
      target instanceof Element
        ? (target.closest(`[${OPENABLE_ATTR}]`)?.getAttribute("id") ?? null)
        : null;
    draggingRef.current = {
      pointer: event.pointerId,
      x: event.clientX,
      y: event.clientY,
    };
    movedRef.current = false;
    event.currentTarget.setPointerCapture(event.pointerId);
  }, []);

  const onPointerMove = useCallback((event: ReactPointerEvent<HTMLDivElement>) => {
    const drag = draggingRef.current;
    if (!drag || drag.pointer !== event.pointerId) return;
    const dx = event.clientX - drag.x;
    const dy = event.clientY - drag.y;
    if (Math.abs(dx) > CLICK_SLOP || Math.abs(dy) > CLICK_SLOP) movedRef.current = true;
    draggingRef.current = { pointer: event.pointerId, x: event.clientX, y: event.clientY };
    setView((current) => panBy(current, dx, dy));
  }, []);

  const endDrag = useCallback((event: ReactPointerEvent<HTMLDivElement>) => {
    if (draggingRef.current?.pointer !== event.pointerId) return;
    draggingRef.current = null;
    if (event.currentTarget.hasPointerCapture(event.pointerId)) {
      event.currentTarget.releasePointerCapture(event.pointerId);
    }
  }, []);

  // ---- toolbar ------------------------------------------------------------

  const zoomFromCentre = useCallback((delta: number) => {
    const host = hostRef.current;
    if (!host) return;
    const centre = { x: host.clientWidth / 2, y: host.clientHeight / 2 };
    setView((current) => zoomAt(current, centre, delta));
  }, []);

  // ---- labels -------------------------------------------------------------

  const origin = derivation ?? graph?.derivation ?? UNSTATED;
  const badge = derivationLabel(origin, edited);
  const summary = warningSummary(warnings);
  const readable = warnings.filter((warning) => warning.trim() !== "");
  const drawn = phase === "ready" && renderError === null && source.trim() !== "";

  return (
    // `diagram-canvas` is the hook styles.css scopes the toolbar, the badge and
    // the warning list to. Nothing shared may be restyled globally — `.badge`
    // and `.warning` are worn by six other views.
    <div className="main diagram-canvas">
      <style>{CANVAS_CSS}</style>

      <div className="toolbar">
        <button onClick={() => zoomFromCentre(BUTTON_ZOOM_DELTA)} disabled={!drawn}>
          −
        </button>
        <button onClick={() => zoomFromCentre(-BUTTON_ZOOM_DELTA)} disabled={!drawn}>
          +
        </button>
        <span className="muted mono" style={{ fontSize: 11, minWidth: 38 }}>
          {Math.round(view.k * 100)}%
        </span>
        <button onClick={fitToHost} disabled={!drawn}>
          Fit
        </button>
        {/* "100%" is the identity view exactly as panZoomLogic defines it: the
            natural size, top-left in the corner. Re-centring it would be new
            arithmetic invented in a file with no tests to hold it. */}
        <button onClick={() => setView(IDENTITY)} disabled={!drawn}>
          100%
        </button>
        <span className="spacer" style={{ flex: 1 }} />
        {summary && (
          <span className="badge" style={{ borderColor: "var(--skip)", color: "var(--skip)" }}>
            ⚠ {summary}
          </span>
        )}
        <span className="badge">{badge}</span>
      </div>

      {phase === "unavailable" && (
        <div className="error">
          The diagram renderer could not be loaded, so nothing can be drawn.
          {loadError ? ` ${loadError}` : ""}
        </div>
      )}

      {renderError !== null && (
        <div className="error">
          <div>This diagram could not be drawn.</div>
          <div className="mono" style={{ fontSize: 12, marginTop: 4 }}>
            {renderError}
          </div>
        </div>
      )}

      <div
        ref={hostRef}
        className="cb-diagram-host"
        style={{
          flex: 1,
          minHeight: 0,
          cursor: drawn ? "grab" : "default",
          touchAction: "none",
        }}
        onPointerDown={onPointerDown}
        onPointerMove={onPointerMove}
        onPointerUp={endDrag}
        onPointerCancel={endDrag}
      />

      {phase === "loading" && (
        <div className="empty">
          <span className="spinner" style={{ display: "inline-block", marginRight: 8 }} />
          Loading the diagram renderer…
        </div>
      )}

      {phase === "ready" && source.trim() === "" && (
        <div className="empty">There is nothing to draw yet.</div>
      )}

      {/* The warnings are the requirement, not the nicety: everything the
          deriver found and refused to draw reaches a person only here. They sit
          under the picture, always counted, expandable when there are more than
          a glance's worth. */}
      {summary && readable.length > 0 && (
        <div className="warning" style={{ flex: "0 0 auto", maxHeight: 180, overflow: "auto" }}>
          <button
            onClick={() => setWarningsOpen((open) => !open)}
            style={{ background: "transparent", border: "none", padding: 0, cursor: "pointer" }}
          >
            {warningsOpen ? "▾" : "▸"} {summary}
          </button>
          {warningsOpen && (
            <ul style={{ margin: "6px 0 0", paddingLeft: 20 }}>
              {readable.map((warning, index) => (
                <li key={`${index}:${warning}`} style={{ fontSize: 12, marginTop: 2 }}>
                  {warning}
                </li>
              ))}
            </ul>
          )}
        </div>
      )}
    </div>
  );
}

// ---------------------------------------------------------------------------
// DOM plumbing
// ---------------------------------------------------------------------------

/**
 * Put the rendered SVG in the host and give it the one `<g>` we transform.
 *
 * Mermaid's output has no single wrapper we can own — the top level is a
 * handful of siblings including its `<style>` — so everything is moved into a
 * group we create. Moving the style element with the rest is deliberate: it is
 * scoped to the diagram's id and must travel with it.
 *
 * The `viewBox` and the intrinsic `width`/`height` are removed so that one user
 * unit is one pixel and the transform is the only mapping in play; the CSS
 * above sizes the element to the host instead.
 */
function mount(
  host: HTMLElement,
  svg: string,
): { svg: SVGSVGElement; pan: SVGGElement } | null {
  host.innerHTML = svg;
  const element = host.querySelector("svg");
  if (!element) return null;

  element.removeAttribute("viewBox");
  element.removeAttribute("width");
  element.removeAttribute("height");
  element.removeAttribute("style");

  const pan = document.createElementNS(SVG_NS, "g");
  while (element.firstChild) pan.appendChild(element.firstChild);
  element.appendChild(pan);
  return { svg: element, pan };
}

/**
 * Where the ink is, in content coordinates.
 *
 * `getBBox` throws on an element that has not been laid out — a hidden tab, the
 * first paint — and an empty box is a value {@link fit} already knows how to
 * decline, so the failure is answered with one rather than propagated.
 */
function contentBox(pan: SVGGElement): { x: number; y: number; width: number; height: number } {
  try {
    const box = pan.getBBox();
    return { x: box.x, y: box.y, width: box.width, height: box.height };
  } catch {
    return { x: 0, y: 0, width: 0, height: 0 };
  }
}

/**
 * Mark the boxes that lead somewhere, and return where each one leads.
 *
 * The attribute is what the CSS above keys the cursor and the hover outline
 * off, so marking and resolving are the same pass and cannot disagree — a box
 * that looks clickable and is not would be this component telling a lie the
 * whole feature is built to avoid.
 *
 * A `null` graph marks nothing, and so does a missing render id: both mean
 * there is nothing to anchor a DOM id against. An authored file's node ids were
 * typed by whoever wrote it and mean nothing to the graph lookup; matching them
 * against the symbol index is a different function (`targetsForAuthored`),
 * which nothing calls yet — so today a saved diagram has no clickable boxes.
 */
function markOpenable(
  svg: SVGSVGElement,
  graph: ArchGraph | null,
  renderId: string | null,
): Map<string, Target> {
  if (!graph || renderId === null) return new Map();
  const boxes = [...svg.querySelectorAll(BOX_SELECTOR)];
  const targets = targetsByDomId(
    renderId,
    boxes.map((box) => box.id),
    graph,
  );
  for (const box of boxes) {
    if (targets.has(box.id)) box.setAttribute(OPENABLE_ATTR, "");
  }
  return targets;
}

/** Whatever a thrown value has to say, without assuming it is an `Error`. */
function messageOf(error: unknown): string {
  if (error instanceof Error && error.message.trim() !== "") return error.message;
  if (typeof error === "string" && error.trim() !== "") return error;
  return String(error);
}

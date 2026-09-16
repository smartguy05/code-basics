import {
  useCallback,
  useEffect,
  useMemo,
  useRef,
  useState,
  type PointerEvent as ReactPointerEvent,
} from "react";
import type { ArchGraph } from "../../ipc/types";
import { onAppearanceChange } from "../../appearance";
import { derivationLabel, warningSummary } from "./architectureLogic";
import { categoryOf, edgeStyle, legendEntries } from "./graphStyleLogic";
import { layout } from "./layoutLogic";
import { targetFor, type Target } from "./nodeTargets";
import { fit, IDENTITY, panBy, zoomAt, type View } from "./panZoomLogic";

/**
 * A derived architecture graph drawn as a top-down hierarchy of glowing,
 * category-coloured circles on the dark canvas — the bespoke renderer that
 * replaces Mermaid for the two built-in maps (Project map, Component map).
 *
 * This exists *beside* {@link ./DiagramCanvas.DiagramCanvas}, not instead of
 * it. A built-in map arrives as a structured {@link ArchGraph} (nodes, edges,
 * kinds, paths), which is everything this renderer needs; a saved or
 * agent-authored diagram arrives as Mermaid text with no graph behind it, and
 * `ArchitectureView` keeps sending those to the Mermaid canvas. So this file
 * never sees a `null` graph and never parses Mermaid.
 *
 * # A rendering shell, like its sibling
 *
 * Every decision is imported and tested elsewhere: the placement is
 * {@link layout} (`layoutLogic`), the colours and the legend are
 * `graphStyleLogic`, the pan/zoom arithmetic is `panZoomLogic`, where a click
 * goes is {@link targetFor} (`nodeTargets`), and both captions are
 * `architectureLogic`. What is left here is DOM plumbing the vitest suite (node
 * environment, no jsdom) cannot reach and therefore must not contain a
 * decision.
 *
 * # Two things carried over verbatim from DiagramCanvas
 *
 * **The click is read at `pointerdown`, not on `click`.** `onPointerDown` takes
 * pointer capture so a drag that leaves the window still pans, and Chromium
 * then retargets the compatibility `click` to the capture element — the host
 * `<div>` — so `event.target` on the click is never the circle. The openable
 * node under the cursor is therefore resolved while the press still names it
 * and stashed in {@link pressedBoxRef}. This was established the hard way in the
 * Mermaid canvas and applies identically here.
 *
 * **Colours are resolved from the live theme, never written as literals.**
 * `graphStyleLogic` names CSS custom properties; this reads their current
 * values with `getComputedStyle` and re-reads them on `onAppearanceChange`, so
 * the graph recolours when the user switches theme. An SVG filter needs a real
 * colour rather than a `var(...)`, which is the one reason literals appear at
 * all — and they are the theme's own values, read at render, not constants.
 */

export interface GraphCanvasProps {
  /** The structured graph to draw. Never `null`: saved diagrams use Mermaid. */
  graph: ArchGraph;
  warnings: string[];
  onOpenNode: (target: { path: string; line?: number }) => void;
  onError: (message: string | null) => void;
  /** Where the diagram came from, for the badge. `graph.derivation` when omitted. */
  derivation?: Parameters<typeof derivationLabel>[0] | null;
  edited?: boolean;
  /** Where this diagram was last looked at; `null`/omitted means "fit it". */
  initialView?: View | null;
  onViewChange?: (view: View) => void;
}

/** Room left around the drawing when fitting, in screen pixels. */
const FIT_PADDING = 32;
/** One notch of the zoom buttons, as a synthetic wheel delta. */
const BUTTON_ZOOM_DELTA = 200;
/** How far the pointer may travel and still count as a click, in pixels. */
const CLICK_SLOP = 4;
/** How long the view must sit still before it is reported, in milliseconds. */
const VIEW_SETTLE_MS = 250;

/** The circle radius, in content units. */
const NODE_RADIUS = 22;
/** The bright inner dot's radius. */
const INNER_RADIUS = 6;

/** Marks a node group so a click can find which node was pressed. */
const NODE_ATTR = "data-cb-node";
/** Marks the nodes that resolved to a file. Only these get a cursor and a hover. */
const OPENABLE_ATTR = "data-cb-open";

/** Every CSS variable this renderer reads, resolved once per theme. */
const READ_VARS = [
  "--accent",
  "--accent-dim",
  "--pass",
  "--fail",
  "--skip",
  "--syntax-keyword",
  "--syntax-type",
  "--syntax-string",
  "--syntax-property",
  "--text",
  "--text-dim",
  "--border-strong",
] as const;

type Colors = Record<string, string>;

/**
 * Cursor and hover for the nodes that resolved, and the glow for every one.
 *
 * Written only against the marking attributes, so it cannot reach any other SVG
 * in the app. Inline because a graph is built as React elements and shares no
 * class with `styles.css`.
 */
const CANVAS_CSS = `
.cb-graph-host { position: relative; overflow: hidden; }
.cb-graph-host svg { display: block; width: 100%; height: 100%; }
.cb-graph-host [${OPENABLE_ATTR}] { cursor: pointer; }
.cb-graph-host [${OPENABLE_ATTR}]:hover .cb-node-ring { stroke-width: 4px; }
.cb-graph-host text { user-select: none; }
.cb-graph-legend {
  position: absolute; top: 10px; left: 12px; display: flex; flex-wrap: wrap;
  gap: 6px 14px; padding: 6px 10px; border-radius: 8px;
  background: color-mix(in srgb, var(--bg-inset) 82%, transparent);
  border: 1px solid var(--border); pointer-events: none;
}
.cb-graph-legend .chip { display: inline-flex; align-items: center; gap: 6px; font-size: 11px; color: var(--text-dim); }
.cb-graph-legend .dot { width: 9px; height: 9px; border-radius: 50%; box-shadow: 0 0 5px currentColor; }
`;

/** Read the palette for the current theme. */
function readColors(): Colors {
  const style = getComputedStyle(document.documentElement);
  const colors: Colors = {};
  for (const name of READ_VARS) colors[name] = style.getPropertyValue(name).trim();
  return colors;
}

export function GraphCanvas({
  graph,
  warnings,
  onOpenNode,
  onError,
  derivation,
  edited = false,
  initialView = null,
  onViewChange,
}: GraphCanvasProps) {
  const hostRef = useRef<HTMLDivElement | null>(null);
  const panRef = useRef<SVGGElement | null>(null);
  const draggingRef = useRef<{ pointer: number; x: number; y: number } | null>(null);
  const movedRef = useRef(false);
  const pressedBoxRef = useRef<string | null>(null);

  const [view, setView] = useState<View>(initialView ?? IDENTITY);
  const restoringRef = useRef(initialView !== null);
  const [warningsOpen, setWarningsOpen] = useState(false);
  const [colors, setColors] = useState<Colors>(() => readColors());

  // Callbacks held in refs so a fresh closure each render does not re-run the
  // effects that depend on them (which would reset the user's pan and zoom).
  const onErrorRef = useRef(onError);
  const onOpenNodeRef = useRef(onOpenNode);
  const onViewChangeRef = useRef(onViewChange);
  useEffect(() => {
    onErrorRef.current = onError;
    onOpenNodeRef.current = onOpenNode;
    onViewChangeRef.current = onViewChange;
  }, [onError, onOpenNode, onViewChange]);

  // This renderer cannot fail the way Mermaid can, so it clears any error the
  // previous renderer left when it takes over a diagram.
  useEffect(() => {
    onErrorRef.current(null);
  }, [graph]);

  // Recolour when the theme changes.
  useEffect(() => onAppearanceChange(() => setColors(readColors())), []);

  const placed = useMemo(
    () => layout(graph.nodes, graph.edges),
    [graph],
  );

  // Where each node leads, decided once from the graph. A node with no path
  // (an external, a data store, a solution folder) is absent from the map and
  // therefore not clickable — the same abstain rule the Mermaid canvas obeys.
  const targets = useMemo(() => {
    const map = new Map<string, Target>();
    for (const node of graph.nodes) {
      const target = targetFor(node.id, graph);
      if (target) map.set(node.id, target);
    }
    return map;
  }, [graph]);

  // ---- fitting ------------------------------------------------------------

  const fitToHost = useCallback(() => {
    const host = hostRef.current;
    if (!host) return;
    setView(
      fit(placed.bounds, { width: host.clientWidth, height: host.clientHeight }, FIT_PADDING),
    );
  }, [placed]);

  // Fit on first mount and when the graph changes, unless a stored view is
  // being restored (which the first render must not overwrite).
  useEffect(() => {
    if (restoringRef.current) {
      restoringRef.current = false;
      return;
    }
    fitToHost();
  }, [fitToHost]);

  // ---- report the settled view -------------------------------------------

  useEffect(() => {
    if (!onViewChangeRef.current) return;
    const timer = setTimeout(() => onViewChangeRef.current?.(view), VIEW_SETTLE_MS);
    return () => clearTimeout(timer);
  }, [view]);

  // ---- one transform, written straight to the DOM -------------------------

  useEffect(() => {
    panRef.current?.setAttribute(
      "transform",
      `translate(${view.x} ${view.y}) scale(${view.k})`,
    );
  });

  // ---- wheel (native, non-passive so preventDefault works) ----------------

  useEffect(() => {
    const host = hostRef.current;
    if (!host) return;
    const onWheel = (event: WheelEvent) => {
      event.preventDefault();
      const rect = host.getBoundingClientRect();
      setView((current) =>
        zoomAt(current, { x: event.clientX - rect.left, y: event.clientY - rect.top }, event.deltaY),
      );
    };
    host.addEventListener("wheel", onWheel, { passive: false });
    return () => host.removeEventListener("wheel", onWheel);
  }, []);

  // ---- clicking (delegated, capture-aware) --------------------------------

  useEffect(() => {
    const host = hostRef.current;
    if (!host) return;
    const onClick = () => {
      const id = pressedBoxRef.current;
      pressedBoxRef.current = null;
      if (movedRef.current || id === null) return;
      const destination = targets.get(id);
      if (destination) onOpenNodeRef.current(destination);
    };
    host.addEventListener("click", onClick);
    return () => host.removeEventListener("click", onClick);
  }, [targets]);

  // ---- dragging -----------------------------------------------------------

  const onPointerDown = useCallback((event: ReactPointerEvent<HTMLDivElement>) => {
    if (event.button !== 0) return;
    const target = event.target;
    // Read the pressed node before setPointerCapture retargets the click.
    pressedBoxRef.current =
      target instanceof Element
        ? (target.closest(`[${OPENABLE_ATTR}]`)?.getAttribute(NODE_ATTR) ?? null)
        : null;
    draggingRef.current = { pointer: event.pointerId, x: event.clientX, y: event.clientY };
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

  // ---- labels & derived rendering data ------------------------------------

  const origin = derivation ?? graph.derivation;
  const badge = derivationLabel(origin, edited);
  const summary = warningSummary(warnings);
  const readable = warnings.filter((warning) => warning.trim() !== "");
  const legend = legendEntries(graph.nodes);

  const nodeById = useMemo(
    () => new Map(graph.nodes.map((node) => [node.id, node])),
    [graph],
  );

  return (
    <div className="main diagram-canvas">
      <style>{CANVAS_CSS}</style>

      <div className="toolbar">
        <button data-command="architecture.zoom-out" onClick={() => zoomFromCentre(BUTTON_ZOOM_DELTA)}>
          −
        </button>
        <button data-command="architecture.zoom-in" onClick={() => zoomFromCentre(-BUTTON_ZOOM_DELTA)}>
          +
        </button>
        <span className="muted mono" style={{ fontSize: 11, minWidth: 38 }}>
          {Math.round(view.k * 100)}%
        </span>
        <button data-command="architecture.fit" onClick={fitToHost}>
          Fit
        </button>
        <button data-command="architecture.actual-size" onClick={() => setView(IDENTITY)}>
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

      <div
        ref={hostRef}
        className="cb-graph-host"
        style={{
          flex: 1,
          minHeight: 0,
          background: "var(--bg-inset)",
          cursor: "grab",
          touchAction: "none",
        }}
        onPointerDown={onPointerDown}
        onPointerMove={onPointerMove}
        onPointerUp={endDrag}
        onPointerCancel={endDrag}
      >
        {legend.length > 0 && (
          <div className="cb-graph-legend">
            {legend.map((entry) => (
              <span className="chip" key={entry.label}>
                <span className="dot" style={{ background: colors[entry.colorVar], color: colors[entry.colorVar] }} />
                {entry.label}
              </span>
            ))}
          </div>
        )}

        <svg width="100%" height="100%">
          <g ref={panRef}>
            {/* Edges first, so the nodes sit on top of their own arrows. */}
            {graph.edges.map((edge, index) => {
              const from = placed.positions.get(edge.from);
              const to = placed.positions.get(edge.to);
              if (!from || !to) return null;
              const style = edgeStyle(edge.kind);
              const midY = (from.y + to.y) / 2;
              return (
                <path
                  key={`${edge.from}->${edge.to}:${edge.kind}:${index}`}
                  d={`M ${from.x} ${from.y} C ${from.x} ${midY} ${to.x} ${midY} ${to.x} ${to.y}`}
                  fill="none"
                  stroke={colors[style.colorVar]}
                  strokeWidth={1.6}
                  strokeOpacity={0.55}
                  strokeDasharray={style.dashed ? "5 5" : undefined}
                />
              );
            })}

            {[...placed.positions].map(([id, position]) => {
              const node = nodeById.get(id);
              if (!node) return null;
              const color = colors[categoryOf(node).colorVar];
              const openable = targets.has(id);
              return (
                <g
                  key={id}
                  transform={`translate(${position.x} ${position.y})`}
                  {...{ [NODE_ATTR]: id }}
                  {...(openable ? { [OPENABLE_ATTR]: "" } : {})}
                >
                  <g style={{ filter: `drop-shadow(0 0 5px ${color})` }}>
                    <circle
                      className="cb-node-ring"
                      r={NODE_RADIUS}
                      fill={color}
                      fillOpacity={0.16}
                      stroke={color}
                      strokeWidth={2.5}
                    />
                    <circle r={INNER_RADIUS} fill={color} />
                  </g>
                  <text
                    x={0}
                    y={NODE_RADIUS + 16}
                    textAnchor="middle"
                    fontSize={12}
                    fill={colors["--text"]}
                  >
                    {node.label}
                  </text>
                </g>
              );
            })}
          </g>
        </svg>
      </div>

      {/* The warnings are the requirement, not the nicety: everything the
          deriver refused to draw reaches a person here, always counted. */}
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

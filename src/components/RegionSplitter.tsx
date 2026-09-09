import { useRef } from "react";
import { resizeRegion, type Edge } from "./regionLayoutLogic";

/**
 * The draggable divider between the center and one docked region. Pure pointer
 * drag modelled on RunView's `startSplitDrag`; the fraction arithmetic (and its
 * clamp) lives in `regionLayoutLogic.resizeRegion`. `containerRef` is the region
 * host, whose width/height is the axis the fraction is measured against.
 */
export function RegionSplitter({
  edge,
  size,
  onResize,
  containerRef,
}: {
  edge: Edge;
  size: number;
  onResize: (fraction: number) => void;
  containerRef: React.RefObject<HTMLElement | null>;
}) {
  const horizontal = edge === "left" || edge === "right";
  const start = useRef<{ pos: number; fraction: number } | null>(null);

  function onPointerDown(e: React.PointerEvent<HTMLDivElement>) {
    if (e.button !== 0) return;
    e.preventDefault();
    (e.currentTarget as HTMLElement).setPointerCapture(e.pointerId);
    start.current = { pos: horizontal ? e.clientX : e.clientY, fraction: size };
  }
  function onPointerMove(e: React.PointerEvent<HTMLDivElement>) {
    if (!start.current) return;
    const host = containerRef.current;
    if (!host) return;
    const rect = host.getBoundingClientRect();
    const container = horizontal ? rect.width : rect.height;
    const delta = (horizontal ? e.clientX : e.clientY) - start.current.pos;
    onResize(resizeRegion(edge, start.current.fraction, delta, container));
  }
  function onPointerUp(e: React.PointerEvent<HTMLDivElement>) {
    start.current = null;
    (e.currentTarget as HTMLElement).releasePointerCapture(e.pointerId);
  }

  return (
    <div
      className={`region-splitter region-splitter-${horizontal ? "v" : "h"}`}
      onPointerDown={onPointerDown}
      onPointerMove={onPointerMove}
      onPointerUp={onPointerUp}
      title="Drag to resize"
    />
  );
}

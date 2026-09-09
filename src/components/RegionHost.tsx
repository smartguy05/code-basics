import { useEffect, useRef, type ReactNode } from "react";
import { RegionDropOverlay } from "./RegionDropOverlay";
import { RegionSplitter } from "./RegionSplitter";
import { slotKey, useRegions } from "./RegionContext";
import { EDGES, type Dockable, type Edge, type Region } from "./regionLayoutLogic";

/**
 * Lays the workspace out as a center (its `children`) surrounded by up to four
 * docked regions, each resizable by a splitter. It renders **empty slots** only;
 * the dockables' actual content is portaled into those slots by their owners
 * (RunView for editors/diffs, WorkspaceTab for terminals). When nothing is
 * docked it is a transparent pass-through, so the workspace looks exactly as it
 * did before docking existed.
 */
export function RegionHost({ children }: { children: ReactNode }) {
  const regions = useRegions();
  const hostRef = useRef<HTMLDivElement | null>(null);

  // Publish the host element for the drag hit-test and the splitters.
  useEffect(() => {
    if (regions) regions.hostRef.current = hostRef.current;
  });

  if (!regions) return <>{children}</>;
  const { layout } = regions;

  const middle = (
    <div className="region-middle">
      {edgeRegion("left")}
      {layout.left && (
        <RegionSplitter
          edge="left"
          size={layout.left.size}
          onResize={(f) => regions.resize("left", f)}
          containerRef={hostRef}
        />
      )}
      <div className="region-center">{children}</div>
      {layout.right && (
        <RegionSplitter
          edge="right"
          size={layout.right.size}
          onResize={(f) => regions.resize("right", f)}
          containerRef={hostRef}
        />
      )}
      {edgeRegion("right")}
    </div>
  );

  return (
    <div className="region-host" ref={hostRef}>
      {edgeRegion("top")}
      {layout.top && (
        <RegionSplitter
          edge="top"
          size={layout.top.size}
          onResize={(f) => regions.resize("top", f)}
          containerRef={hostRef}
        />
      )}
      {middle}
      {layout.bottom && (
        <RegionSplitter
          edge="bottom"
          size={layout.bottom.size}
          onResize={(f) => regions.resize("bottom", f)}
          containerRef={hostRef}
        />
      )}
      {edgeRegion("bottom")}
      <RegionDropOverlay />
    </div>
  );

  function edgeRegion(edge: Edge) {
    const region = layout[edge];
    if (!region) return null;
    const horizontal = edge === "left" || edge === "right";
    const style = horizontal
      ? { flex: `0 0 ${(region.size * 100).toFixed(2)}%` }
      : { flex: `0 0 ${(region.size * 100).toFixed(2)}%` };
    return (
      <div className={`region region-${edge}`} style={style}>
        <RegionTabs edge={edge} region={region} />
        <div className="region-body">
          {region.items.map((item, i) => (
            <RegionSlot key={slotKey(item)} dockable={item} hidden={i !== region.active} />
          ))}
        </div>
      </div>
    );
  }
}

/** The tab strip for one region. Reads each item's label/close from the meta
 *  registry, so it does not know editors from terminals. Dragging a tab re-docks
 *  it (drag to center to undock). */
function RegionTabs({ edge, region }: { edge: Edge; region: Region }) {
  const regions = useRegions()!;
  return (
    <div className="region-tabs">
      {region.items.map((item, i) => {
        const meta = regions.meta(slotKey(item));
        return (
          <div
            key={slotKey(item)}
            className={`region-tab ${i === region.active ? "active" : ""}`}
            onPointerDown={(e) => {
              // A press that becomes a drag re-docks; a plain click (no move)
              // still selects, because the click handler below fires on pointerup
              // without an intervening drag.
              regions.startDrag(item, e);
            }}
            onClick={() => regions.activate(edge, i)}
            title={meta?.label ?? item.id}
          >
            <span className="region-tab-label">{meta?.label ?? item.id}</span>
            {meta?.onClose && (
              <button
                className="region-tab-close"
                onPointerDown={(e) => e.stopPropagation()}
                onClick={(e) => {
                  e.stopPropagation();
                  meta.onClose?.();
                }}
                title="Close"
              >
                ×
              </button>
            )}
          </div>
        );
      })}
    </div>
  );
}

/** One item's slot: an empty div registered under the dockable's key for its
 *  owner to portal content into. Hidden (not unmounted) when it is not the
 *  region's active item, so the portaled editor/terminal keeps its state. */
function RegionSlot({ dockable, hidden }: { dockable: Dockable; hidden: boolean }) {
  const regions = useRegions()!;
  const ref = useRef<HTMLDivElement | null>(null);
  const key = slotKey(dockable);
  useEffect(() => {
    regions.registerSlot(key, ref.current);
    return () => regions.registerSlot(key, null);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [key]);
  return <div className="region-slot" ref={ref} hidden={hidden} />;
}

export { EDGES };

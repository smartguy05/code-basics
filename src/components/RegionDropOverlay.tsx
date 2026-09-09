import { useRegions } from "./RegionContext";
import { EDGE_BAND, type DropZone } from "./regionLayoutLogic";

/**
 * The edge highlight shown while a dockable is being dragged. Mounted only
 * during a drag (returns null otherwise, so it costs nothing at rest) and sits
 * above the region host. The band it paints is the same `EDGE_BAND` fraction
 * `edgeHitTest` uses, so what the user sees is exactly where a drop would land.
 */
export function RegionDropOverlay() {
  const regions = useRegions();
  if (!regions || !regions.drag) return null;
  const zone = regions.drag.zone;
  if (zone === "center") {
    // A faint center highlight communicates "let go here to undock / do
    // nothing", distinct from an edge.
    return <div className="region-drop region-drop-center" />;
  }
  return <div className={`region-drop region-drop-${zone}`} style={bandStyle(zone)} />;
}

function bandStyle(zone: Exclude<DropZone, "center">): React.CSSProperties {
  const pct = `${Math.round(EDGE_BAND * 100)}%`;
  switch (zone) {
    case "left":
      return { left: 0, top: 0, bottom: 0, width: pct };
    case "right":
      return { right: 0, top: 0, bottom: 0, width: pct };
    case "top":
      return { left: 0, right: 0, top: 0, height: pct };
    case "bottom":
      return { left: 0, right: 0, bottom: 0, height: pct };
  }
}

import { useEffect, useState, type ReactNode } from "react";
import { createPortal } from "react-dom";
import { slotKey, useRegions } from "./RegionContext";
import type { Dockable } from "./regionLayoutLogic";

/**
 * Hosts one editor or diff tab so it can float in the center or dock in a
 * region, **without remounting** its `children`. It always renders `children`
 * through a portal — into the region slot when docked, otherwise into a center
 * host it owns — so a `FileEditor` (whose buffer is only saved on Ctrl+S) keeps
 * its unsaved edits, language-server document and scroll across a dock/undock.
 *
 * The center host is a plain div inside the editor area; when this tab is not
 * the active center tab it is `display:none` (mounted, so the portaled editor
 * survives a tab switch — exactly the old behaviour). While docked, the center
 * host is hidden and the region slot shows the content.
 */
export function DockableEditorSlot({
  tabId,
  label,
  centerActive,
  centerHost = true,
  onClose,
  children,
}: {
  tabId: string;
  /** The region tab strip label while docked. */
  label: string;
  /** Whether this is the active tab in the center strip (ignored while docked). */
  centerActive: boolean;
  /**
   * Whether to own a center host. True for file editors (the center shows them
   * here). False for diffs, whose center rendering is the one shared `DiffPane`
   * elsewhere — so this only portals them into a region slot while docked and
   * renders nothing otherwise.
   */
  centerHost?: boolean;
  onClose: () => void;
  children: ReactNode;
}) {
  const regions = useRegions();
  const [centerNode, setCenterNode] = useState<HTMLElement | null>(null);
  const dockable: Dockable = { kind: "tab", id: tabId };
  const key = slotKey(dockable);
  const isDocked = regions?.dockedTabs.has(tabId) ?? false;
  const slot = isDocked ? (regions?.slot(key) ?? null) : null;
  const showDocked = isDocked && slot !== null;

  useEffect(() => {
    if (!regions || !isDocked) return;
    regions.registerMeta(key, { label, onClose });
    return () => regions.registerMeta(key, null);
  }, [regions, isDocked, key, label, onClose]);

  const target = showDocked ? slot : centerHost ? centerNode : null;
  return (
    <>
      {centerHost && (
        <div
          ref={setCenterNode}
          style={{
            display: !showDocked && centerActive ? "block" : "none",
            height: "100%",
          }}
        />
      )}
      {target && createPortal(children, target)}
    </>
  );
}

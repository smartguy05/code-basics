import { useEffect, useRef, useState, type ReactNode } from "react";
import { useOccluder } from "./occlusionContext";

/**
 * A floating right-click menu: a click-catching backdrop, and a panel at the
 * pointer.
 *
 * The shape was already in the codebase three times over (`ChangesView`,
 * `OutputConsole`, `BranchMenu`) and this is the fourth and fifth use, so it is
 * a component now. It carries no menu content of its own — items are ordinary
 * `<div className="dropdown-item">` children, exactly as the hand-rolled copies
 * wrote them, so the existing styling in `styles.css` applies unchanged.
 *
 * Two things it adds over the copies, both of which they get wrong today:
 * **Escape closes it**, and it is **kept on screen** — a menu opened near the
 * right or bottom edge of the window is shifted back into view rather than
 * being clipped by the viewport, which is where a right-click most often lands
 * in a narrow sidebar.
 */
export function ContextMenu({
  x,
  y,
  zIndex = 46,
  elevated = false,
  onClose,
  children,
}: {
  x: number;
  y: number;
  /** Stacking level; overlay-hosted menus must sit above their host overlay. */
  zIndex?: number;
  /**
   * Raise this menu above the floating panels.
   *
   * The default 46 sits *below* the panel band (`--z-panel` is 60, and a
   * raised terminal is higher still). That is right for a menu opened from
   * inside a view — it shares a plane with the thing it acts on — and wrong
   * for one opened from the **titlebar**, which is above everything: at 46
   * the menu hid behind any open terminal, and so did its click-catching
   * backdrop, so clicking that terminal neither closed the menu nor was
   * intercepted.
   *
   * A class rather than a number, because the bands live in `styles.css`
   * and no z-index integer is written in TypeScript. The inline `zIndex` is
   * omitted when this is set, or it would win over the class.
   */
  elevated?: boolean;
  onClose: () => void;
  children: ReactNode;
}) {
  // While a menu is open, the embedded browser page (an OS surface above the DOM)
  // must hide, or it paints over the menu. Mounted only while open, so this is a
  // plain acquire-on-mount.
  useOccluder(true);

  const panel = useRef<HTMLDivElement>(null);
  /**
   * The correction applied after measuring, `null` until then.
   *
   * The menu is rendered at the pointer first and moved on the next frame,
   * because its size is not knowable until it is in the document — there is no
   * way to measure a menu that has not been drawn.
   */
  const [shift, setShift] = useState<{ dx: number; dy: number } | null>(null);

  useEffect(() => {
    const element = panel.current;
    if (!element) return;
    const box = element.getBoundingClientRect();
    const margin = 4;
    const dx = Math.min(0, window.innerWidth - margin - (x + box.width));
    const dy = Math.min(0, window.innerHeight - margin - (y + box.height));
    setShift({ dx, dy });
  }, [x, y]);

  useEffect(() => {
    function onKey(event: KeyboardEvent) {
      if (event.key === "Escape") {
        event.preventDefault();
        onClose();
      }
    }
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [onClose]);

  return (
    <>
      <div
        className={`dropdown-backdrop${elevated ? " context-menu-elevated-backdrop" : ""}`}
        style={elevated ? undefined : { zIndex: zIndex - 1 }}
        onClick={onClose}
        // A right-click on the backdrop closes the menu rather than opening the
        // webview's own; without this the two menus stack.
        onContextMenu={(e) => {
          e.preventDefault();
          onClose();
        }}
      />
      <div
        ref={panel}
        className={`dropdown-menu${elevated ? " context-menu-elevated" : ""}`}
        style={{
          position: "fixed",
          left: x + (shift?.dx ?? 0),
          top: y + (shift?.dy ?? 0),
          ...(elevated ? {} : { zIndex }),
          // Invisible for the one frame between being drawn and being measured,
          // so a menu near an edge is never seen in the wrong place first.
          visibility: shift === null ? "hidden" : "visible",
        }}
      >
        {children}
      </div>
    </>
  );
}

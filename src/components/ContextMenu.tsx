import { useEffect, useRef, useState, type ReactNode } from "react";

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
  onClose,
  children,
}: {
  x: number;
  y: number;
  onClose: () => void;
  children: ReactNode;
}) {
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
        className="dropdown-backdrop"
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
        className="dropdown-menu"
        style={{
          position: "fixed",
          left: x + (shift?.dx ?? 0),
          top: y + (shift?.dy ?? 0),
          zIndex: 46,
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

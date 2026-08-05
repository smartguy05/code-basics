import { useState, type ReactNode } from "react";

const WIDTH_KEY = "code-basics.sidebarWidth";
const MIN = 180;
const MAX = 600;

const clamp = (value: number) => Math.min(MAX, Math.max(MIN, value));

/**
 * The left column of a view, resizable by dragging its right edge. All views
 * share one stored width, so the layout does not jump between tabs.
 */
export function Sidebar({
  children,
  className,
}: {
  children: ReactNode;
  className?: string;
}) {
  const [width, setWidth] = useState(() => {
    const stored = Number(localStorage.getItem(WIDTH_KEY));
    return Number.isFinite(stored) && stored > 0 ? clamp(stored) : 280;
  });

  function startDrag(event: React.MouseEvent) {
    event.preventDefault();
    const startX = event.clientX;
    const startWidth = width;

    const onMove = (move: MouseEvent) => {
      setWidth(clamp(startWidth + move.clientX - startX));
    };
    const onUp = (up: MouseEvent) => {
      window.removeEventListener("mousemove", onMove);
      window.removeEventListener("mouseup", onUp);
      localStorage.setItem(WIDTH_KEY, String(clamp(startWidth + up.clientX - startX)));
    };
    window.addEventListener("mousemove", onMove);
    window.addEventListener("mouseup", onUp);
  }

  return (
    <>
      <div className={`sidebar ${className ?? ""}`} style={{ width }}>
        {children}
      </div>
      <div className="sidebar-resizer" onMouseDown={startDrag} title="Drag to resize" />
    </>
  );
}

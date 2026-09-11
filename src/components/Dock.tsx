//! The shared minimized-window dock: one fixed strip that renders every
//! minimized panel's pill. Rendered once by `App`. It decides nothing — the
//! ordering and scoping are `dockLogic.visibleEntries`; this only paints the
//! result and calls each entry's `onRestore` on click.
//!
//! No-overlap is structural: the pills are flex children of one strip, so they
//! lay out side by side however many there are, replacing the old per-panel fixed
//! corners that collided (Notes over Review) or scattered (SQL/Browser bottom-left,
//! terminals cascading upward).

import { visibleEntries } from "./dockLogic";
import type { LiveDockEntry } from "./DockContext";

export function Dock({
  entries,
  activeRoot,
}: {
  entries: LiveDockEntry[];
  activeRoot: string | null;
}) {
  const shown = visibleEntries(entries, activeRoot);
  if (shown.length === 0) return null;
  return (
    <div className="dock" role="toolbar" aria-label="Minimized windows">
      {shown.map((entry) => (
        <button
          key={entry.id}
          className={`review-pill dock-pill${entry.attention ? " attention" : ""}`}
          onClick={entry.onRestore}
          title={pillTitle(entry)}
          // The tint is dropped while flashing for attention, matching the old
          // terminal pill: the flash keyframes own the background transiently.
          style={entry.color && !entry.attention ? { background: entry.color } : undefined}
        >
          {entry.spinner && <span className="review-spinner" aria-hidden />}
          <span className="dock-pill-label">{entry.label}</span>
        </button>
      ))}
    </div>
  );
}

/** The pill's tooltip: its label, plus its status or an attention note. */
function pillTitle(entry: LiveDockEntry): string {
  if (entry.attention) return `${entry.label} — needs attention`;
  if (entry.status) return `${entry.label} — ${entry.status}`;
  return `Restore ${entry.label}`;
}

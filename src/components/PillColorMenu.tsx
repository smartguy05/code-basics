import { useState } from "react";

/**
 * A tiny colour-swatch popover for a floating panel's minimized pill — used by
 * both {@link TerminalPanel} and {@link NotesPanel} so a terminal or the Notes
 * bar can be tinted and told apart at a glance.
 *
 * Modelled on `RunConfigMenu`'s self-contained `.dropdown` (an `open` flag plus
 * a backdrop that closes it), because there is no shared popover primitive in
 * the app. It offers a fixed set of theme-friendly presets plus "Default", which
 * clears the colour back to the theme (`undefined`). Deliberately not a native
 * `<input type="color">`: a small preset set reads at a glance and keeps the two
 * pills visually consistent.
 */

/** The preset swatches, chosen to sit legibly on the dark pill background. */
export const PILL_COLORS: { label: string; value: string }[] = [
  { label: "Slate", value: "#3b4a5a" },
  { label: "Blue", value: "#2b4c7e" },
  { label: "Teal", value: "#1f5a52" },
  { label: "Green", value: "#2f5d34" },
  { label: "Amber", value: "#7a4b00" },
  { label: "Red", value: "#7a2f2f" },
  { label: "Purple", value: "#4b2f6e" },
  { label: "Pink", value: "#6e2f57" },
];

export function PillColorMenu({
  color,
  onPick,
  title = "Set pill colour",
}: {
  /** The current colour, or `undefined` for the theme default. */
  color: string | undefined;
  /** Chosen colour, or `undefined` to clear back to the theme default. */
  onPick: (color: string | undefined) => void;
  title?: string;
}) {
  const [open, setOpen] = useState(false);

  return (
    <div className="dropdown pill-color-menu">
      <button
        className="pill-color-dot"
        title={title}
        onClick={() => setOpen((was) => !was)}
        // The trigger itself previews the current colour.
        style={color ? { background: color } : undefined}
      >
        <span aria-hidden>◆</span>
      </button>

      {open && (
        <>
          <div className="dropdown-backdrop" onClick={() => setOpen(false)} />
          <div className="dropdown-menu pill-color-swatches">
            <button
              className={`pill-color-swatch default${color === undefined ? " selected" : ""}`}
              title="Default (theme colour)"
              onClick={() => {
                onPick(undefined);
                setOpen(false);
              }}
            >
              ✕
            </button>
            {PILL_COLORS.map((c) => (
              <button
                key={c.value}
                className={`pill-color-swatch${color === c.value ? " selected" : ""}`}
                title={c.label}
                style={{ background: c.value }}
                onClick={() => {
                  onPick(c.value);
                  setOpen(false);
                }}
              />
            ))}
          </div>
        </>
      )}
    </div>
  );
}

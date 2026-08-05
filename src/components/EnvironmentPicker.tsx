import { useState } from "react";

/** The environment picker's saved state: the option list plus the selection. */
export interface EnvironmentState {
  options: string[];
  selected: string;
}

/**
 * A dropdown for choosing the ASPNETCORE_ENVIRONMENT of the next run.
 *
 * A custom menu rather than a `<select>` so managing the list lives *inside*
 * it: every option carries its own remove ×, and the last row is a free-text
 * input that adds a new option. "(config default)" applies no override.
 */
export function EnvironmentPicker({
  state,
  onChange,
}: {
  state: EnvironmentState;
  onChange: (next: EnvironmentState) => void;
}) {
  const [open, setOpen] = useState(false);
  const [draft, setDraft] = useState("");

  function select(value: string) {
    onChange({ ...state, selected: value });
    setOpen(false);
  }

  function add(name: string) {
    const trimmed = name.trim();
    setDraft("");
    if (!trimmed) return;
    onChange({
      options: state.options.includes(trimmed)
        ? state.options
        : [...state.options, trimmed],
      selected: trimmed,
    });
    setOpen(false);
  }

  function remove(option: string) {
    const options = state.options.filter((o) => o !== option);
    onChange({
      options,
      selected: state.selected === option ? (options[0] ?? "") : state.selected,
    });
  }

  return (
    <div className="dropdown">
      <button
        onClick={() => setOpen((o) => !o)}
        title="Sets ASPNETCORE_ENVIRONMENT for this run"
      >
        Env: {state.selected || "(config default)"} ▾
      </button>

      {open && (
        <>
          <div className="dropdown-backdrop" onClick={() => setOpen(false)} />
          <div className="dropdown-menu">
            <div
              className={`dropdown-item ${state.selected === "" ? "selected" : ""}`}
              onClick={() => select("")}
            >
              <span style={{ flex: 1 }}>(config default)</span>
            </div>

            {state.options.map((option) => (
              <div
                key={option}
                className={`dropdown-item ${state.selected === option ? "selected" : ""}`}
                onClick={() => select(option)}
              >
                <span style={{ flex: 1 }}>{option}</span>
                <span
                  className="remove"
                  role="button"
                  title={`Remove ${option}`}
                  onClick={(e) => {
                    e.stopPropagation();
                    remove(option);
                  }}
                >
                  ×
                </span>
              </div>
            ))}

            <input
              placeholder="Add environment…"
              value={draft}
              onChange={(e) => setDraft(e.target.value)}
              onKeyDown={(e) => {
                if (e.key === "Enter") add(draft);
                if (e.key === "Escape") setOpen(false);
              }}
            />
          </div>
        </>
      )}
    </div>
  );
}

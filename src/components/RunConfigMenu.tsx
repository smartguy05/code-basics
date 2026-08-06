import { useEffect, useState } from "react";
import { createPortal } from "react-dom";
import type { RunConfig } from "../ipc/types";

/**
 * The titlebar's run-configuration dropdown, next to the branch widget.
 *
 * Rendered by the Run view (which owns the selection, status and process
 * state) but displayed in the titlebar via a portal into `#run-config-slot`,
 * so the state survives tab switches without being lifted out of RunView.
 */
export function RunConfigMenu({
  configs,
  selectedId,
  favorites,
  dotClass,
  canMove,
  groupLabel,
  onSelect,
  onToggleFavorite,
  onMove,
  onNew,
  onImport,
}: {
  configs: RunConfig[];
  selectedId: string | null;
  favorites: Set<string>;
  /** Status-dot class for a config: grey idle, yellow busy, green up, red failed. */
  dotClass: (config: RunConfig) => string;
  /** Whether the config has a neighbour `delta` away within its group. */
  canMove: (config: RunConfig, delta: -1 | 1) => boolean;
  /**
   * Which solution a config's project belongs to, shown beside its name.
   *
   * A label rather than a grouped tree on purpose: the list order is the
   * user's own (favourites first, then their saved ordering), and grouping
   * would have to fight it.
   */
  groupLabel?: (config: RunConfig) => string | null;
  onSelect: (config: RunConfig) => void;
  onToggleFavorite: (config: RunConfig) => void;
  onMove: (config: RunConfig, delta: -1 | 1) => void;
  onNew: () => void;
  onImport: () => void;
}) {
  const [open, setOpen] = useState(false);
  const [slot, setSlot] = useState<HTMLElement | null>(null);

  // The slot is rendered by App in the same tree; it exists by effect time.
  useEffect(() => {
    setSlot(document.getElementById("run-config-slot"));
  }, []);

  if (!slot) return null;

  const selected = configs.find((c) => c.id === selectedId) ?? null;

  return createPortal(
    <div className="dropdown">
      <button
        onClick={() => setOpen((was) => !was)}
        title="Run configurations — select, reorder, create, import"
      >
        {selected && <span className={`dot ${dotClass(selected)}`} />}
        <span className="config-name">
          {selected?.name ?? "No configuration"}
        </span>
        {" ▾"}
      </button>

      {open && (
        <>
          <div className="dropdown-backdrop" onClick={() => setOpen(false)} />
          <div className="dropdown-menu" style={{ minWidth: 280 }}>
            {configs.length === 0 && (
              <div className="dropdown-item muted">Nothing runnable was detected.</div>
            )}

            {configs.map((config) => (
              <div
                key={config.id}
                className={`dropdown-item ${config.id === selectedId ? "selected" : ""}`}
                onClick={() => {
                  onSelect(config);
                  setOpen(false);
                }}
              >
                <span className={`dot ${dotClass(config)}`} />
                <span style={{ flex: 1, overflow: "hidden", textOverflow: "ellipsis" }}>
                  {config.name}
                </span>
                {groupLabel?.(config) && (
                  <span className="badge" title="Solution this project belongs to">
                    {groupLabel(config)}
                  </span>
                )}
                {config.source !== "detected" && (
                  <span className="badge">
                    {config.source === "riderImport" ? "rider" : "custom"}
                  </span>
                )}
                {canMove(config, -1) && (
                  <span
                    className="row-action"
                    role="button"
                    title="Move up in the list"
                    onClick={(e) => {
                      e.stopPropagation();
                      onMove(config, -1);
                    }}
                  >
                    ↑
                  </span>
                )}
                {canMove(config, 1) && (
                  <span
                    className="row-action"
                    role="button"
                    title="Move down in the list"
                    onClick={(e) => {
                      e.stopPropagation();
                      onMove(config, 1);
                    }}
                  >
                    ↓
                  </span>
                )}
                <span
                  className={`star ${favorites.has(config.id) ? "active" : ""}`}
                  role="button"
                  title={
                    favorites.has(config.id)
                      ? "Remove from favourites"
                      : "Add to favourites"
                  }
                  onClick={(e) => {
                    e.stopPropagation();
                    onToggleFavorite(config);
                  }}
                >
                  {favorites.has(config.id) ? "★" : "☆"}
                </span>
              </div>
            ))}

            <div className="dropdown-separator" />

            <div
              className="dropdown-item"
              onClick={() => {
                onNew();
                setOpen(false);
              }}
            >
              + New configuration…
            </div>
            <div
              className="dropdown-item"
              onClick={() => {
                onImport();
                setOpen(false);
              }}
            >
              Import from Rider…
            </div>
          </div>
        </>
      )}
    </div>,
    slot,
  );
}

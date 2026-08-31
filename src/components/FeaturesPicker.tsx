import { useEffect, useState } from "react";
import * as api from "../ipc/api";
import type { FeatureInfo } from "../ipc/types";

/**
 * The optional-features picker: one checkbox per feature this build ships.
 *
 * A rendering shell — every decision (what the defaults are, what an unknown id
 * means, what a switched-off feature hides) lives in `cb_core::features` and
 * `featuresLogic.ts`.
 *
 * The same choices a Windows installer offers, reachable afterwards. That is not
 * a convenience: a `.deb` cannot ask the question at all, and neither can an
 * AppImage or a `cargo run`, so this dialog — not the installer — is the surface
 * every platform has.
 *
 * Writes go straight through on each toggle and the component re-renders from
 * what the backend returned, so what is on screen is what is on disk. There is
 * no OK/Cancel because there is nothing buffered to cancel.
 */
export function FeaturesPicker({
  features,
  onChange,
  onClose,
}: {
  /** The current set, or `null` while the startup load is still in flight. */
  features: FeatureInfo[] | null;
  onChange: (features: FeatureInfo[]) => void;
  onClose: () => void;
}) {
  const [busy, setBusy] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    const onKey = (event: KeyboardEvent) => {
      if (event.key === "Escape") onClose();
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [onClose]);

  const toggle = async (feature: FeatureInfo) => {
    setBusy(feature.id);
    setError(null);
    try {
      onChange(await api.setFeature(feature.id, !feature.enabled));
    } catch (e) {
      // Report the failure and leave the checkbox where it was: showing it
      // flipped would claim a write that did not happen.
      setError(String(e));
    } finally {
      setBusy(null);
    }
  };

  return (
    <div className="launcher-overlay" onClick={onClose}>
      <div className="features-picker" onClick={(e) => e.stopPropagation()}>
        <div className="features-header">
          <h2>Optional features</h2>
          <button className="icon-button" onClick={onClose} title="Close">
            ×
          </button>
        </div>

        <p className="features-intro">
          Turn parts of the app on or off. Everything is installed either way —
          this only decides what is shown.
        </p>

        {features === null ? (
          <p className="features-empty">Loading…</p>
        ) : (
          <ul className="features-list">
            {features.map((feature) => (
              <li key={feature.id} className="features-row">
                <label>
                  <input
                    type="checkbox"
                    checked={feature.enabled}
                    disabled={busy !== null}
                    onChange={() => void toggle(feature)}
                  />
                  <span className="features-label">{feature.label}</span>
                </label>
                <div className="features-description">{feature.description}</div>
              </li>
            ))}
          </ul>
        )}

        {error && <div className="features-error">{error}</div>}
      </div>
    </div>
  );
}

import { useEffect, useState } from "react";
import * as api from "../ipc/api";
import type { RiderImportPreview, Workspace } from "../ipc/types";

/**
 * Review step for a Rider import.
 *
 * JetBrains publishes no schema for `.run` XML, so nothing is written until
 * the user has seen what the conversion produced — including the warnings
 * attached to configurations that only partly translated.
 */
export function RiderImportDialog({
  onClose,
  onImported,
}: {
  onClose: () => void;
  onImported: (workspace: Workspace) => void;
}) {
  const [preview, setPreview] = useState<RiderImportPreview | null>(null);
  const [accepted, setAccepted] = useState<Set<string>>(new Set());
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);

  useEffect(() => {
    api
      .previewRiderImport()
      .then((result) => {
        setPreview(result);
        // Everything is accepted by default; the user opts out.
        setAccepted(new Set(result.configs.map((c) => c.id)));
      })
      .catch((e) => setError(api.errorMessage(e)));
  }, []);

  async function apply() {
    if (!preview) return;
    setBusy(true);
    try {
      const chosen = preview.configs.filter((c) => accepted.has(c.id));
      onImported(await api.applyRiderImport(chosen));
    } catch (e) {
      setError(api.errorMessage(e));
    } finally {
      setBusy(false);
    }
  }

  return (
    <div className="modal-backdrop" onClick={onClose}>
      <div className="modal" onClick={(e) => e.stopPropagation()}>
        <header>Import run configurations from Rider</header>

        <div className="modal-body">
          {error && <div className="error">{error}</div>}

          {!preview && !error && <div className="muted">Reading Rider configurations…</div>}

          {preview && preview.configs.length === 0 && (
            <div className="muted">
              No importable configurations were found in <code>.run/</code> or{" "}
              <code>.idea/</code>.
            </div>
          )}

          {preview?.configs.map((config) => (
            <div
              key={config.id}
              style={{
                padding: "8px 0",
                borderBottom: "1px solid var(--border)",
              }}
            >
              <label style={{ display: "flex", gap: 8, alignItems: "center" }}>
                <input
                  type="checkbox"
                  checked={accepted.has(config.id)}
                  onChange={() =>
                    setAccepted((previous) => {
                      const next = new Set(previous);
                      if (next.has(config.id)) next.delete(config.id);
                      else next.add(config.id);
                      return next;
                    })
                  }
                />
                <span style={{ fontWeight: 600 }}>{config.name}</span>
                <span className="badge">{config.ecosystem}</span>
                <span className="badge">{config.kind}</span>
              </label>

              <div className="muted mono" style={{ fontSize: 11, marginLeft: 24 }}>
                {config.project ?? "(no project)"}
                {config.launchProfile ? ` · profile ${config.launchProfile}` : ""}
                {config.script ? ` · script ${config.script}` : ""}
              </div>

              {config.warnings?.map((warning) => (
                <div
                  className="warning"
                  key={warning}
                  style={{ margin: "6px 0 0 24px" }}
                >
                  {warning}
                </div>
              ))}
            </div>
          ))}

          {preview && preview.skipped.length > 0 && (
            <>
              <div className="group-label">Not imported</div>
              {preview.skipped.map(([name, kind]) => (
                <div key={`${name}:${kind}`} className="muted" style={{ fontSize: 12 }}>
                  {name} — <span className="mono">{kind}</span> configurations cannot be
                  launched from this app.
                </div>
              ))}
            </>
          )}
        </div>

        <footer>
          <button onClick={onClose}>Cancel</button>
          <button
            className="primary"
            onClick={apply}
            disabled={busy || accepted.size === 0}
          >
            Import {accepted.size > 0 ? `(${accepted.size})` : ""}
          </button>
        </footer>
      </div>
    </div>
  );
}

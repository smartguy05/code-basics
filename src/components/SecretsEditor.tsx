import { useEffect, useState } from "react";
import * as api from "../ipc/api";

/**
 * Edit a .NET project's user secrets, the way Rider's "Manage .NET User
 * Secrets" does.
 *
 * The secrets live in a `secrets.json` under the user profile — never in the
 * repository — keyed by the project's `<UserSecretsId>`. Saving a project that
 * has no id yet adds one to the `.csproj` first, exactly like
 * `dotnet user-secrets init`.
 */
export function SecretsEditor({
  project,
  projectName,
  onClose,
}: {
  /** Workspace-relative path to the `.csproj`, from `RunConfig.project`. */
  project: string;
  projectName: string;
  onClose: () => void;
}) {
  const [text, setText] = useState("");
  const [path, setPath] = useState<string | null>(null);
  const [hasId, setHasId] = useState(true);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    let cancelled = false;
    api
      .readProjectSecrets(project)
      .then((secrets) => {
        if (cancelled) return;
        setText(secrets.content ?? "{\n}\n");
        setPath(secrets.path);
        setHasId(secrets.secretsId !== null);
      })
      .catch((e) => !cancelled && setError(api.errorMessage(e)))
      .finally(() => !cancelled && setLoading(false));
    return () => {
      cancelled = true;
    };
  }, [project]);

  async function save() {
    setError(null);
    try {
      await api.writeProjectSecrets(project, text);
      onClose();
    } catch (e) {
      setError(api.errorMessage(e));
    }
  }

  return (
    <div className="modal-backdrop" onClick={onClose}>
      <div className="modal" onClick={(e) => e.stopPropagation()}>
        <header>User secrets — {projectName}</header>

        <div className="modal-body">
          {!hasId && !loading && (
            <div className="warning">
              This project has no UserSecretsId yet. Saving adds one to the
              project file, like <code>dotnet user-secrets init</code>.
            </div>
          )}

          <div className="field">
            <label>secrets.json (kept outside the repository)</label>
            <textarea
              className="mono"
              rows={14}
              value={loading ? "Loading…" : text}
              disabled={loading}
              onChange={(e) => setText(e.target.value)}
              spellCheck={false}
            />
          </div>

          {path && (
            <div className="faint mono" style={{ fontSize: 11 }}>
              {path}
            </div>
          )}

          {error && <div className="error">{error}</div>}
        </div>

        <footer>
          <button onClick={onClose}>Cancel</button>
          <button className="primary" onClick={save} disabled={loading}>
            Save
          </button>
        </footer>
      </div>
    </div>
  );
}

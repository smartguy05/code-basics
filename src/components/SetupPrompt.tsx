import { useState } from "react";
import * as api from "../ipc/api";
import { PlanPreview } from "./PlanPreview";
import type { InstallPlan, InstallScope } from "../ipc/types";

/**
 * First-open setup: a modal shown when a workspace opens without the agent hooks
 * installed, so setup happens once rather than being forgotten. The user picks a
 * scope, sees the exact files that will be written, and confirms — the same
 * preview-before-write contract every other install uses.
 *
 * "Not now" closes for this session; "Don't ask again" persists per workspace
 * (handled by the parent). Nothing is written until a plan is confirmed.
 */
export function SetupPrompt({
  onDismiss,
  onDontAskAgain,
  onInstalled,
}: {
  onDismiss: () => void;
  onDontAskAgain: () => void;
  onInstalled: () => void;
}) {
  const [plan, setPlan] = useState<InstallPlan | null>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const preview = async (scope: InstallScope) => {
    setBusy(true);
    setError(null);
    try {
      setPlan(await api.setupInstallPlan(scope));
    } catch (e) {
      setError(api.errorMessage(e));
    } finally {
      setBusy(false);
    }
  };

  const confirm = async () => {
    if (!plan) return;
    setBusy(true);
    setError(null);
    try {
      await api.installSetup(plan.scope);
      onInstalled();
    } catch (e) {
      setError(api.errorMessage(e));
      setBusy(false);
    }
  };

  return (
    <div className="modal-backdrop" onClick={onDismiss}>
      <div className="modal" onClick={(e) => e.stopPropagation()}>
        <header>
          <strong>Set up agent hooks</strong>
        </header>

        <div className="modal-body">
          <p style={{ fontSize: 13, marginTop: 0 }}>
            This workspace doesn't have the code-basics agent hooks yet. They capture{" "}
            <em>why</em> a coding agent changed code (intent capture), and run a quality
            gate — typecheck / <code>cargo fmt</code> on changed files, and a check for
            unresolved rejection notes — when a turn ends. Setting them up once means it
            isn't forgotten.
          </p>

          {!plan ? (
            <div className="actions">
              <button
                disabled={busy}
                className="primary"
                onClick={() => void preview("project")}
                title="Install into this repository (.claude/settings.json), shared with anyone who clones it"
              >
                For this repo…
              </button>
              <button
                disabled={busy}
                onClick={() => void preview("user")}
                title="Install into your own configuration, covering every repository you open"
              >
                For me…
              </button>
            </div>
          ) : (
            <PlanPreview
              plan={plan}
              busy={busy}
              confirmLabel="Install"
              onConfirm={() => void confirm()}
              onCancel={() => setPlan(null)}
            />
          )}

          {error && (
            <div className="error" style={{ fontSize: 11, marginTop: 6 }}>
              {error}
            </div>
          )}
        </div>

        <footer>
          <button disabled={busy} onClick={onDontAskAgain}>
            Don't ask again
          </button>
          <button disabled={busy} onClick={onDismiss}>
            Not now
          </button>
        </footer>
      </div>
    </div>
  );
}

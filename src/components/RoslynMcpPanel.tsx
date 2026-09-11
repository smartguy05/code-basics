import { useEffect, useState } from "react";
import * as api from "../ipc/api";
import { PlanPreview } from "./PlanPreview";
import {
  MCP_PROVIDERS,
  confirmLabel,
  defaultScope,
  planOutcome,
  providerLabel,
  scopeOptions,
  statusText,
  writtenNote,
  type McpMode,
  type McpProvider,
} from "./mcpServerLogic";
import type { InstallPlan, InstallScope } from "../ipc/types";

/**
 * Installing the **Roslyn / LSP** MCP server into a coding agent's configuration.
 *
 * # Everything reusable is reused
 *
 * The provider list, the scope options, the status wording, the confirm label,
 * the zero-write answer and the written note are all `mcpServerLogic` — the same
 * module the SQL and browser servers' panels use. The preview is the shared
 * {@link PlanPreview}, which renders the whole final contents of every file,
 * because these are files a team shares. And the plan itself comes from
 * `cb_core::roslyn::install`, which reuses `mcp::install`'s merge rather than
 * growing a second one.
 *
 * What is deliberately **not** shared is the description and the caveats. What an
 * agent gains here is read-only *code intelligence* over one repository — the
 * app's already-warm Roslyn/LSP session answering `find_references`,
 * `get_diagnostics`, `get_type_hierarchy` and `resolve_overloads` — which is a
 * different thing to reason about than a database login or a live browser
 * session, so it is stated on screen rather than behind a shared parameter.
 *
 * # Installing grants nothing on its own, and the boundary is the repository
 *
 * This writes a configuration file. A project-scope entry names *this* workspace
 * as the consent boundary: an agent configured for it reaches only this repo's
 * warm session, regardless of which window is focused. The app must be running —
 * which it is by construction, since the agent runs inside one of its terminals.
 */
export function RoslynMcpPanel({ onClose }: { onClose: () => void }) {
  const [provider, setProvider] = useState<McpProvider>("claudeCode");
  const [scope, setScope] = useState<InstallScope>(defaultScope("claudeCode"));
  /** `undefined` while the status read is in flight — distinct from "not installed". */
  const [status, setStatus] = useState<InstallScope | null | undefined>(undefined);
  const [pending, setPending] = useState<{ plan: InstallPlan; mode: McpMode } | null>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  /**
   * What the last write actually did. A cleared preview is indistinguishable
   * from a cancel, so without this a successful install reads as "nothing
   * happened" — and the natural response to that is to install a second time.
   */
  const [written, setWritten] = useState<string | null>(null);

  // Switching agent re-reads the status and drops any preview: a plan is for one
  // provider and one scope, and confirming it under a different heading would
  // write the file the user is no longer looking at.
  useEffect(() => {
    let live = true;
    setStatus(undefined);
    setPending(null);
    setWritten(null);
    setScope(defaultScope(provider));
    void api
      .roslynMcpStatus(provider)
      .then((s) => live && setStatus(s))
      .catch((e) => {
        if (!live) return;
        setStatus(null);
        setError(api.errorMessage(e));
      });
    return () => {
      live = false;
    };
  }, [provider]);

  const preview = async (mode: McpMode) => {
    setError(null);
    setWritten(null);
    try {
      const plan =
        mode === "install"
          ? await api.roslynMcpInstallPlan(provider, scope)
          : await api.roslynMcpUninstallPlan(provider, scope);
      setPending({ plan, mode });
    } catch (e) {
      setPending(null);
      setError(api.errorMessage(e));
    }
  };

  const confirm = async () => {
    if (!pending) return;
    setBusy(true);
    setError(null);
    try {
      const { plan, mode } = pending;
      setStatus(
        mode === "install"
          ? await api.installRoslynMcp(provider, plan.scope)
          : await api.uninstallRoslynMcp(provider, plan.scope),
      );
      // The paths come from the plan the user just approved, not from a second
      // read, so the sentence names exactly the files that were shown.
      setWritten(
        writtenNote(
          mode,
          provider,
          plan.scope,
          plan.writes.map((w) => w.path),
        ),
      );
      setPending(null);
    } catch (e) {
      setError(api.errorMessage(e));
    } finally {
      setBusy(false);
    }
  };

  const outcome = pending ? planOutcome(pending.plan, pending.mode, "Roslyn MCP server") : null;

  return (
    <div className="launcher-overlay" onMouseDown={onClose}>
      <div className="ask-panel" onMouseDown={(e) => e.stopPropagation()}>
        <div className="ask-header">
          <h2>Roslyn MCP server</h2>
          <button className="ask-close" title="Close" onClick={onClose}>
            ✕
          </button>
        </div>

        <div className="faint" style={{ fontSize: 11, marginBottom: 6 }}>
          Gives a coding agent this application&rsquo;s warm language-server answers over MCP —
          find references, diagnostics, type hierarchy and overloads — for this repository. Every
          call is read-only, resolved against the session already loaded for the editor, and reaches
          only the repository this entry is scoped to.
        </div>

        {/* `.segmented` rather than `.actions`: a plain button has no active
            state of its own, so the chosen agent would look inert. */}
        <div className="segmented" style={{ padding: 0, marginBottom: 6 }}>
          {MCP_PROVIDERS.map((p) => (
            <button
              key={p}
              className={p === provider ? "active" : ""}
              disabled={busy}
              onClick={() => setProvider(p)}
            >
              {providerLabel(p)}
            </button>
          ))}
        </div>

        <div style={{ fontSize: 12, marginBottom: 6 }}>
          {status === undefined ? "Checking…" : statusText(provider, status)}
        </div>

        {scopeOptions(provider).map((option) => (
          <label
            key={option.scope}
            className={option.available ? "" : "faint"}
            style={{ display: "block", fontSize: 12, marginBottom: 4 }}
          >
            <input
              type="radio"
              name="roslyn-mcp-scope"
              checked={scope === option.scope}
              disabled={!option.available || busy}
              onChange={() => {
                setScope(option.scope);
                setPending(null);
                setWritten(null);
              }}
            />{" "}
            {option.label}
            <div className="faint" style={{ fontSize: 11, marginLeft: 20 }}>
              {option.detail}
            </div>
          </label>
        ))}

        <div className="actions">
          <button disabled={busy} onClick={() => void preview("install")}>
            Install…
          </button>
          <button disabled={busy} onClick={() => void preview("remove")}>
            Remove…
          </button>
        </div>

        {error && <div className="error">{error}</div>}

        {written && (
          <div className="success" style={{ fontSize: 11, marginTop: 8 }}>
            {written}
          </div>
        )}

        {outcome?.kind === "nothing" && (
          <div className="warning" style={{ fontSize: 11, marginTop: 8 }}>
            {outcome.message}
          </div>
        )}

        {pending && outcome?.kind === "confirm" && (
          <PlanPreview
            plan={outcome.plan}
            busy={busy}
            confirmLabel={confirmLabel(pending.mode)}
            onConfirm={() => void confirm()}
            onCancel={() => setPending(null)}
          />
        )}
      </div>
    </div>
  );
}

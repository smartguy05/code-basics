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
 * Installing the **Build & Diagnostics** MCP server into a coding agent's
 * configuration.
 *
 * # The twin of {@link RoslynMcpPanel} and {@link EditorMcpPanel}
 *
 * The provider list, scope options, status wording, confirm label, zero-write
 * answer and written note are all `mcpServerLogic` — the same module every MCP
 * server panel uses. The preview is the shared {@link PlanPreview}, and the plan
 * comes from `cb_core::build::mcp::install`, which reuses `mcp::install`'s merge
 * rather than growing a second one. Only the wrappers differ (`buildMcp*`).
 *
 * # What is deliberately not shared: the description and caveats
 *
 * What an agent gains here is the ability to **build this repository and read the
 * build's diagnostics** — its errors, warnings and status, from the same build
 * the editor runs. That is a different thing to reason about than a database
 * login (SQL), the semantic model (Roslyn) or what the user is looking at
 * (Editor context), so two things are stated on screen rather than behind a
 * shared parameter:
 *
 * - A build **writes compiler output** such as `bin/` and `obj/`, but it changes
 *   no source file and there is no tool here that edits code. Said plainly.
 * - The server answers nothing while the **Build MCP** feature is off, and the
 *   install is scoped to one repository — so installing here is only part of
 *   turning it on.
 */
export function BuildMcpPanel({ onClose }: { onClose: () => void }) {
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
      .buildMcpStatus(provider)
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
          ? await api.buildMcpInstallPlan(provider, scope)
          : await api.buildMcpUninstallPlan(provider, scope);
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
          ? await api.installBuildMcp(provider, plan.scope)
          : await api.uninstallBuildMcp(provider, plan.scope),
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

  const outcome = pending
    ? planOutcome(pending.plan, pending.mode, "Build MCP server")
    : null;

  return (
    <div className="launcher-overlay" onMouseDown={onClose}>
      <div className="ask-panel" onMouseDown={(e) => e.stopPropagation()}>
        <div className="ask-header">
          <h2>Build MCP server</h2>
          <button className="ask-close" title="Close" onClick={onClose}>
            ✕
          </button>
        </div>

        <div className="faint" style={{ fontSize: 11, marginBottom: 6 }}>
          Lets a coding agent build this repository and read the build&rsquo;s diagnostics &mdash;
          its errors, warnings and status, from the same build the editor runs. A build writes
          compiler output such as <code>bin/</code> and <code>obj/</code>, but it changes no source
          file, and there is no tool here that edits code. The server answers only while the
          <strong> Build MCP</strong> feature is switched on.
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
              name="build-mcp-scope"
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

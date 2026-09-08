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
 * Installing the read-only SQL MCP server into a coding agent's configuration.
 *
 * A transient modal over `.launcher-overlay`, like `AskPanel` and
 * `LauncherPicker`: it is opened, a decision is made, and it goes away. There is
 * no live state worth keeping mounted — unlike `SqlPanel`, which owns database
 * connections — so `WorkspaceTab` mounts it only while it is open.
 *
 * # Nothing here decides anything
 *
 * Which file each scope writes, what to warn about, and how an existing
 * configuration is merged into all come back from
 * `cb_core::mcp::install::plan` inside the {@link InstallPlan}, and are rendered
 * verbatim by the shared {@link PlanPreview} — the same preview the hook
 * installers use, for the same reason: these are files a team shares, so the
 * exact final contents are shown before anything is written. What is decided in
 * the frontend at all lives in the pure, tested `mcpServerLogic`.
 *
 * # A removal with nothing to remove is a sentence, not a disabled button
 *
 * `planOutcome` turns a zero-write uninstall plan into a rendered answer. A
 * greyed-out Remove is indistinguishable from one that is busy or broken;
 * "this configuration holds no entry for the server" is something a person can
 * act on.
 */
export function McpServerPanel({ onClose }: { onClose: () => void }) {
  const [provider, setProvider] = useState<McpProvider>("claudeCode");
  const [scope, setScope] = useState<InstallScope>(defaultScope("claudeCode"));
  /** `undefined` while the status read is in flight — distinct from "not installed". */
  const [status, setStatus] = useState<InstallScope | null | undefined>(undefined);
  const [pending, setPending] = useState<{ plan: InstallPlan; mode: McpMode } | null>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  /**
   * What the last write actually did. A cleared preview is indistinguishable
   * from a cancel, so without this a successful install read as "nothing
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
      .mcpServerStatus(provider)
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
          ? await api.mcpServerInstallPlan(provider, scope)
          : await api.mcpServerUninstallPlan(provider, scope);
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
          ? await api.installMcpServer(provider, plan.scope)
          : await api.uninstallMcpServer(provider, plan.scope),
      );
      // The paths come from the plan the user just approved, not from a second
      // read, so the sentence names exactly the files that were shown.
      setWritten(writtenNote(mode, provider, plan.scope, plan.writes.map((w) => w.path)));
      setPending(null);
    } catch (e) {
      setError(api.errorMessage(e));
    } finally {
      setBusy(false);
    }
  };

  const outcome = pending ? planOutcome(pending.plan, pending.mode) : null;

  return (
    <div className="launcher-overlay" onMouseDown={onClose}>
      <div className="ask-panel" onMouseDown={(e) => e.stopPropagation()}>
        <div className="ask-header">
          <h2>SQL MCP server</h2>
          <button className="ask-close" title="Close" onClick={onClose}>
            ✕
          </button>
        </div>

        <div className="faint" style={{ fontSize: 11, marginBottom: 6 }}>
          Points a coding agent at the databases you have switched on for it. Every
          call is read-only, and no connection is reachable until you expose it
          individually in the SQL console.
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

        {/* Undefined is "still reading", which is neither installed nor not —
            saying "not installed" during the read would be a wrong answer shown
            confidently, and it is the answer someone would act on. */}
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
              name="mcp-scope"
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

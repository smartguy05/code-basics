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
 * Installing the browser MCP server into a coding agent's configuration.
 *
 * # Everything reusable is reused
 *
 * The provider list, the scope options, the status wording, the confirm label,
 * the zero-write answer and the written note are all `mcpServerLogic` — the same
 * module the SQL server's panel uses, unchanged apart from one sentence that
 * named a particular server and now takes it as an argument. The preview is the
 * shared {@link PlanPreview}, which renders the whole final contents of every
 * file, because these are files a team shares. And the plan itself comes from
 * `cb_core::browser::install`, which reuses `mcp::install`'s merge rather than
 * growing a second one.
 *
 * What is deliberately **not** shared is the description and the caveats. What
 * an agent gains here is the page the user is looking at, in their own
 * logged-in session; the SQL server's warnings are about a database login. A
 * shared component with a discriminator would have put the choice of which
 * warnings a person sees behind a parameter, and the failure mode is showing
 * somebody the wrong one before they click Install.
 *
 * # Installing grants nothing, and the panel says so first
 *
 * This writes a configuration file. It does not make any page readable: every
 * read and every state-changing call needs the user's permission for the page
 * currently open, granted in the browser panel's own banner and forgotten when
 * the page changes. That is the first thing on screen rather than a footnote,
 * because "I installed it, so it can see my browser" is the wrong model to
 * leave someone with.
 */
export function BrowserMcpPanel({ onClose }: { onClose: () => void }) {
  const [provider, setProvider] = useState<McpProvider>("claudeCode");
  const [scope, setScope] = useState<InstallScope>(defaultScope("claudeCode"));
  /** `undefined` is *still reading*, which is neither installed nor not. */
  const [status, setStatus] = useState<InstallScope | null | undefined>(undefined);
  const [pending, setPending] = useState<{ plan: InstallPlan; mode: McpMode } | null>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  /**
   * What the last write actually did. A preview disappearing looks exactly
   * like a cancel, so without this a successful install reads as "nothing
   * happened" — and the natural response to that is to install again.
   */
  const [written, setWritten] = useState<string | null>(null);

  // Switching agent re-reads the status and drops any preview: a plan is for
  // one provider and one scope, and confirming it under a different heading
  // would write the file the user is no longer looking at.
  useEffect(() => {
    let live = true;
    setStatus(undefined);
    setPending(null);
    setWritten(null);
    setScope(defaultScope(provider));
    void api
      .browserMcpStatus(provider)
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
          ? await api.browserMcpInstallPlan(provider, scope)
          : await api.browserMcpUninstallPlan(provider, scope);
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
          ? await api.installBrowserMcp(provider, plan.scope)
          : await api.uninstallBrowserMcp(provider, plan.scope),
      );
      // From the plan the user just approved, not from a second read, so the
      // sentence names exactly the files that were shown.
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

  const outcome = pending ? planOutcome(pending.plan, pending.mode, "browser MCP server") : null;

  return (
    <div className="launcher-overlay" onMouseDown={onClose}>
      <div className="ask-panel" onMouseDown={(e) => e.stopPropagation()}>
        <div className="ask-header">
          <h2>Browser MCP server</h2>
          <button className="ask-close" title="Close" onClick={onClose}>
            ✕
          </button>
        </div>

        <div className="faint" style={{ fontSize: 11, marginBottom: 6 }}>
          Lets a coding agent see the page in this application&rsquo;s browser panel — its address,
          its text, its elements, its console — and, with a second permission, click and type in it.
          Installing this grants nothing on its own: every call needs your permission for the page
          that is open at the time, asked for in the panel and forgotten when the page changes.
          There is no tool that runs JavaScript in the page.
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
              name="browser-mcp-scope"
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

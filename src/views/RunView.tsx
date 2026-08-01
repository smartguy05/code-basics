import { useEffect, useRef, useState } from "react";
import { OutputConsole, type ConsoleHandle } from "../components/OutputConsole";
import { ConfigEditor } from "../components/ConfigEditor";
import { RiderImportDialog } from "../components/RiderImportDialog";
import * as api from "../ipc/api";
import type { RunConfig, Workspace } from "../ipc/types";

export function RunView({
  workspace,
  onWorkspaceChange,
}: {
  workspace: Workspace;
  onWorkspaceChange: (workspace: Workspace) => void;
}) {
  const appConfigs = workspace.configs.filter((c) => c.kind === "app");

  const [selectedId, setSelectedId] = useState<string | null>(
    appConfigs[0]?.id ?? null,
  );
  const [running, setRunning] = useState<Set<string>>(new Set());
  const [error, setError] = useState<string | null>(null);
  const [editing, setEditing] = useState<RunConfig | null>(null);
  const [importing, setImporting] = useState(false);

  const consoleRef = useRef<ConsoleHandle>(null);
  const selected = appConfigs.find((c) => c.id === selectedId) ?? null;

  // Processes outlive a view switch, so reconcile on mount rather than
  // assuming nothing is running.
  useEffect(() => {
    api
      .runningIds()
      .then((ids) => setRunning(new Set(ids)))
      .catch(() => {
        /* nothing running */
      });
  }, []);

  async function start(config: RunConfig) {
    setError(null);
    setSelectedId(config.id);
    consoleRef.current?.clear();
    setRunning((previous) => new Set(previous).add(config.id));

    try {
      await api.startRun(config.id, (event) => consoleRef.current?.handle(event));
    } catch (e) {
      setError(api.errorMessage(e));
    } finally {
      setRunning((previous) => {
        const next = new Set(previous);
        next.delete(config.id);
        return next;
      });
    }
  }

  async function stop(config: RunConfig) {
    await api.cancelRun(config.id);
  }

  async function save(config: RunConfig) {
    try {
      onWorkspaceChange(await api.saveConfig(config));
      setEditing(null);
    } catch (e) {
      setError(api.errorMessage(e));
    }
  }

  async function remove(config: RunConfig) {
    try {
      onWorkspaceChange(await api.deleteConfig(config.id));
    } catch (e) {
      setError(api.errorMessage(e));
    }
  }

  return (
    <>
      <div className="sidebar">
        <div className="group-label">Run configurations</div>

        {appConfigs.length === 0 && (
          <div className="muted" style={{ padding: 8 }}>
            Nothing runnable was detected.
          </div>
        )}

        {appConfigs.map((config) => (
          <button
            key={config.id}
            className={`row ${config.id === selectedId ? "selected" : ""}`}
            onClick={() => setSelectedId(config.id)}
          >
            {running.has(config.id) ? (
              <span className="spinner" />
            ) : (
              <span className="dot other" />
            )}
            <span style={{ flex: 1, overflow: "hidden", textOverflow: "ellipsis" }}>
              {config.name}
            </span>
            {config.source !== "detected" && (
              <span className="badge">
                {config.source === "riderImport" ? "rider" : "custom"}
              </span>
            )}
          </button>
        ))}

        <div className="group-label">Configuration</div>
        <button
          className="row"
          onClick={() =>
            setEditing({
              id: `custom:${Date.now()}`,
              name: "New configuration",
              kind: "app",
              ecosystem: "dotnet",
              source: "userFile",
            })
          }
        >
          + New configuration
        </button>
        <button className="row" onClick={() => setImporting(true)}>
          Import from Rider…
        </button>
      </div>

      <div className="main">
        <div className="toolbar">
          <button
            className="primary"
            onClick={() => selected && start(selected)}
            disabled={!selected || running.has(selected.id)}
          >
            Run
          </button>
          <button
            onClick={() => selected && stop(selected)}
            disabled={!selected || !running.has(selected.id)}
          >
            Stop
          </button>
          <button
            onClick={() => selected && start(selected)}
            disabled={!selected}
            title="Stop and start again"
          >
            Restart
          </button>
          <button onClick={() => selected && setEditing(selected)} disabled={!selected}>
            Edit
          </button>
          <button
            onClick={() => selected && remove(selected)}
            disabled={!selected || selected.source === "detected"}
            title={
              selected?.source === "detected"
                ? "Detected configurations reappear on the next scan"
                : "Remove this configuration"
            }
          >
            Delete
          </button>

          <span style={{ flex: 1 }} />
          {selected && (
            <span className="muted mono" style={{ fontSize: 11 }}>
              {selected.ecosystem}
              {selected.buildConfiguration ? ` · ${selected.buildConfiguration}` : ""}
              {selected.launchProfile ? ` · ${selected.launchProfile}` : ""}
              {selected.script ? ` · ${selected.script}` : ""}
            </span>
          )}
        </div>

        {error && <div className="error">{error}</div>}
        {selected?.warnings?.map((warning) => (
          <div className="warning" key={warning}>
            {warning}
          </div>
        ))}

        <div className="content">
          <OutputConsole ref={consoleRef} />
        </div>
      </div>

      {editing && (
        <ConfigEditor
          config={editing}
          workspace={workspace}
          onCancel={() => setEditing(null)}
          onSave={save}
        />
      )}

      {importing && (
        <RiderImportDialog
          onClose={() => setImporting(false)}
          onImported={(updated) => {
            onWorkspaceChange(updated);
            setImporting(false);
          }}
        />
      )}
    </>
  );
}

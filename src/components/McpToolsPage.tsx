import { useEffect, useState } from "react";

import * as api from "../ipc/api";
import type { FeatureInfo, McpServerToolsInfo } from "../ipc/types";
import { featureEnabled, type FeatureKey } from "./featuresLogic";
import {
  enabledCount,
  nextServerValue,
  serverToggleState,
  toolBlockedByServer,
  toolKey,
} from "./mcpToolsLogic";

/**
 * Which feature flag gates each MCP server, or `null` for a server with no flag.
 *
 * Per-server enablement stays owned by the features store; this page's per-tool
 * gate ([`cb_core::tool_gate`]) is a separate, finer control. Roslyn has no
 * feature flag today, so its tools show with no server toggle.
 */
const SERVER_FEATURE: Record<string, FeatureKey | null> = {
  sql: "mcpSqlServer",
  tasks: "tasks",
  browser: "webBrowser",
  roslyn: null,
};

export function McpToolsPage() {
  const [servers, setServers] = useState<McpServerToolsInfo[] | null>(null);
  const [features, setFeatures] = useState<FeatureInfo[] | null>(null);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    let live = true;
    void api
      .listMcpTools()
      .then((list) => {
        if (live) setServers(list);
      })
      .catch((e) => {
        if (live) setError(String(e));
      });
    void api
      .listFeatures()
      .then((list) => {
        if (live) setFeatures(list);
      })
      .catch(() => {});
    return () => {
      live = false;
    };
  }, []);

  async function toggleTool(server: string, tool: string, enabled: boolean) {
    try {
      setServers(await api.setMcpTool(server, tool, enabled));
      setError(null);
    } catch (e) {
      // Leave the boxes where they are and say what happened, rather than
      // showing a state that was not persisted.
      setError(String(e));
    }
  }

  async function toggleServerTools(server: McpServerToolsInfo, enabled: boolean) {
    try {
      let latest = servers ?? [];
      for (const tool of server.tools) {
        latest = await api.setMcpTool(server.id, tool.name, enabled);
      }
      setServers(latest);
      setError(null);
    } catch (e) {
      setError(String(e));
    }
  }

  async function toggleFeature(feature: FeatureKey, enabled: boolean) {
    try {
      setFeatures(await api.setFeature(feature, enabled));
      setError(null);
    } catch (e) {
      setError(String(e));
    }
  }

  if (error && servers === null) {
    return <div className="settings-message">Could not load MCP tools: {error}</div>;
  }
  if (servers === null) {
    return <p>Loading MCP tools…</p>;
  }

  return (
    <div className="mcp-tools-page">
      <p>
        Turn individual MCP tools on or off for the agents you point at code-basics.
        A disabled tool is not advertised and is refused if called. An agent already
        running may need to reconnect before the tool list changes; a call is refused
        immediately either way.
      </p>
      {servers.map((server) => {
        const feature = SERVER_FEATURE[server.id] ?? null;
        const serverOn = feature === null ? true : featureEnabled(features, feature);
        const blocked = toolBlockedByServer(feature === null ? true : serverOn);
        const state = serverToggleState(server);
        return (
          <section key={server.id} className="mcp-server">
            <header className="mcp-server-head">
              <h3>{server.label}</h3>
              {feature === null ? (
                <span className="muted">always on</span>
              ) : (
                <label className="mcp-server-toggle">
                  <input
                    type="checkbox"
                    checked={serverOn}
                    onChange={(e) => void toggleFeature(feature, e.target.checked)}
                  />
                  Server enabled
                </label>
              )}
            </header>
            {blocked && (
              <p className="muted">
                This server is switched off, so its tools are not installed. Turn the
                server on to gate its tools individually.
              </p>
            )}
            <div className="mcp-server-actions">
              <button
                disabled={blocked}
                onClick={() => void toggleServerTools(server, nextServerValue(state))}
              >
                {state === "none" ? "Enable all" : "Disable all"}
              </button>
              <span className="muted">
                {enabledCount(server)} of {server.tools.length} enabled
              </span>
            </div>
            <ul className="mcp-tool-list">
              {server.tools.map((tool) => (
                <li key={toolKey(server.id, tool)} className="mcp-tool-row">
                  <label>
                    <input
                      type="checkbox"
                      disabled={blocked}
                      checked={tool.enabled}
                      onChange={(e) =>
                        void toggleTool(server.id, tool.name, e.target.checked)
                      }
                    />
                    <span className="mcp-tool-name">{tool.name}</span>
                  </label>
                  {tool.description && (
                    <small className="mcp-tool-desc">{tool.description}</small>
                  )}
                </li>
              ))}
            </ul>
          </section>
        );
      })}
      {error && <div className="settings-message">{error}</div>}
    </div>
  );
}

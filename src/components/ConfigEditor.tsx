import { useState } from "react";
import type { Project, RunConfig, Workspace } from "../ipc/types";

function envToText(env: Record<string, string> | undefined): string {
  return Object.entries(env ?? {})
    .map(([key, value]) => `${key}=${value}`)
    .join("\n");
}

/**
 * Parse `KEY=value` lines.
 *
 * Splits on the *first* `=` only, so values containing `=` — connection
 * strings, base64, JWTs — survive intact.
 */
function textToEnv(text: string): Record<string, string> {
  const env: Record<string, string> = {};
  for (const line of text.split("\n")) {
    const trimmed = line.trim();
    if (!trimmed || trimmed.startsWith("#")) continue;

    const separator = trimmed.indexOf("=");
    if (separator <= 0) continue;
    env[trimmed.slice(0, separator).trim()] = trimmed.slice(separator + 1).trim();
  }
  return env;
}

/**
 * The value the backend expects in `RunConfig.project`: a path relative to the
 * workspace root.
 *
 * Deliberately *not* `project.id` — ids replace path separators with `-`
 * (`workspace::project_id`), so they never resolve through
 * `workspace::find_project`, which joins the value onto the workspace root.
 *
 * Which path depends on the ecosystem, and it matters: the .NET side reads the
 * value as a file and takes its parent directory, while Node and the
 * declarative adapters treat it as a directory. This mirrors what
 * auto-detection writes.
 */
function projectTarget(project: Project, root: string): string {
  const absolute = project.ecosystem === "dotnet" ? project.manifestPath : project.dir;
  const trimmedRoot = root.replace(/[\\/]+$/, "");

  return absolute.startsWith(trimmedRoot)
    ? absolute.slice(trimmedRoot.length).replace(/^[\\/]+/, "")
    : absolute;
}

export function ConfigEditor({
  config,
  workspace,
  onCancel,
  onSave,
}: {
  config: RunConfig;
  workspace: Workspace;
  onCancel: () => void;
  onSave: (config: RunConfig) => void;
}) {
  const [draft, setDraft] = useState<RunConfig>(config);
  const [envText, setEnvText] = useState(() => envToText(config.env));
  const [argsText, setArgsText] = useState(() => (config.args ?? []).join(" "));

  const update = <K extends keyof RunConfig>(key: K, value: RunConfig[K]) =>
    setDraft((previous) => ({ ...previous, [key]: value }));

  function save() {
    onSave({
      ...draft,
      env: textToEnv(envText),
      // Arguments are stored as a list; splitting on whitespace is enough for
      // the editor, and anything subtler can be set in the config file.
      args: argsText.split(/\s+/).filter(Boolean),
    });
  }

  return (
    <div className="modal-backdrop" onClick={onCancel}>
      <div className="modal" onClick={(e) => e.stopPropagation()}>
        <header>Edit configuration</header>

        <div className="modal-body">
          <div className="field">
            <label>Name</label>
            <input
              value={draft.name}
              onChange={(e) => update("name", e.target.value)}
            />
          </div>

          <div className="field">
            <label>Kind</label>
            <select
              value={draft.kind}
              onChange={(e) => update("kind", e.target.value as RunConfig["kind"])}
            >
              <option value="app">Application</option>
              <option value="test">Tests</option>
            </select>
          </div>

          <div className="field">
            <label>Project</label>
            <select
              value={draft.project ?? ""}
              onChange={(e) => {
                const target = e.target.value || undefined;
                update("project", target);

                // The ecosystem follows the project, since it decides which
                // adapter builds the command.
                const project = workspace.projects.find(
                  (p) => projectTarget(p, workspace.root) === target,
                );
                if (project) update("ecosystem", project.ecosystem);
              }}
            >
              <option value="">(workspace root)</option>
              {workspace.projects.map((project) => (
                <option key={project.id} value={projectTarget(project, workspace.root)}>
                  {project.name} ({project.ecosystem})
                </option>
              ))}
            </select>
          </div>

          {draft.ecosystem === "dotnet" && (
            <>
              <div className="field">
                <label>Build configuration</label>
                <select
                  value={draft.buildConfiguration ?? ""}
                  onChange={(e) =>
                    update("buildConfiguration", e.target.value || undefined)
                  }
                >
                  <option value="">(default)</option>
                  <option value="Debug">Debug</option>
                  <option value="Release">Release</option>
                </select>
              </div>

              <div className="field">
                <label>Target framework</label>
                <input
                  placeholder="net8.0 — leave empty unless multi-targeting"
                  value={draft.framework ?? ""}
                  onChange={(e) => update("framework", e.target.value || undefined)}
                />
              </div>

              <div className="field">
                <label>Launch profile</label>
                <input
                  placeholder="Leave empty to ignore launchSettings.json"
                  value={draft.launchProfile ?? ""}
                  onChange={(e) =>
                    update("launchProfile", e.target.value || undefined)
                  }
                />
              </div>
            </>
          )}

          {draft.ecosystem === "node" && (
            <div className="field">
              <label>Script</label>
              <input
                placeholder="dev"
                value={draft.script ?? ""}
                onChange={(e) => update("script", e.target.value || undefined)}
              />
            </div>
          )}

          <div className="field">
            <label>Arguments</label>
            <input
              className="mono"
              value={argsText}
              onChange={(e) => setArgsText(e.target.value)}
            />
          </div>

          <div className="field">
            <label>Working directory</label>
            <input
              placeholder="Relative to the workspace root"
              value={draft.cwd ?? ""}
              onChange={(e) => update("cwd", e.target.value || undefined)}
            />
          </div>

          <div className="field">
            <label>Environment (one KEY=value per line)</label>
            <textarea
              className="mono"
              rows={5}
              value={envText}
              onChange={(e) => setEnvText(e.target.value)}
            />
          </div>
        </div>

        <footer>
          <button onClick={onCancel}>Cancel</button>
          <button className="primary" onClick={save} disabled={!draft.name.trim()}>
            Save
          </button>
        </footer>
      </div>
    </div>
  );
}

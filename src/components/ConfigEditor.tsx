import { useEffect, useState } from "react";
import * as api from "../ipc/api";
import type { LaunchProfile, RunConfig, Workspace } from "../ipc/types";
import { envToText, projectTarget, textToEnv } from "./configLogic";

export function ConfigEditor({
  config,
  workspace,
  onCancel,
  onSave,
  onDelete,
}: {
  config: RunConfig;
  workspace: Workspace;
  onCancel: () => void;
  onSave: (config: RunConfig) => void;
  /** Delete this configuration. Absent for new or detected configurations. */
  onDelete?: () => void;
}) {
  const [draft, setDraft] = useState<RunConfig>(config);
  const [envText, setEnvText] = useState(() => envToText(config.env));
  const [argsText, setArgsText] = useState(() => (config.args ?? []).join(" "));
  const [profiles, setProfiles] = useState<LaunchProfile[]>([]);

  // The launch profiles the targeted project actually defines. Profiles
  // `dotnet run --launch-profile` cannot apply are listed but disabled, so a
  // project whose only profile is IIS Express explains itself instead of
  // showing an empty dropdown.
  useEffect(() => {
    if (draft.ecosystem !== "dotnet" || !draft.project) {
      setProfiles([]);
      return;
    }
    let cancelled = false;
    api
      .launchProfiles(draft.project)
      .then((found) => !cancelled && setProfiles(found))
      .catch(() => !cancelled && setProfiles([]));
    return () => {
      cancelled = true;
    };
  }, [draft.ecosystem, draft.project]);

  /** The project this configuration targets, when it resolves to one. */
  const target = workspace.projects.find(
    (p) => projectTarget(p, workspace.root) === draft.project,
  );

  // Offer what the project declares, falling back to the MSBuild default pair
  // so the dropdown is never empty for a project we could not read.
  const buildConfigurations =
    target?.configurations?.length ? target.configurations : ["Debug", "Release"];

  const selectedProfile = profiles.find((p) => p.name === draft.launchProfile);

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
                  {buildConfigurations.map((name) => (
                    <option key={name} value={name}>
                      {name}
                    </option>
                  ))}
                  {/* A saved value the project no longer declares still has to
                      be visible, or the select would silently lie about it. */}
                  {draft.buildConfiguration &&
                    !buildConfigurations.includes(draft.buildConfiguration) && (
                      <option value={draft.buildConfiguration}>
                        {draft.buildConfiguration} (not in &lt;Configurations&gt;)
                      </option>
                    )}
                </select>
              </div>

              <div className="field">
                <label>Target framework</label>
                {target && target.frameworks.length > 1 ? (
                  <select
                    value={draft.framework ?? ""}
                    onChange={(e) => update("framework", e.target.value || undefined)}
                  >
                    {/* Multi-targeted projects must choose: `dotnet run`
                        refuses to guess between frameworks. */}
                    <option value="">(choose a framework)</option>
                    {target.frameworks.map((framework) => (
                      <option key={framework} value={framework}>
                        {framework}
                      </option>
                    ))}
                  </select>
                ) : (
                  <input
                    placeholder="net8.0 — leave empty unless multi-targeting"
                    value={draft.framework ?? ""}
                    onChange={(e) => update("framework", e.target.value || undefined)}
                  />
                )}
              </div>

              <div className="field">
                <label>Launch profile</label>
                <select
                  value={draft.launchProfile ?? ""}
                  onChange={(e) =>
                    update("launchProfile", e.target.value || undefined)
                  }
                >
                  <option value="">
                    (default — dotnet run picks the first profile)
                  </option>
                  {profiles.map((profile) => (
                    <option
                      key={profile.name}
                      value={profile.name}
                      disabled={!profile.launchable}
                    >
                      {profile.name}
                      {profile.launchable
                        ? ""
                        : ` — ${profile.commandName ?? "unknown"} profiles cannot be launched here`}
                    </option>
                  ))}
                  {/* A saved value the project no longer defines still has to
                      be visible, or the select would silently lie about it. */}
                  {draft.launchProfile &&
                    !profiles.some((p) => p.name === draft.launchProfile) && (
                      <option value={draft.launchProfile}>
                        {draft.launchProfile} (not found in launchSettings.json)
                      </option>
                    )}
                </select>
              </div>

              {/* What the profile will actually do. These values are applied by
                  `dotnet run` itself, so without showing them the run has
                  invisible environment variables and URLs. */}
              {selectedProfile && (
                <div className="field">
                  <label>Profile applies</label>
                  <div className="mono" style={{ fontSize: "0.85em", opacity: 0.85 }}>
                    {selectedProfile.applicationUrl && (
                      <div>URL: {selectedProfile.applicationUrl}</div>
                    )}
                    {selectedProfile.workingDirectory && (
                      <div>Working directory: {selectedProfile.workingDirectory}</div>
                    )}
                    {selectedProfile.args.length > 0 && (
                      <div>Arguments: {selectedProfile.args.join(" ")}</div>
                    )}
                    {Object.entries(selectedProfile.env).map(([key, value]) => (
                      <div key={key}>
                        {key}={value}
                      </div>
                    ))}
                    {!selectedProfile.applicationUrl &&
                      !selectedProfile.workingDirectory &&
                      selectedProfile.args.length === 0 &&
                      Object.keys(selectedProfile.env).length === 0 && (
                        <div>Nothing beyond the defaults.</div>
                      )}
                  </div>
                </div>
              )}

              {!draft.launchProfile && (
                <div className="field">
                  <label style={{ textTransform: "none", letterSpacing: 0 }}>
                    <input
                      type="checkbox"
                      checked={!!draft.ignoreLaunchSettings}
                      onChange={(e) =>
                        update("ignoreLaunchSettings", e.target.checked || undefined)
                      }
                    />{" "}
                    Ignore launchSettings.json (skips its environment variables
                    and applicationUrl; otherwise <code>dotnet run</code> applies
                    the default profile)
                  </label>
                </div>
              )}
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
          {onDelete && (
            <button
              onClick={onDelete}
              style={{ marginRight: "auto", borderColor: "var(--fail)" }}
              title="Remove this configuration from .code-basics/config.json"
            >
              Delete
            </button>
          )}
          <button onClick={onCancel}>Cancel</button>
          <button className="primary" onClick={save} disabled={!draft.name.trim()}>
            Save
          </button>
        </footer>
      </div>
    </div>
  );
}

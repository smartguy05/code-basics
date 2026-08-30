import type { Project, RunConfig } from "../ipc/types";

export function envToText(env: Record<string, string> | undefined): string {
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
export function textToEnv(text: string): Record<string, string> {
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
export function projectTarget(project: Project, root: string): string {
  const absolute = project.ecosystem === "dotnet" ? project.manifestPath : project.dir;
  const trimmedRoot = root.replace(/[\\/]+$/, "");

  return absolute.startsWith(trimmedRoot)
    ? absolute.slice(trimmedRoot.length).replace(/^[\\/]+/, "")
    : absolute;
}

/**
 * The build configurations the Run toolbar's picker offers for a configuration.
 *
 * Read from the *project*, not from the run configuration, because that is
 * where MSBuild's `<Configurations>` actually lands
 * (`workspace::Project.configurations`) and because detection now produces one
 * run configuration per project rather than one per build configuration — so
 * the run config knows only which one is currently chosen, never the set.
 *
 * Empty is a real answer and the caller must respect it: a Node package, a
 * cargo crate or a declarative adapter has no Debug/Release distinction at all,
 * and offering a made-up pair would put a `-c Release` on a command line that
 * has never accepted one. The default pair is supplied only for a *.NET*
 * project the scan could not read — there the concept exists and only the
 * declaration is missing, which is exactly when a fallback is honest.
 *
 * The same lookup as `ConfigEditor`'s dropdown, which is why it is here rather
 * than inline in either.
 */
export function buildConfigurationsFor(
  config: Pick<RunConfig, "ecosystem" | "project"> | null,
  projects: Project[],
  root: string,
): string[] {
  if (!config || config.ecosystem !== "dotnet") return [];

  const project = projects.find((p) => projectTarget(p, root) === config.project);
  if (project?.configurations?.length) return project.configurations;
  return ["Debug", "Release"];
}

/**
 * Which entry the picker shows: the remembered choice when it is still on
 * offer, else the configuration's own default, else the first option.
 *
 * A remembered choice that is no longer offered is dropped rather than shown.
 * `<Configurations>` can lose a value between scans, and a picker displaying
 * `Staging` for a project that no longer declares it would send a `-c Staging`
 * that fails at build time with an error naming MSBuild rather than the picker.
 */
export function selectedBuildConfiguration(
  options: string[],
  remembered: string | null,
  fallback: string | null | undefined,
): string | null {
  if (options.length === 0) return null;
  if (remembered !== null && options.includes(remembered)) return remembered;
  if (fallback != null && options.includes(fallback)) return fallback;
  return options[0] ?? null;
}

import type { Project } from "../ipc/types";

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

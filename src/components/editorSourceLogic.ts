/**
 * What backs an open editor tab.
 *
 * The app's one real editor (`FileEditor`) was born addressing a single
 * workspace-relative path: it read and wrote through `fs_read_file`/
 * `fs_write_file` and drove the whole language-server surface off that path. A
 * .NET project's user secrets are a real file the user wants to edit the same
 * way, but they live **outside** the workspace (under the user profile, keyed by
 * `<UserSecretsId>`) and are read/written through the dedicated secrets
 * commands. So a tab's backing is no longer "a path" but one of these, and the
 * editor branches its IO — and whether it talks to a language server at all — on
 * the variant.
 */
import type { Project } from "../ipc/types";

export type EditorSource =
  | { kind: "workspace"; path: string }
  | { kind: "secrets"; project: string };

/** An open tab: its identity, its label, and what backs it. */
export interface OpenEditorFile {
  /**
   * The tab's identity — the React key and the key of every per-file map
   * (active, dirty, pinned, reveal). For a workspace file this is its path; for
   * secrets it is `secrets:<project>`, which cannot collide with a
   * workspace-relative path because no such path begins with `secrets:`.
   */
  id: string;
  /** The tab label. */
  name: string;
  source: EditorSource;
}

/** The last path segment, from either separator a path may carry. */
function baseName(path: string): string {
  const parts = path.split(/[\\/]/);
  return parts[parts.length - 1] || path;
}

/** The seed for a secrets file that does not exist on disk yet. */
export const EMPTY_SECRETS = "{\n}\n";

/** An ordinary workspace file, identified and labelled by its path. */
export function workspaceFile(path: string): OpenEditorFile {
  return { id: path, name: baseName(path), source: { kind: "workspace", path } };
}

/** A project's user secrets, opened as a `secrets.json` tab. */
export function secretsFile(project: string): OpenEditorFile {
  return { id: `secrets:${project}`, name: "secrets.json", source: { kind: "secrets", project } };
}

/**
 * The .NET projects whose user secrets can be opened, in scan order.
 *
 * Secrets are a .NET concept, and an unreadable project's manifest never parsed
 * — there is nothing to attach a `<UserSecretsId>` to — so those are excluded.
 * This is what the Run toolbar's Secrets picker lists: **every** .NET project in
 * the workspace, not just the one behind the selected run configuration, so a
 * folder with several projects can reach any of their secrets.
 */
export function secretsProjects(projects: Project[]): Project[] {
  return projects.filter((p) => p.ecosystem === "dotnet" && !p.unreadable);
}

/**
 * Whether this source participates in the language-server surface.
 *
 * Only a workspace file does. A secrets file is JSON that lives outside the
 * workspace and no server knows about it, so the editor skips `didOpen`/usages
 * entirely rather than firing calls that would only ever come back "unavailable".
 */
export function sourceEnablesLsp(source: EditorSource): boolean {
  return source.kind === "workspace";
}

/**
 * The path handed to `languageFor` to pick syntax highlighting.
 *
 * `languageFor` keys off the extension, so a secrets tab reports `secrets.json`
 * and gets JSON highlighting; a workspace file reports its own path.
 */
export function sourceLanguageHint(source: EditorSource): string {
  return source.kind === "secrets" ? "secrets.json" : source.path;
}

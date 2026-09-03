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
 *
 * The `diff` variant arrived with the Project tab: selecting a changed file in
 * the Changes rail opens its diff as **another editor tab** beside the open
 * files rather than replacing the main area, so a diff needs the same identity,
 * label and per-tab state every other tab has.
 */
import type { ComparisonMode, Project } from "../ipc/types";

export type EditorSource =
  | { kind: "workspace"; path: string }
  | { kind: "secrets"; project: string }
  | { kind: "diff"; path: string; mode: ComparisonMode };

/**
 * The sources a text editor can actually open, edit and save.
 *
 * A diff is deliberately **excluded at the type level** rather than guarded at
 * runtime. `FileEditor` dispatches its read and write with two-way ternaries of
 * the shape `kind === "secrets" ? ... : source.path`, and a diff also carries a
 * `path` — so it type-checks, and the else-branch would read the plain file
 * and, on the flush timer or Ctrl+S, **write the diff buffer over the real
 * file**. Nothing would report an error. Narrowing the prop turns that into a
 * compile failure at the one call site that mints tabs, which is where the
 * decision to render a diff differently actually belongs.
 */
export type EditableSource = Exclude<EditorSource, { kind: "diff" }>;

/** An open tab: its identity, its label, and what backs it. */
export interface OpenEditorFile {
  /**
   * The tab's identity — the React key and the key of every per-file map
   * (active, dirty, pinned, reveal). For a workspace file this is its path; for
   * secrets it is `secrets:<project>` and for a diff `diff:<mode>:<path>`.
   *
   * The two prefixes are disjoint from each other, so a secrets tab and a diff
   * tab can never collide. Against a **workspace** id they are disjoint only by
   * convention: a workspace id is the bare relative path, so a file genuinely
   * named `diff:workingToHead:App.tsx` would produce the same string as a diff
   * tab on `App.tsx`. That is not defended here, because the bare-path id is
   * load-bearing elsewhere (callers look a tab up by path), and narrowing it
   * would be a larger change than the hazard warrants. What removes the blast
   * radius instead is that a caller asking "is this path already open?" must
   * match on the **source**, not the id — see {@link sameWorkspaceFile}.
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
 * The short per-mode word a diff tab is labelled with.
 *
 * Deliberately shorter than `ChangesView`'s `MODE_LABELS` ("Unstaged (vs
 * staged)"): that phrasing explains a picker the user is reading, while this has
 * to fit in a tab strip beside a file name. It exists at all because two diffs
 * of the same file differ only by mode, so a bare base name would put two tabs
 * on screen with the same label and no way to tell them apart.
 */
export const DIFF_MODE_LABELS: Record<ComparisonMode, string> = {
  workingToHead: "Diff",
  workingToIndex: "Unstaged",
  indexToHead: "Staged",
};

/**
 * A file's diff at one comparison mode, opened as its own tab.
 *
 * **The mode is part of the identity, not just of the label.** The same file
 * compared against the index and against HEAD are two different things to look
 * at — `workingToIndex` shows what is not staged yet, `indexToHead` what is —
 * and they routinely disagree. If the id ignored the mode, switching the
 * comparison would silently change what an already-open tab means while its
 * label, its scroll position and its place in the strip all claimed it was the
 * same view. Two tabs is the honest answer; one tab that quietly re-aims is the
 * bug.
 */
export function diffFile(path: string, mode: ComparisonMode): OpenEditorFile {
  return {
    id: `diff:${mode}:${path}`,
    name: `${baseName(path)} (${DIFF_MODE_LABELS[mode]})`,
    source: { kind: "diff", path, mode },
  };
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
 *
 * A diff is not an LSP surface either, and for a stronger reason than secrets:
 * its text is not the file's text. Its lines are two revisions interleaved with
 * `+`/`-` markers, so every position the server was handed would name the wrong
 * line — and a usages count or a go-to-definition landing one revision away is
 * worse than none at all. It is also read-only, so there is nothing to `didOpen`
 * a document for.
 */
export function sourceEnablesLsp(source: EditorSource): boolean {
  return source.kind === "workspace";
}

/**
 * Whether opening this tab records a stop in the editor's back/forward stack.
 *
 * Only a workspace file. The stack stores **paths**, and going back to one
 * reopens the file — so a diff would come back as the source file at a mode the
 * stack never recorded, i.e. as a different tab than the one the user left. A
 * diff is also not a place in the code: nothing navigates *to* it (go-to-
 * definition and the search palette both land on a path), it is reached only by
 * clicking a row in the Changes rail, and that row is still there to click
 * again. Secrets are excluded for the matching reason — their id is
 * `secrets:<project>`, which is no path at all.
 *
 * A **type guard**, so the one caller does not have to retype `kind ===
 * "workspace"` alongside it just to reach `.path`. Two conditions meaning the
 * same thing, sitting next to each other, is how the rule drifts: the next
 * source variant would be added to this function and missed at the call site.
 */
export function sourceEntersNavStack(
  source: EditorSource,
): source is Extract<EditorSource, { kind: "workspace" }> {
  return source.kind === "workspace";
}

/**
 * The path handed to `languageFor` to pick syntax highlighting.
 *
 * `languageFor` keys off the extension, so a secrets tab reports `secrets.json`
 * and gets JSON highlighting; a workspace file reports its own path.
 *
 * A diff reports its own path too: the diff panes highlight each side as the
 * language of the file being compared, exactly as the Changes tab already does.
 */
export function sourceLanguageHint(source: EditorSource): string {
  return source.kind === "secrets" ? "secrets.json" : source.path;
}

/**
 * Whether an open tab is the workspace file at `path`.
 *
 * The rule callers need instead of `tab.id === path`. That comparison was
 * correct while every id was either a bare path or `secrets:`-prefixed, and the
 * diff variant breaks it twice over: a diff tab on a file is not the same tab
 * as the file itself (opening the file must not find the diff and stop), and a
 * pathological filename can make the two ids equal outright. Matching the
 * discriminated source answers the question that was actually being asked.
 */
export function sameWorkspaceFile(file: OpenEditorFile, path: string): boolean {
  return file.source.kind === "workspace" && file.source.path === path;
}

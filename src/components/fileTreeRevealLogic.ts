/**
 * Pure decisions behind the file tree's "select opened file" action: which
 * directories have to be opened, and which of those still have to be fetched,
 * before a deeply nested file can be scrolled to.
 *
 * This is a logic module rather than a line inside `FileTree` because the tree
 * is *lazy* — one `fs_list_dir` call per directory, made the first time it is
 * expanded — so revealing `src/views/architecture/DiagramCanvas.tsx` is not one
 * state update but an ordered walk: `src`, then `src/views`, then
 * `src/views/architecture`, each fetch needing its parent's listing to already
 * be present. Getting that order wrong half-expands the tree and looks like a
 * backend failure, and vitest runs in the node environment so the React half
 * cannot be tested at all. The order is the part worth checking.
 *
 * Path spelling matches `fileTreeLogic`: workspace-relative, forward slashes,
 * `""` for the root. The one thing this module tolerates that the rest does not
 * is a backslash — an active path can arrive from the editor, a search jump or
 * a saved session, and on a Windows-first app any of those may spell a
 * separator the way the shell does. Both spellings name the same directory, so
 * folding them is not a guess.
 */

/**
 * Every directory prefix of a workspace-relative path, outermost first.
 *
 * The file itself is excluded (it is not a directory to expand) and so is the
 * root, which is always loaded and has no row to open. Repeated separators
 * collapse, so `src//views/x.ts` and `src\views\x.ts` both yield
 * `["src", "src/views"]`. A top-level file, the empty path and a string of
 * nothing but separators all yield `[]` — there is nothing above them, which is
 * an answer rather than a failure.
 */
export function ancestorsOf(path: string): string[] {
  const segments = path.split(/[\\/]+/).filter((segment) => segment !== "");
  // The last segment is the file, and dropping it is what makes this a list of
  // *directories*. A single segment therefore leaves nothing behind.
  const dirs = segments.slice(0, -1);

  const result: string[] = [];
  let prefix = "";
  for (const segment of dirs) {
    prefix = prefix === "" ? segment : `${prefix}/${segment}`;
    result.push(prefix);
  }
  return result;
}

/** What revealing a file requires the tree to do, in the order to do it. */
export interface RevealPlan {
  /** Directories that must end up expanded — every ancestor of the file. */
  expand: string[];
  /**
   * The subset of `expand` whose listing is not cached yet, outermost first so
   * each fetch happens with its parent already on screen.
   */
  load: string[];
}

/**
 * The plan for revealing `activePath`, given the directories already listed.
 *
 * Everything on the way down has to be expanded, including directories that are
 * already loaded — a cached listing says nothing about whether its row is open,
 * and a previous *collapse all* leaves exactly that state.
 *
 * A null or empty `activePath` yields empty arrays rather than something that
 * looks like work: there is no open file, so there is nothing to reveal, and
 * expanding the root's children instead would be inventing an answer.
 */
export function revealPlan(activePath: string | null, loaded: Iterable<string>): RevealPlan {
  if (activePath === null || activePath.trim() === "") return { expand: [], load: [] };

  const expand = ancestorsOf(activePath);
  const cached = new Set(loaded);
  return { expand, load: expand.filter((dir) => !cached.has(dir)) };
}

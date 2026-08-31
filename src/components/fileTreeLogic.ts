/**
 * Pure decisions for the Run tab's file tree — where a new file lands, whether
 * a typed name is usable, and what a rename resolves to.
 *
 * Extracted rather than left inline for the reason every `*Logic.ts` module in
 * here exists: vitest runs in the node environment, so the React half of
 * `FileTree` cannot be tested at all, and these are the parts worth checking.
 *
 * Path spelling is the workspace-relative, forward-slash form
 * `cb_core::files::list_dir` returns, and every function here stays in it. The
 * backend re-validates anyway — `cb_core::files::resolve` refuses anything that
 * escapes the root — so this layer exists to give the user a useful message
 * before the round trip, not to be the guard.
 */

/** What a right-click acted on: a tree entry, or the empty space at the root. */
export interface MenuTarget {
  path: string;
  isDir: boolean;
}

/**
 * The directory a new file or folder should be created in.
 *
 * Right-clicking a folder means "in here"; right-clicking a file means "beside
 * this", which is the folder containing it. `null` is the root itself.
 */
export function targetDir(target: MenuTarget | null): string {
  if (target === null) return "";
  return target.isDir ? target.path : parentDir(target.path);
}

/** Everything before the last segment; `""` for a path with only one. */
export function parentDir(path: string): string {
  const cut = path.lastIndexOf("/");
  return cut === -1 ? "" : path.slice(0, cut);
}

/** The last segment of a path — what a rename starts out showing. */
export function baseName(path: string): string {
  const cut = path.lastIndexOf("/");
  return cut === -1 ? path : path.slice(cut + 1);
}

/** Join a directory and a name, without a leading slash at the root. */
export function joinPath(dir: string, name: string): string {
  return dir === "" ? name : `${dir}/${name}`;
}

/**
 * Windows forbids these outright in a file name, and a path containing one
 * cannot be created whatever the backend does. `:` is included even though the
 * backend's `resolve` already refuses a drive prefix, because `C:` typed into a
 * name box deserves a better message than "path escapes the workspace".
 */
const FORBIDDEN = /[<>:"|?*\u0000-\u001f]/;

/**
 * Why a typed name cannot be used, or `null` if it can.
 *
 * A nested name (`views/thing.tsx`) is allowed on purpose — the backend creates
 * the missing parents, and typing the folders is faster than making them one at
 * a time. What is refused is anything that would leave the workspace or name
 * nothing: `..`, an absolute path, an empty segment (`a//b`), a trailing slash.
 */
export function validateName(name: string): string | null {
  const trimmed = name.trim();
  if (trimmed === "") return "A name is required.";
  if (trimmed.startsWith("/") || trimmed.startsWith("\\")) {
    return "Enter a name, not an absolute path.";
  }
  if (FORBIDDEN.test(trimmed)) {
    return 'A name cannot contain < > : " | ? *';
  }

  const segments = trimmed.replace(/\\/g, "/").split("/");
  for (const segment of segments) {
    if (segment === "") return "A name cannot contain an empty folder.";
    if (segment === "." || segment === "..") return "A name cannot contain . or ..";
    // Trailing dots and spaces are silently stripped by Windows, so a file
    // created as `a ` is really `a` — refuse rather than create something
    // under a different name than the one the user read back.
    if (segment !== segment.trimEnd() || segment.endsWith(".")) {
      return "A name cannot end with a space or a dot.";
    }
  }
  return null;
}

/**
 * The full path a create resolves to, or `null` if the name is unusable.
 *
 * Backslashes are folded to forward slashes: on Windows the user may well type
 * the separator their shell uses, and both name the same place.
 */
export function createPath(dir: string, name: string): string | null {
  if (validateName(name) !== null) return null;
  return joinPath(dir, name.trim().replace(/\\/g, "/"));
}

/**
 * The full path a rename resolves to, or `null` if the name is unusable.
 *
 * The new name replaces the last segment only, so renaming `src/a.ts` to `b.ts`
 * keeps it in `src`. A name with slashes in it therefore *moves* the file, which
 * is deliberate — it is the only move gesture the tree has.
 */
export function renamePath(path: string, name: string): string | null {
  if (validateName(name) !== null) return null;
  return joinPath(parentDir(path), name.trim().replace(/\\/g, "/"));
}

/** Whether a rename is worth sending: valid, and actually a change. */
export function isRenameWorthSending(path: string, name: string): boolean {
  const next = renamePath(path, name);
  return next !== null && next !== path;
}

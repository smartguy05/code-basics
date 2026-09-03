/**
 * Which file-type icon a row shows.
 *
 * The Run tab's tree and the Changes tab's file list both render a filename and
 * nothing else, which makes a long list of near-identical paths hard to scan.
 * An icon fixes that, but only if it is *right*: an icon claiming a file is
 * Rust when it is not is worse than the plain text it replaced, because it is
 * read at a glance and never questioned. So this is a lookup table and not a
 * heuristic — an extension nobody wrote down here yields the generic document
 * icon rather than the nearest-looking brand.
 *
 * Keys are the file names of `material-icon-theme/icons/<key>.svg`, so
 * `FileIcon` can resolve one to an imported raw SVG with no second table. They
 * are a closed union rather than `string` for exactly that reason: adding a key
 * here without importing its SVG is a type error rather than a blank square.
 */

export type IconKey =
  | "folder"
  | "folder-open"
  | "document"
  | "csharp"
  | "visualstudio"
  | "rust"
  | "toml"
  | "typescript"
  | "react"
  | "javascript"
  | "json"
  | "markdown"
  | "yaml"
  | "database"
  | "html"
  | "css"
  | "xml"
  | "powershell"
  | "console"
  | "python"
  | "java"
  | "go"
  | "image"
  | "svg"
  | "log"
  | "lock"
  | "http"
  | "nodejs"
  | "docker"
  | "git";

/** What a file gets when nothing below recognises it. Never a guess. */
export const GENERIC_FILE: IconKey = "document";

/**
 * Whole names that outrank their own extension.
 *
 * `Cargo.toml` is a Rust manifest before it is TOML and `package.json` is a
 * Node manifest before it is JSON — the extension is true but says the less
 * useful of the two things. A lock file is the clearest case: `pnpm-lock.yaml`
 * is YAML that nobody ever edits, and drawing it as YAML puts it beside the
 * files that *are* edited.
 *
 * Looked up lowercased, so `Dockerfile` and `dockerfile` are the same entry.
 */
const BY_NAME: Record<string, IconKey> = {
  "cargo.toml": "rust",
  "cargo.lock": "rust",
  "package.json": "nodejs",
  "package-lock.json": "lock",
  "pnpm-lock.yaml": "lock",
  "pnpm-workspace.yaml": "nodejs",
  "yarn.lock": "lock",
  "tauri.conf.json": "json",
  dockerfile: "docker",
  "docker-compose.yml": "docker",
  "docker-compose.yaml": "docker",
  "compose.yml": "docker",
  "compose.yaml": "docker",
  ".gitignore": "git",
  ".gitattributes": "git",
  ".gitmodules": "git",
};

/**
 * Extension to icon, lowercased.
 *
 * Deliberately covers the mix this repository and the projects it opens
 * actually contain, and stops there. An entry earns its place by being a file
 * a developer here opens; anything else falls through to `GENERIC_FILE`, which
 * is a correct answer rather than a missing one.
 */
const BY_EXTENSION: Record<string, IconKey> = {
  cs: "csharp",
  csproj: "csharp",
  sln: "visualstudio",
  rs: "rust",
  toml: "toml",
  ts: "typescript",
  tsx: "react",
  js: "javascript",
  jsx: "react",
  mjs: "javascript",
  cjs: "javascript",
  json: "json",
  md: "markdown",
  yaml: "yaml",
  yml: "yaml",
  sql: "database",
  html: "html",
  css: "css",
  xml: "xml",
  ps1: "powershell",
  sh: "console",
  py: "python",
  java: "java",
  go: "go",
  png: "image",
  jpg: "image",
  jpeg: "image",
  gif: "image",
  ico: "image",
  svg: "svg",
  log: "log",
  lock: "lock",
  http: "http",
};

/**
 * The extension of `name`, lowercased, or `""` when it has none.
 *
 * A dot at index 0 is not an extension separator — `.gitignore` is a name whose
 * whole text is the identity, and reading `gitignore` as its extension would
 * put every dotfile into the extension table by accident. `..` therefore yields
 * `""` (the dot it finds is followed by nothing) and so does a trailing dot.
 */
export function extensionOf(name: string): string {
  const dot = name.lastIndexOf(".");
  if (dot <= 0) return "";
  return name.slice(dot + 1).toLowerCase();
}

/**
 * The icon for one row.
 *
 * A directory answers on its own — the name of a folder says nothing about what
 * it holds, and the open/closed pair is the row's expansion state made visible
 * rather than a claim about content. For a file the order is exact name, then
 * extension, then abstain.
 */
export function iconFor(name: string, isDir: boolean, expanded: boolean): IconKey {
  if (isDir) return expanded ? "folder-open" : "folder";

  const exact = BY_NAME[name.toLowerCase()];
  if (exact) return exact;

  const byExtension = BY_EXTENSION[extensionOf(name)];
  if (byExtension) return byExtension;

  return GENERIC_FILE;
}

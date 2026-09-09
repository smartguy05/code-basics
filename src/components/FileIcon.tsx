/**
 * The file-type icon beside a name in the file tree and the Changes list.
 *
 * A render shell: every decision about *which* icon is `fileIconLogic.iconFor`,
 * and this maps that key onto the SVG source.
 *
 * Two details are deliberate and easy to undo by accident.
 *
 * **Each icon is imported explicitly with `?raw`.** `material-icon-theme` ships
 * 1251 SVGs; an `import.meta.glob` over the directory would pull every one of
 * them into the build to use twenty-nine. Naming them one at a time is what
 * keeps the bundle honest, and the closed `IconKey` union is what makes a key
 * with no import here a type error rather than a blank square at runtime.
 *
 * **The SVG is inlined rather than referenced.** The Tauri CSP is
 * `default-src 'self'; style-src 'self' 'unsafe-inline'; img-src 'self' data:`,
 * so an `<img>` at an external URL simply would not load. The string is known
 * at build time and comes from a package on disk — never from a file's
 * contents, a workspace, or anything a user or agent wrote — which is what
 * makes `dangerouslySetInnerHTML` the correct tool here rather than a risk.
 */
import csharpSvg from "material-icon-theme/icons/csharp.svg?raw";
import consoleSvg from "material-icon-theme/icons/console.svg?raw";
import cssSvg from "material-icon-theme/icons/css.svg?raw";
import databaseSvg from "material-icon-theme/icons/database.svg?raw";
import dockerSvg from "material-icon-theme/icons/docker.svg?raw";
import documentSvg from "material-icon-theme/icons/document.svg?raw";
import folderOpenSvg from "material-icon-theme/icons/folder-open.svg?raw";
import folderSvg from "material-icon-theme/icons/folder.svg?raw";
import gitSvg from "material-icon-theme/icons/git.svg?raw";
import goSvg from "material-icon-theme/icons/go.svg?raw";
import htmlSvg from "material-icon-theme/icons/html.svg?raw";
import httpSvg from "material-icon-theme/icons/http.svg?raw";
import imageSvg from "material-icon-theme/icons/image.svg?raw";
import javaSvg from "material-icon-theme/icons/java.svg?raw";
import javascriptSvg from "material-icon-theme/icons/javascript.svg?raw";
import jsonSvg from "material-icon-theme/icons/json.svg?raw";
import lockSvg from "material-icon-theme/icons/lock.svg?raw";
import logSvg from "material-icon-theme/icons/log.svg?raw";
import markdownSvg from "material-icon-theme/icons/markdown.svg?raw";
import nodejsSvg from "material-icon-theme/icons/nodejs.svg?raw";
import powershellSvg from "material-icon-theme/icons/powershell.svg?raw";
import pythonSvg from "material-icon-theme/icons/python.svg?raw";
import reactSvg from "material-icon-theme/icons/react.svg?raw";
import rustSvg from "material-icon-theme/icons/rust.svg?raw";
import svgSvg from "material-icon-theme/icons/svg.svg?raw";
import tomlSvg from "material-icon-theme/icons/toml.svg?raw";
import typescriptSvg from "material-icon-theme/icons/typescript.svg?raw";
import visualstudioSvg from "material-icon-theme/icons/visualstudio.svg?raw";
import xmlSvg from "material-icon-theme/icons/xml.svg?raw";
import yamlSvg from "material-icon-theme/icons/yaml.svg?raw";
import { iconFor, type IconKey } from "./fileIconLogic";

const SOURCES: Record<IconKey, string> = {
  folder: folderSvg,
  "folder-open": folderOpenSvg,
  document: documentSvg,
  csharp: csharpSvg,
  visualstudio: visualstudioSvg,
  rust: rustSvg,
  toml: tomlSvg,
  typescript: typescriptSvg,
  react: reactSvg,
  javascript: javascriptSvg,
  json: jsonSvg,
  markdown: markdownSvg,
  yaml: yamlSvg,
  database: databaseSvg,
  html: htmlSvg,
  css: cssSvg,
  xml: xmlSvg,
  powershell: powershellSvg,
  console: consoleSvg,
  python: pythonSvg,
  java: javaSvg,
  go: goSvg,
  image: imageSvg,
  svg: svgSvg,
  log: logSvg,
  lock: lockSvg,
  http: httpSvg,
  nodejs: nodejsSvg,
  docker: dockerSvg,
  git: gitSvg,
};

/**
 * `aria-hidden` because the icon adds nothing a screen reader needs: the
 * filename sits immediately beside it and already carries the whole meaning,
 * so announcing the type a second time would only be noise.
 */
export function FileIcon({
  name,
  isDir = false,
  expanded = false,
}: {
  name: string;
  isDir?: boolean;
  expanded?: boolean;
}) {
  return (
    <span
      className="tree-icon"
      aria-hidden="true"
      dangerouslySetInnerHTML={{ __html: SOURCES[iconFor(name, isDir, expanded)] }}
    />
  );
}

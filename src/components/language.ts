import type { Extension } from "@codemirror/state";
import {
  bracketMatching,
  HighlightStyle,
  syntaxHighlighting,
} from "@codemirror/language";
import { tags } from "@lezer/highlight";
import { javascript } from "@codemirror/lang-javascript";
import { markdown, markdownLanguage } from "@codemirror/lang-markdown";
import { languages } from "@codemirror/language-data";
import { json } from "@codemirror/lang-json";
import { css } from "@codemirror/lang-css";
import { html } from "@codemirror/lang-html";
import { python } from "@codemirror/lang-python";
import { rust } from "@codemirror/lang-rust";
import { xml } from "@codemirror/lang-xml";
import { cpp } from "@codemirror/lang-cpp";

/**
 * Token colours for the app's dark theme.
 *
 * CodeMirror's language modes only *parse* — without a `syntaxHighlighting`
 * extension every token renders in the plain text colour. The palette loosely
 * follows VS Code's dark theme, familiar enough to read at a glance.
 */
const highlightStyle = HighlightStyle.define([
  { tag: [tags.keyword, tags.modifier, tags.controlKeyword], color: "#c586c0" },
  { tag: [tags.string, tags.special(tags.string)], color: "#ce9178" },
  { tag: [tags.comment, tags.blockComment, tags.lineComment], color: "#6a9955", fontStyle: "italic" },
  { tag: [tags.number, tags.integer, tags.float], color: "#b5cea8" },
  { tag: [tags.bool, tags.atom, tags.null, tags.self], color: "#569cd6" },
  { tag: [tags.function(tags.variableName), tags.function(tags.propertyName)], color: "#dcdcaa" },
  { tag: [tags.typeName, tags.className, tags.namespace], color: "#4ec9b0" },
  { tag: [tags.propertyName, tags.attributeName], color: "#9cdcfe" },
  { tag: [tags.definition(tags.variableName), tags.local(tags.variableName)], color: "#9cdcfe" },
  { tag: [tags.tagName], color: "#569cd6" },
  { tag: [tags.operator, tags.operatorKeyword], color: "#d4d4d4" },
  { tag: [tags.bracket, tags.paren, tags.squareBracket, tags.brace], color: "#f2c55c" },
  { tag: [tags.angleBracket], color: "#808080" },
  { tag: [tags.regexp, tags.escape], color: "#d16969" },
  { tag: [tags.meta, tags.processingInstruction], color: "#8b93a3" },
  { tag: tags.invalid, color: "#e05561" },
  { tag: tags.strong, fontWeight: "bold" },
  { tag: tags.emphasis, fontStyle: "italic" },
  { tag: tags.link, color: "#5a78dc", textDecoration: "underline" },
  { tag: tags.heading, color: "#569cd6", fontWeight: "bold" },
  { tag: tags.strikethrough, textDecoration: "line-through" },
  { tag: tags.monospace, color: "#ce9178" },
  { tag: tags.quote, color: "#6a9955", fontStyle: "italic" },
  { tag: tags.contentSeparator, color: "#f2c55c" },
]);

/**
 * Syntax colours plus matching-bracket highlighting, for every code editor in
 * the app (file editor and diff views). Spread alongside `languageFor`.
 */
export const editorColors: Extension[] = [
  syntaxHighlighting(highlightStyle),
  bracketMatching(),
];

/**
 * Pick a CodeMirror language mode from the file extension.
 *
 * Syntax highlighting is cosmetic here, so an unknown extension falls back to
 * no mode rather than guessing.
 */
export function languageFor(path: string): Extension[] {
  const extension = path.split(".").pop()?.toLowerCase() ?? "";

  switch (extension) {
    case "ts":
    case "tsx":
      return [javascript({ typescript: true, jsx: extension === "tsx" })];
    case "js":
    case "jsx":
    case "mjs":
    case "cjs":
      return [javascript({ jsx: extension === "jsx" })];
    case "json":
      return [json()];
    case "md":
    case "markdown":
      // GFM (tables, strikethrough, task lists), with fenced code blocks
      // highlighted in their own language, loaded lazily per language.
      return [markdown({ base: markdownLanguage, codeLanguages: languages })];
    case "css":
    case "scss":
      return [css()];
    case "html":
    // ASP.NET razor / cshtml views: HTML markup with C# interleaved in
    // `@code` / `@{ … }` blocks. No razor mode ships with CodeMirror, so the
    // HTML mode is the close-enough approximation — it highlights the markup
    // and leaves the C# as plain text (the same trade cpp makes for C#).
    case "razor":
    case "cshtml":
      return [html()];
    case "py":
      return [python()];
    case "rs":
      return [rust()];
    case "xml":
    case "csproj":
    case "fsproj":
    case "props":
    case "targets":
      return [xml()];
    case "cs":
    case "c":
    case "h":
    case "cpp":
    case "hpp":
      // No dedicated C# mode ships with CodeMirror; the C-family one is a
      // close enough approximation.
      return [cpp()];
    default:
      return [];
  }
}

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
  { tag: [tags.keyword, tags.modifier, tags.controlKeyword], color: "var(--syntax-keyword)" },
  { tag: [tags.string, tags.special(tags.string)], color: "var(--syntax-string)" },
  { tag: [tags.comment, tags.blockComment, tags.lineComment], color: "var(--syntax-comment)", fontStyle: "italic" },
  { tag: [tags.number, tags.integer, tags.float], color: "var(--syntax-number)" },
  { tag: [tags.bool, tags.atom, tags.null, tags.self], color: "var(--syntax-literal)" },
  { tag: [tags.function(tags.variableName), tags.function(tags.propertyName)], color: "var(--syntax-function)" },
  { tag: [tags.typeName, tags.className, tags.namespace], color: "var(--syntax-type)" },
  { tag: [tags.propertyName, tags.attributeName], color: "var(--syntax-property)" },
  { tag: [tags.definition(tags.variableName), tags.local(tags.variableName)], color: "var(--syntax-property)" },
  { tag: [tags.tagName], color: "var(--syntax-tag)" },
  { tag: [tags.operator, tags.operatorKeyword], color: "var(--syntax-operator)" },
  { tag: [tags.bracket, tags.paren, tags.squareBracket, tags.brace], color: "var(--syntax-bracket)" },
  { tag: [tags.angleBracket], color: "#808080" },
  { tag: [tags.regexp, tags.escape], color: "var(--syntax-regexp)" },
  { tag: [tags.meta, tags.processingInstruction], color: "var(--syntax-meta)" },
  { tag: tags.invalid, color: "var(--syntax-invalid)" },
  { tag: tags.strong, fontWeight: "bold" },
  { tag: tags.emphasis, fontStyle: "italic" },
  { tag: tags.link, color: "var(--syntax-link)", textDecoration: "underline" },
  { tag: tags.heading, color: "var(--syntax-tag)", fontWeight: "bold" },
  { tag: tags.strikethrough, textDecoration: "line-through" },
  { tag: tags.monospace, color: "var(--syntax-string)" },
  { tag: tags.quote, color: "var(--syntax-comment)", fontStyle: "italic" },
  { tag: tags.contentSeparator, color: "var(--syntax-bracket)" },
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

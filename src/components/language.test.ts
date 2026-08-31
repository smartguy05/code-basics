import { describe, expect, it } from "vitest";
import { Language, LanguageSupport } from "@codemirror/language";
import {
  javascriptLanguage,
  jsxLanguage,
  tsxLanguage,
  typescriptLanguage,
} from "@codemirror/lang-javascript";
import { editorColors, languageFor } from "./language";

/** The single language mode `languageFor` picked, or null when it abstained. */
function modeOf(path: string): LanguageSupport | null {
  const extensions = languageFor(path);
  if (extensions.length === 0) return null;
  expect(extensions).toHaveLength(1);
  const [mode] = extensions;
  expect(mode).toBeInstanceOf(LanguageSupport);
  return mode as LanguageSupport;
}

/** Name of the CodeMirror language the path resolved to. */
function languageName(path: string): string | null {
  return modeOf(path)?.language.name ?? null;
}

/**
 * The exact `Language` instance behind the path. The javascript mode shares one
 * singleton per dialect, so identity is what distinguishes tsx from ts — the
 * `name` is "typescript" for both.
 */
function languageOf(path: string): Language | null {
  return modeOf(path)?.language ?? null;
}

describe("languageFor", () => {
  it("picks the TypeScript/JSX flavour of the javascript mode for .ts and .tsx", () => {
    expect(languageOf("src/App.tsx")).toBe(tsxLanguage);
    expect(languageOf("src/ipc/api.ts")).toBe(typescriptLanguage);
  });

  it("picks plain javascript for .js/.mjs/.cjs and the jsx flavour for .jsx", () => {
    expect(languageOf("scripts/build.js")).toBe(javascriptLanguage);
    expect(languageOf("scripts/generate-index.mjs")).toBe(javascriptLanguage);
    expect(languageOf("scripts/legacy.cjs")).toBe(javascriptLanguage);
    expect(languageOf("src/Old.jsx")).toBe(jsxLanguage);
  });

  it("maps the data and markup extensions to their own modes", () => {
    expect(languageName("tauri.conf.json")).toBe("json");
    expect(languageName("README.md")).toBe("markdown");
    expect(languageName("docs/notes.markdown")).toBe("markdown");
    expect(languageName("src/styles.css")).toBe("css");
    expect(languageName("src/theme.scss")).toBe("css");
    expect(languageName("index.html")).toBe("html");
  });

  it("approximates ASP.NET razor/cshtml views with the html mode", () => {
    // HTML-dominant with embedded C#; no razor mode ships, so the markup mode
    // is the close-enough approximation (like cpp for C#).
    expect(languageName("src/Pages/Counter.razor")).toBe("html");
    expect(languageName("Views/Home/Index.cshtml")).toBe("html");
    expect(languageName("PAGES/COUNTER.RAZOR")).toBe("html");
  });

  it("maps python and rust sources", () => {
    expect(languageName("tools/report.py")).toBe("python");
    expect(languageName("crates/core/src/lib.rs")).toBe("rust");
  });

  it("routes MSBuild project files through the xml mode", () => {
    expect(languageName("app.xml")).toBe("xml");
    expect(languageName("src/App.csproj")).toBe("xml");
    expect(languageName("src/App.fsproj")).toBe("xml");
    expect(languageName("Directory.Build.props")).toBe("xml");
    expect(languageName("Directory.Build.targets")).toBe("xml");
  });

  it("approximates the C family (including C#) with the cpp mode", () => {
    for (const path of ["Program.cs", "main.c", "main.h", "main.cpp", "main.hpp"]) {
      expect(languageName(path)).toBe("cpp");
    }
  });

  it("is case-insensitive about the extension", () => {
    expect(languageName("Program.CS")).toBe("cpp");
    expect(languageName("CONFIG.JSON")).toBe("json");
    expect(languageOf("src/App.TSX")).toBe(tsxLanguage);
  });

  it("abstains rather than guessing for unknown or missing extensions", () => {
    expect(languageFor("LICENSE")).toEqual([]);
    expect(languageFor("bin/tool.exe")).toEqual([]);
    expect(languageFor("")).toEqual([]);
    expect(languageFor("archive.tar.gz")).toEqual([]);
  });

  it("uses only the final extension of a multi-dotted name", () => {
    expect(languageName("vite.config.ts")).toBe("typescript");
    expect(languageName("app.settings.json")).toBe("json");
  });

  it("treats a trailing dot as an empty extension", () => {
    expect(languageFor("weird.")).toEqual([]);
  });

  it("ignores dots that appear in directory names", () => {
    expect(languageFor(".code-basics/adapters/pytest.toml")).toEqual([]);
    expect(languageName("my.dir/file.rs")).toBe("rust");
  });

  it("returns a fresh array on each call so callers can spread it safely", () => {
    const first = languageFor("a.json");
    const second = languageFor("a.json");
    expect(first).not.toBe(second);
  });
});

describe("editorColors", () => {
  it("bundles the highlighter and bracket matching for every editor", () => {
    expect(editorColors).toHaveLength(2);
  });
});

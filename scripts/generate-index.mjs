#!/usr/bin/env node
/**
 * Regenerates docs/INDEX.md — the generated map of source files, Tauri
 * commands, IPC wrappers, and public core APIs.
 *
 * Usage: node scripts/generate-index.mjs   (or: pnpm docs:index)
 *
 * Everything is derived from the source tree, so the index can never drift
 * further than one run of this script. Do not edit docs/INDEX.md by hand.
 */

import { readFileSync, writeFileSync, readdirSync, statSync } from "node:fs";
import { join, relative, extname } from "node:path";
import { fileURLToPath } from "node:url";

const root = join(fileURLToPath(import.meta.url), "..", "..");
const out = join(root, "docs", "INDEX.md");

/** Directories under the repo root that contain first-party source. */
const SOURCE_ROOTS = ["crates/core", "src", "src-tauri/src", "scripts", "examples"];
const SOURCE_EXTS = new Set([".rs", ".ts", ".tsx", ".mjs", ".toml"]);
const SKIP = new Set(["node_modules", "target", "dist", "fixtures", "icons", "capabilities", "gen"]);

function* walk(dir) {
  for (const name of readdirSync(dir).sort()) {
    const path = join(dir, name);
    const stat = statSync(path);
    if (stat.isDirectory()) {
      if (!SKIP.has(name)) yield* walk(path);
    } else if (SOURCE_EXTS.has(extname(name))) {
      yield path;
    }
  }
}

/** First meaningful doc line of a file: `//!` (Rust), a leading comment, or the doc block. */
function summary(path, text) {
  const lines = text.split(/\r?\n/);
  for (const line of lines.slice(0, 10)) {
    const t = line.trim();
    if (t.startsWith("#!")) continue; // shebang, not a comment
    for (const prefix of ["//!", "/**", "*", "//", "#"]) {
      if (t.startsWith(prefix)) {
        const body = t.slice(prefix.length).replace(/\*\/\s*$/, "").trim();
        if (body) return body;
      }
    }
    if (t !== "") break; // first line is code — no summary comment
  }
  return "";
}

function lineCount(text) {
  return text.split("\n").length - (text.endsWith("\n") ? 1 : 0);
}

// ---------------------------------------------------------------------------
// Collect files
// ---------------------------------------------------------------------------

const files = [];
for (const src of SOURCE_ROOTS) {
  for (const path of walk(join(root, src))) {
    const text = readFileSync(path, "utf8");
    files.push({
      rel: relative(root, path).replaceAll("\\", "/"),
      lines: lineCount(text),
      summary: summary(path, text),
      text,
    });
  }
}

// ---------------------------------------------------------------------------
// Tauri commands, from the generate_handler! block
// ---------------------------------------------------------------------------

const libRs = readFileSync(join(root, "src-tauri", "src", "lib.rs"), "utf8");
const handlerBlock = libRs.match(/generate_handler!\[([\s\S]*?)\]/);
const commands = handlerBlock
  ? [...handlerBlock[1].matchAll(/commands::(\w+)::(\w+)/g)].map((m) => ({
      module: m[1],
      name: m[2],
    }))
  : [];

// ---------------------------------------------------------------------------
// Frontend IPC wrappers, from src/ipc/api.ts exports
// ---------------------------------------------------------------------------

const apiTs = readFileSync(join(root, "src", "ipc", "api.ts"), "utf8");
const wrappers = [...apiTs.matchAll(/^export (?:const|function) (\w+)/gm)].map((m) => m[1]);

// ---------------------------------------------------------------------------
// Public core API: pub fn / struct / enum per cb-core file
// ---------------------------------------------------------------------------

const PUB_RE = /^\s*pub(?:\(crate\))? (?:async )?(fn|struct|enum) (\w+)/;
const coreApi = files
  .filter((f) => f.rel.startsWith("crates/core/src/") && !f.rel.includes("_tests"))
  .map((f) => {
    const items = [];
    let inTests = false;
    for (const line of f.text.split(/\r?\n/)) {
      if (/^\s*#\[cfg\(test\)\]/.test(line)) inTests = true;
      if (inTests) continue;
      const m = line.match(PUB_RE);
      if (m) items.push(`${m[2]}${m[1] === "fn" ? "()" : ""}`);
    }
    return { rel: f.rel, items };
  })
  .filter((f) => f.items.length > 0);

// ---------------------------------------------------------------------------
// Render
// ---------------------------------------------------------------------------

const md = [];
md.push("# Code index");
md.push("");
md.push("> **Generated** by [`scripts/generate-index.mjs`](../scripts/generate-index.mjs) — do not edit by hand.");
md.push("> Regenerate with `pnpm docs:index` after adding files, commands, or public APIs.");
md.push("");
md.push("Use this file to locate things fast: every first-party source file with its one-line purpose, the full Tauri command surface, the frontend IPC wrappers, and the public API of each `cb-core` module.");
md.push("");

md.push("## Source files");
md.push("");
md.push("| File | Lines | Purpose |");
md.push("|------|------:|---------|");
for (const f of files) {
  md.push(`| \`${f.rel}\` | ${f.lines} | ${f.summary.replaceAll("|", "\\|")} |`);
}
md.push("");

md.push("## Tauri command surface");
md.push("");
md.push("Registered in `src-tauri/src/lib.rs`; documented with parameters in [reference/commands.md](reference/commands.md).");
md.push("");
for (const module of [...new Set(commands.map((c) => c.module))]) {
  const names = commands.filter((c) => c.module === module).map((c) => `\`${c.name}\``);
  md.push(`- **${module}** (\`src-tauri/src/commands/${module}.rs\`): ${names.join(", ")}`);
}
md.push("");

md.push("## Frontend IPC wrappers (`src/ipc/api.ts`)");
md.push("");
md.push(wrappers.map((w) => `\`${w}\``).join(", "));
md.push("");

md.push("## Public core API (`cb-core`)");
md.push("");
for (const f of coreApi) {
  md.push(`- \`${f.rel}\`: ${f.items.map((i) => `\`${i}\``).join(", ")}`);
}
md.push("");

writeFileSync(out, md.join("\n"));
console.log(
  `Wrote docs/INDEX.md: ${files.length} files, ${commands.length} commands, ` +
    `${wrappers.length} IPC wrappers, ${coreApi.length} core modules.`,
);

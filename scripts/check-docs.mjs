#!/usr/bin/env node
/**
 * Documentation health check.
 *
 * Usage: node scripts/check-docs.mjs   (or: pnpm docs:check)
 *
 * Enforces the two rules the docs tree lives by:
 *   1. No markdown file in docs/ (or README.md / CLAUDE.md) exceeds 500 lines.
 *      The generated `docs/INDEX.md` is exempt — it is a machine-written lookup
 *      table that grows with the codebase and cannot be "split logically".
 *   2. Every relative link in those files resolves to a real file.
 *
 * Exits non-zero listing every violation, so it can run in CI or a hook.
 */

import { readFileSync, readdirSync, statSync, existsSync } from "node:fs";
import { join, dirname, relative } from "node:path";
import { fileURLToPath } from "node:url";

const root = join(fileURLToPath(import.meta.url), "..", "..");
const MAX_LINES = 500;

function* walk(dir) {
  for (const name of readdirSync(dir).sort()) {
    const path = join(dir, name);
    if (statSync(path).isDirectory()) yield* walk(path);
    else if (name.endsWith(".md")) yield path;
  }
}

const targets = [...walk(join(root, "docs"))];
for (const extra of ["README.md", "CLAUDE.md"]) {
  const path = join(root, extra);
  if (existsSync(path)) targets.push(path);
}

const problems = [];

// Markdown links: [text](target), skipping images, absolute URLs and anchors.
const LINK_RE = /(?<!!)\[[^\]]*\]\(([^)\s]+)\)/g;

for (const path of targets) {
  const rel = relative(root, path).replaceAll("\\", "/");
  const text = readFileSync(path, "utf8");

  // The generated index is a lookup table that grows with the source tree and
  // cannot be hand-split — it is exempt from the line cap (links still checked).
  const lines = text.split("\n").length;
  if (lines > MAX_LINES && rel !== "docs/INDEX.md") {
    problems.push(`${rel}: ${lines} lines (limit ${MAX_LINES}) — split it logically`);
  }

  for (const match of text.matchAll(LINK_RE)) {
    const target = match[1];
    if (/^[a-z]+:/i.test(target) || target.startsWith("#")) continue; // URL or anchor
    const file = target.split("#")[0];
    if (!file) continue;
    if (!existsSync(join(dirname(path), decodeURIComponent(file)))) {
      problems.push(`${rel}: broken relative link -> ${target}`);
    }
  }
}

if (problems.length > 0) {
  console.error(`docs check failed (${problems.length} problem${problems.length === 1 ? "" : "s"}):`);
  for (const p of problems) console.error(`  - ${p}`);
  process.exit(1);
}

console.log(`docs check passed: ${targets.length} files, all under ${MAX_LINES} lines, all relative links resolve.`);

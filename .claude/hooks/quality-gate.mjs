#!/usr/bin/env node
// Stop-hook quality gate for code-basics.
//
// Runs when a turn ends. Deterministically enforces the CLAUDE.md rules that a
// shell command *can* enforce, and blocks the stop (exit 2) with actionable
// text when a blocking gate fails — Claude Code shows that stderr text to the
// model and lets it continue. Every decision lives in ./quality-gate-logic.mjs
// so it can be unit-tested; this file only does I/O and process spawning.
//
// Gates, in order:
//   1. loop guard   — stop_hook_active ⇒ exit 0 (never re-block the same turn)
//   2. typecheck    — blocking, if *.ts/tsx changed  (`pnpm typecheck`)
//   3. rustfmt      — blocking, if *.rs changed       (`cargo fmt --check`)
//   4. AI-REJECTED  — blocking, if a changed file carries a date-stamped note
//   5. memory nudge — advisory (exit 0) when source changed but no .memories/
//
// Exit codes (Claude Code contract): 0 = allow stop (stderr shown as a notice);
// 2 = block stop, feed stderr back to the model; other = non-blocking error.
//
// Slow/lock-prone checks (`cargo test`, `cargo clippy`) are opt-in via
// CB_GATE_FULL=1 — cargo can hit the "app is running ⇒ Access denied" relink
// lock, so they are off by default. `cargo fmt --check` never relinks.

import { spawnSync } from "node:child_process";
import { readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import { dirname, resolve } from "node:path";
import {
  gatesForChanges,
  shouldRemindMemories,
  hasUnresolvedRejection,
  shouldSkipForLoop,
} from "./quality-gate-logic.mjs";

const HOOK_DIR = dirname(fileURLToPath(import.meta.url));
// .claude/hooks -> repo root. Prefer the env var Claude Code provides; fall
// back to walking up from this file so a manual `node .claude/hooks/…` works.
const REPO_ROOT = process.env.CLAUDE_PROJECT_DIR
  ? resolve(process.env.CLAUDE_PROJECT_DIR)
  : resolve(HOOK_DIR, "..", "..");

function readPayload() {
  try {
    const raw = readFileSync(0, "utf8"); // fd 0 = stdin
    return raw.trim() ? JSON.parse(raw) : {};
  } catch {
    return {};
  }
}

// Working-tree change set vs HEAD: tracked modifications plus untracked files.
function changedPaths() {
  const run = (args) =>
    spawnSync("git", args, { cwd: REPO_ROOT, encoding: "utf8" });
  const tracked = run(["diff", "--name-only", "HEAD"]);
  const untracked = run(["ls-files", "--others", "--exclude-standard"]);
  const out = [];
  for (const r of [tracked, untracked]) {
    if (r.status === 0 && r.stdout) {
      for (const line of r.stdout.split(/\r?\n/)) {
        const p = line.trim();
        if (p) out.push(p);
      }
    }
  }
  return [...new Set(out)];
}

// Run a command, capturing combined output. Returns {ok, output}.
function runCheck(cmd, args) {
  const r = spawnSync(cmd, args, {
    cwd: REPO_ROOT,
    encoding: "utf8",
    shell: process.platform === "win32", // resolve pnpm/cargo .cmd shims on Windows
  });
  const output = [r.stdout, r.stderr].filter(Boolean).join("\n").trim();
  if (r.error) return { ok: false, output: `failed to run ${cmd}: ${r.error.message}` };
  return { ok: r.status === 0, output };
}

const GATE_COMMANDS = {
  typecheck: { label: "pnpm typecheck", cmd: "pnpm", args: ["typecheck"] },
  rustfmt: { label: "cargo fmt --check", cmd: "cargo", args: ["fmt", "--check"] },
};

function blockStop(message) {
  process.stderr.write(message.endsWith("\n") ? message : message + "\n");
  process.exit(2);
}

function main() {
  const payload = readPayload();

  // 1. Loop guard — never re-block a turn a prior Stop hook already handled.
  if (shouldSkipForLoop(payload)) process.exit(0);

  const changed = changedPaths();
  if (changed.length === 0) process.exit(0); // nothing edited ⇒ nothing to gate

  // 2 & 3. Blocking language gates.
  const gates = gatesForChanges(changed);
  if (process.env.CB_GATE_FULL === "1") {
    // Opt-in heavier Rust checks, appended after fmt so fmt failures surface first.
    if (gates.includes("rustfmt")) {
      GATE_COMMANDS.clippy = {
        label: "cargo clippy",
        cmd: "cargo",
        args: ["clippy", "--workspace", "--all-targets", "--", "-D", "warnings"],
      };
      gates.push("clippy");
    }
  }
  for (const gate of gates) {
    const spec = GATE_COMMANDS[gate];
    if (!spec) continue;
    const { ok, output } = runCheck(spec.cmd, spec.args);
    if (!ok) {
      blockStop(
        `Quality gate failed: ${spec.label}\n` +
          `Fix the reported problems before finishing this turn.\n\n` +
          output,
      );
    }
  }

  // 4. AI-REJECTED detector — surface the pre-commit refusal now, not at commit.
  const flagged = [];
  for (const rel of changed) {
    try {
      const text = readFileSync(resolve(REPO_ROOT, rel), "utf8");
      if (hasUnresolvedRejection(text)) flagged.push(rel);
    } catch {
      // deleted/binary/unreadable — skip
    }
  }
  if (flagged.length > 0) {
    blockStop(
      `Unresolved AI-REJECTED note(s) in changed files:\n` +
        flagged.map((f) => `  ${f}`).join("\n") +
        `\n\nImplement a correct fix that addresses the stated reason, then ` +
        `delete the whole note block in the same edit (a commit that still ` +
        `carries one is refused by the pre-commit hook).`,
    );
  }

  // 5. Memory advisory — non-blocking reminder, turn still completes.
  if (shouldRemindMemories(changed)) {
    process.stderr.write(
      "Reminder: this turn edited source but touched no .memories/ file. " +
        "If this work item's state changed, update its work-item memory " +
        "(notes.md / todos.md / completed.md).\n",
    );
  }

  process.exit(0);
}

main();

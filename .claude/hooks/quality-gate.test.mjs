// Unit tests for the pure decision helpers behind the Stop quality-gate hook.
// Run: node .claude/hooks/quality-gate.test.mjs   (no deps — node:test)

import { test } from "node:test";
import assert from "node:assert/strict";
import {
  gatesForChanges,
  touchedSource,
  touchedMemories,
  shouldRemindMemories,
  hasUnresolvedRejection,
  shouldSkipForLoop,
} from "./quality-gate-logic.mjs";

test("gatesForChanges picks typecheck for TS/TSX changes", () => {
  assert.deepEqual(gatesForChanges(["src/App.tsx"]), ["typecheck"]);
  assert.deepEqual(gatesForChanges(["src/ipc/api.ts"]), ["typecheck"]);
});

test("gatesForChanges picks rustfmt for .rs changes", () => {
  assert.deepEqual(gatesForChanges(["crates/core/src/model.rs"]), ["rustfmt"]);
});

test("gatesForChanges picks both, and handles backslash paths", () => {
  assert.deepEqual(
    gatesForChanges(["src\\App.tsx", "crates\\core\\src\\model.rs"]),
    ["typecheck", "rustfmt"],
  );
});

test("gatesForChanges ignores unrelated files", () => {
  assert.deepEqual(gatesForChanges(["README.md", "docs/INDEX.md"]), []);
  assert.deepEqual(gatesForChanges([]), []);
  assert.deepEqual(gatesForChanges(undefined), []);
});

test("touchedSource is true only under real source roots", () => {
  assert.equal(touchedSource(["src/App.tsx"]), true);
  assert.equal(touchedSource(["crates/core/src/model.rs"]), true);
  assert.equal(touchedSource(["src-tauri/src/lib.rs"]), true);
  assert.equal(touchedSource(["sidecar/inspector/Program.cs"]), true);
  assert.equal(touchedSource(["docs/README.md"]), false);
  assert.equal(touchedSource([".memories/features/x/notes.md"]), false);
});

test("touchedMemories detects .memories/ paths", () => {
  assert.equal(touchedMemories([".memories/bugs/x/notes.md"]), true);
  assert.equal(touchedMemories(["src/App.tsx"]), false);
});

test("shouldRemindMemories: source edited, no memory touched", () => {
  assert.equal(shouldRemindMemories(["src/App.tsx"]), true);
  // memory was updated alongside source ⇒ no reminder
  assert.equal(
    shouldRemindMemories(["src/App.tsx", ".memories/features/x/completed.md"]),
    false,
  );
  // only docs changed ⇒ no reminder
  assert.equal(shouldRemindMemories(["docs/README.md"]), false);
});

test("hasUnresolvedRejection matches a date-stamped head line only", () => {
  const tokenParts = ["AI-", "REJECTED"].join("");
  assert.equal(hasUnresolvedRejection(`// ${tokenParts} 2026-08-19\n// reason`), true);
  // bare token without a date is committable ⇒ not flagged
  assert.equal(hasUnresolvedRejection(`// mentions ${tokenParts} in prose`), false);
  assert.equal(hasUnresolvedRejection("nothing here"), false);
  assert.equal(hasUnresolvedRejection(undefined), false);
});

test("shouldSkipForLoop honors stop_hook_active", () => {
  assert.equal(shouldSkipForLoop({ stop_hook_active: true }), true);
  assert.equal(shouldSkipForLoop({ stop_hook_active: false }), false);
  assert.equal(shouldSkipForLoop({}), false);
  assert.equal(shouldSkipForLoop(null), false);
});

// Pure decision helpers for the Stop quality-gate hook.
//
// No I/O, no process spawning — every function here is a pure map from data to
// data so it can be unit-tested headlessly (see quality-gate.test.mjs), the
// same split the repo uses for its `*Logic.ts` modules. The runner
// (quality-gate.mjs) does the filesystem/git/spawn work and delegates every
// decision to this file.

// Extensions that trigger each language gate.
const TS_EXT = /\.(ts|tsx|mts|cts)$/i;
const RS_EXT = /\.rs$/i;

// The AI-REJECTED head-line pattern, kept identical to the git pre-commit guard
// (`.git/hooks/pre-commit`): the bare token alone is committable; only a
// date-stamped note head line is a real unresolved rejection. Assembled from
// two pieces so this source file does not itself contain the token it scans
// for — matching guard.rs's reasoning.
const REJECT_TOKEN = "AI-" + "REJECTED";
export const REJECT_PATTERN = new RegExp(
  REJECT_TOKEN + " \\d{4}-\\d{2}-\\d{2}",
);

// Source paths that should nudge a `.memories/` update when none was touched.
// Anything under these roots counts as "real work"; config/doc/memory edits do
// not, to keep the advisory from firing on trivial turns.
const SOURCE_ROOTS = ["src/", "src-tauri/", "crates/", "sidecar/"];

function normalize(paths) {
  return (paths ?? []).map((p) => p.replace(/\\/g, "/")).filter(Boolean);
}

// Which blocking language gates the change set warrants. Returns a set of
// gate ids; the runner maps each to a command.
export function gatesForChanges(changedPaths) {
  const paths = normalize(changedPaths);
  const gates = [];
  if (paths.some((p) => TS_EXT.test(p))) gates.push("typecheck");
  if (paths.some((p) => RS_EXT.test(p))) gates.push("rustfmt");
  return gates;
}

// True when the change set includes real source edits (so an Intent/memory
// note would be expected) — used only for the non-blocking memory advisory.
export function touchedSource(changedPaths) {
  const paths = normalize(changedPaths);
  return paths.some((p) => SOURCE_ROOTS.some((root) => p.startsWith(root)));
}

// True when at least one `.memories/` file was part of the change set.
export function touchedMemories(changedPaths) {
  const paths = normalize(changedPaths);
  return paths.some((p) => p.startsWith(".memories/"));
}

// Whether the memory advisory should fire: source was edited but no memory
// file was touched.
export function shouldRemindMemories(changedPaths) {
  return touchedSource(changedPaths) && !touchedMemories(changedPaths);
}

// Does a file's text carry an unresolved AI-REJECTED note (date-stamped head
// line)? Mirrors the git pre-commit guard so detection here matches what the
// committer will later refuse.
export function hasUnresolvedRejection(fileText) {
  return REJECT_PATTERN.test(fileText ?? "");
}

// The Stop-hook loop guard. Claude Code sets `stop_hook_active` true once a
// Stop hook has already fired this turn; re-blocking then would loop forever.
export function shouldSkipForLoop(payload) {
  return Boolean(payload && payload.stop_hook_active);
}

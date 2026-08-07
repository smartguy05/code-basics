import type { TestCase, TestNode, TestOutcome, TestSummary } from "../ipc/types";

/**
 * Per-test result lines across the supported runners: VSTest's
 * `Passed Name [1 ms]` (console verbosity `normal`), MTP's lowercase
 * `failed Name`, Vitest's `✓ file > name`, Jest's per-file `PASS path`.
 * Summary lines like `Passed!  - Failed: 0, ...` do not match — the `!`
 * blocks the required whitespace after the word.
 */
export const LIVE_LINE =
  /^\s*(passed|√|✓|✔|pass|failed|×|✗|✕|fail|skipped|↓|○|skip)\s+(\S.*)$/i;
export const PASS_MARKS = new Set(["passed", "√", "✓", "✔", "pass"]);
export const FAIL_MARKS = new Set(["failed", "×", "✗", "✕", "fail"]);

/** Read one console line as a test result, extracting the test's name. */
export function classifyLine(
  line: string,
): { outcome: TestOutcome; name: string } | null {
  const match = LIVE_LINE.exec(line);
  if (!match || match[1] == null || match[2] == null) return null;

  const marker = match[1].toLowerCase();
  const outcome: TestOutcome = PASS_MARKS.has(marker)
    ? "passed"
    : FAIL_MARKS.has(marker)
      ? "failed"
      : "skipped";
  // Drop a trailing duration: `[12 ms]`, `(12ms)`, or a bare `12ms`.
  const name = match[2].replace(/\s*[[(]?[\d.,]+\s*m?s[\])]?\s*$/i, "").trim();
  return { outcome, name };
}

/** The live outcome of a case, matched loosely against reported names. */
export function liveOutcomeFor(
  testCase: TestCase,
  results: Map<string, TestOutcome>,
): TestOutcome {
  const exact = results.get(testCase.fullName) ?? results.get(testCase.name);
  if (exact) return exact;
  for (const [name, outcome] of results) {
    if (name.endsWith(testCase.fullName) || testCase.fullName.endsWith(name)) {
      return outcome;
    }
  }
  return "other";
}

/**
 * Recolour a finished run's tree with this run's live results: every test
 * starts grey and turns green/red/yellow as its result line streams in.
 */
export function applyLiveOutcomes(
  node: TestNode,
  results: Map<string, TestOutcome>,
): TestNode {
  if (node.case) {
    const outcome = liveOutcomeFor(node.case, results);
    const summary: TestSummary = {
      total: 1,
      passed: outcome === "passed" ? 1 : 0,
      failed: outcome === "failed" ? 1 : 0,
      skipped: outcome === "skipped" ? 1 : 0,
      other: outcome === "other" ? 1 : 0,
    };
    return { ...node, outcome, durationMs: null, summary };
  }

  const children = node.children.map((child) => applyLiveOutcomes(child, results));
  const summary = children.reduce<TestSummary>(
    (sum, child) => ({
      total: sum.total + child.summary.total,
      passed: sum.passed + child.summary.passed,
      failed: sum.failed + child.summary.failed,
      skipped: sum.skipped + child.summary.skipped,
      other: sum.other + child.summary.other,
    }),
    { total: 0, passed: 0, failed: 0, skipped: 0, other: 0 },
  );
  const outcome: TestOutcome =
    summary.failed > 0
      ? "failed"
      : summary.other > 0
        ? "other" // still running somewhere below
        : summary.passed > 0
          ? "passed"
          : "skipped";

  return { ...node, children, outcome, durationMs: null, summary };
}

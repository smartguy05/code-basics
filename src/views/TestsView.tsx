import { useEffect, useMemo, useRef, useState } from "react";
import {
  OutputConsole,
  stripAnsi,
  type ConsoleHandle,
} from "../components/OutputConsole";
import { TestTree } from "../components/TestTree";
import * as api from "../ipc/api";
import type { InspectRequest } from "../App";
import type {
  ProcessEvent,
  RunConfig,
  RunDump,
  TestCase,
  TestNode,
  TestOutcome,
  TestRunOutcome,
  TestSummary,
  Workspace,
} from "../ipc/types";

const OUTCOME_FILTERS: TestOutcome[] = ["passed", "failed", "skipped", "other"];

/** Provisional per-test counts read off the runner's console output. */
interface LiveCounts {
  passed: number;
  failed: number;
  skipped: number;
}

/**
 * Per-test result lines across the supported runners: VSTest's
 * `Passed Name [1 ms]` (console verbosity `normal`), MTP's lowercase
 * `failed Name`, Vitest's `✓ file > name`, Jest's per-file `PASS path`.
 * Summary lines like `Passed!  - Failed: 0, ...` do not match — the `!`
 * blocks the required whitespace after the word.
 */
const LIVE_LINE = /^\s*(passed|√|✓|✔|pass|failed|×|✗|✕|fail|skipped|↓|○|skip)\s+(\S.*)$/i;
const PASS_MARKS = new Set(["passed", "√", "✓", "✔", "pass"]);
const FAIL_MARKS = new Set(["failed", "×", "✗", "✕", "fail"]);

/** Read one console line as a test result, extracting the test's name. */
function classifyLine(line: string): { outcome: TestOutcome; name: string } | null {
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
function liveOutcomeFor(testCase: TestCase, results: Map<string, TestOutcome>): TestOutcome {
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
function applyLiveOutcomes(
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

export function TestsView({
  workspace,
  onInspect,
}: {
  workspace: Workspace;
  onInspect: (request: InspectRequest) => void;
}) {
  const testConfigs = workspace.configs.filter((c) => c.kind === "test");

  const [selectedConfig, setSelectedConfig] = useState<string | null>(
    testConfigs[0]?.id ?? null,
  );
  const [outcome, setOutcome] = useState<TestRunOutcome | null>(null);
  const [running, setRunning] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [filter, setFilter] = useState("");
  const [outcomeFilter, setOutcomeFilter] = useState<Set<TestOutcome>>(new Set());
  const [selectedNode, setSelectedNode] = useState<TestNode | null>(null);
  const [live, setLive] = useState<LiveCounts | null>(null);
  const [liveResults, setLiveResults] = useState<Map<string, TestOutcome>>(new Map());

  /** A dump this run produced, if one turned up. Null means offer nothing. */
  const [runDump, setRunDump] = useState<RunDump | null>(null);

  const consoleRef = useRef<ConsoleHandle>(null);
  /** Output chunks split lines anywhere; the tail carries over. */
  const partialLine = useRef("");

  /**
   * Look for a dump written while the run that started at `startedAt` was going.
   *
   * The test host writes its blame dump as it exits, so a dump older than the
   * run cannot have come from it; the host is still tearing down when
   * `runTests` returns, hence one retry. If nothing turns up — capture is off,
   * the runner writes no dump, or the file is not listed — no affordance is
   * shown at all.
   *
   * Nothing here attributes the dump to the run. A test run reports no pid for
   * the process that actually crashes (the test host is a grandchild), so the
   * backend can only ever return `certain: false`, and the affordance is worded
   * as a candidate rather than as this run's crash.
   */
  async function findRunDump(startedAt: number) {
    for (const delay of [0, 1500]) {
      if (delay > 0) await new Promise((resolve) => setTimeout(resolve, delay));
      try {
        const status = await api.inspectStatus();
        if (!status.available || !status.dumpCaptureEnabled) return;
        const found = await api.inspectRunDump(null, startedAt);
        if (found) {
          setRunDump(found);
          return;
        }
      } catch {
        return;
      }
    }
  }

  function trackLive(event: ProcessEvent) {
    if (event.type !== "output") return;
    const text = partialLine.current + event.text;
    const lines = text.split(/\r?\n/);
    partialLine.current = lines.pop() ?? "";

    const found = lines
      .map((raw) => classifyLine(stripAnsi(raw)))
      .filter((r): r is { outcome: TestOutcome; name: string } => r !== null);
    if (found.length === 0) return;

    setLive((previous) => {
      if (!previous) return previous;
      const next = { ...previous };
      for (const { outcome } of found) {
        if (outcome === "passed") next.passed += 1;
        else if (outcome === "failed") next.failed += 1;
        else next.skipped += 1;
      }
      return next;
    });
    setLiveResults((previous) => {
      const next = new Map(previous);
      for (const { name, outcome } of found) next.set(name, outcome);
      return next;
    });
  }

  /** The previous run's tree recoloured with this run's results so far. */
  const liveTree = useMemo(() => {
    if (!running || !outcome) return null;
    return outcome.tree.map((node) => applyLiveOutcomes(node, liveResults));
  }, [running, outcome, liveResults]);

  // Restore the previous run when switching configurations, so the tree does
  // not go blank just because the selection changed.
  useEffect(() => {
    setSelectedNode(null);
    if (!selectedConfig) {
      setOutcome(null);
      return;
    }
    let cancelled = false;
    api
      .lastTestRun(selectedConfig)
      .then((previous) => {
        if (!cancelled) setOutcome(previous);
      })
      .catch(() => {
        /* nothing recorded yet */
      });
    return () => {
      cancelled = true;
    };
  }, [selectedConfig]);

  async function run(onlyFailed: boolean) {
    if (!selectedConfig || running) return;

    setRunning(true);
    setError(null);
    // Show the console: a selected failure would otherwise cover it just as
    // the new activity starts.
    setSelectedNode(null);
    consoleRef.current?.clear();
    partialLine.current = "";
    setLive({ passed: 0, failed: 0, skipped: 0 });
    setLiveResults(new Map());
    // The previous run's dump describes a process that is gone.
    setRunDump(null);
    const startedAt = Math.floor(Date.now() / 1000);

    try {
      const result = await api.runTests(selectedConfig, onlyFailed, (event) => {
        consoleRef.current?.handle(event);
        trackLive(event);
      });
      setOutcome(result);
      setSelectedNode(null);
      if (result.result.summary.failed > 0) void findRunDump(startedAt);
    } catch (e) {
      setError(api.errorMessage(e));
    } finally {
      setRunning(false);
      setLive(null);
    }
  }

  async function cancel() {
    if (selectedConfig) await api.cancelRun(selectedConfig);
  }

  const failedCount = outcome?.result.summary.failed ?? 0;
  const selectedCase = selectedNode?.case ?? null;

  if (testConfigs.length === 0) {
    return (
      <div className="empty">
        No test projects were found in this workspace.
        <br />
        Add a test project and rescan, or define one in{" "}
        <code>.code-basics/config.json</code>.
      </div>
    );
  }

  return (
    <div className="main">
      <div className="toolbar">
        <select
          value={selectedConfig ?? ""}
          onChange={(e) => setSelectedConfig(e.target.value)}
          disabled={running}
        >
          {testConfigs.map((config: RunConfig) => (
            <option key={config.id} value={config.id}>
              {config.name}
            </option>
          ))}
        </select>

        <button className="primary" onClick={() => run(false)} disabled={running}>
          Run
        </button>
        <button
          onClick={() => run(true)}
          disabled={running || failedCount === 0}
          title={
            failedCount === 0
              ? "No failures from the last run of this configuration"
              : `Re-run ${failedCount} failed test${failedCount === 1 ? "" : "s"}`
          }
        >
          Re-run failed{failedCount > 0 ? ` (${failedCount})` : ""}
        </button>
        <button onClick={cancel} disabled={!running}>
          Stop
        </button>
        <button
          onClick={() => consoleRef.current?.clear()}
          title="Clear the console output"
        >
          Clear
        </button>

        {running && <span className="spinner" />}

        <span className="spacer" style={{ flex: 1 }} />

        <input
          placeholder="Filter tests"
          value={filter}
          onChange={(e) => setFilter(e.target.value)}
          style={{ width: 160 }}
        />

        {OUTCOME_FILTERS.map((value) => (
          <button
            key={value}
            className={outcomeFilter.has(value) ? "primary" : ""}
            onClick={() =>
              setOutcomeFilter((previous) => {
                const next = new Set(previous);
                if (next.has(value)) next.delete(value);
                else next.add(value);
                return next;
              })
            }
          >
            {value}
          </button>
        ))}
      </div>

      {running && live && (
        <div className="toolbar summary-counts">
          <span>
            <span className="dot passed" /> {live.passed} passed
          </span>
          <span>
            <span className="dot failed" /> {live.failed} failed
          </span>
          <span>
            <span className="dot skipped" /> {live.skipped} skipped
          </span>
          <span className="muted">
            live — the tree and exact counts land when the run finishes
          </span>
        </div>
      )}

      {outcome && !running && (
        <div className="toolbar summary-counts">
          <span>
            <span className="dot passed" /> {outcome.result.summary.passed} passed
          </span>
          <span>
            <span className="dot failed" /> {outcome.result.summary.failed} failed
          </span>
          <span>
            <span className="dot skipped" /> {outcome.result.summary.skipped} skipped
          </span>
          {outcome.result.durationMs != null && (
            <span className="muted">
              in {(outcome.result.durationMs / 1000).toFixed(2)}s
            </span>
          )}
        </div>
      )}

      {error && <div className="error">{error}</div>}
      {outcome?.warnings.map((warning) => (
        <div className="warning" key={warning}>
          {warning}
        </div>
      ))}

      <div className="content split">
        <div className="top">
          {running && !outcome ? (
            <div className="empty">
              <span className="spinner" style={{ display: "inline-block", marginRight: 8 }} />
              Running tests — the tree appears after the first run of this
              configuration…
            </div>
          ) : (
            <TestTree
              // While running, the previous run's tree recoloured live: grey
              // until each test reports, then green/red/yellow.
              nodes={liveTree ?? outcome?.tree ?? []}
              filter={filter}
              outcomes={outcomeFilter}
              selectedId={selectedNode?.id ?? null}
              onSelect={setSelectedNode}
            />
          )}
        </div>

        <div className="bottom">
          {/* The console stays mounted while a failure detail covers it —
              unmounting would drop streamed output and break clear(). */}
          {selectedCase && (
            <div className="failure-detail">
              <h3>{selectedCase.fullName}</h3>

              {/* Only for a failure, and only once a dump written during this
                  run has been found — see `findRunDump`. Two things are stated
                  rather than implied: nothing ties this dump to this run (a
                  test run reports no pid for the process that crashes, and
                  another configuration running at the same time is armed too),
                  and the dump is written when the test host exits, which is
                  after the failing test has finished and its locals have gone. */}
              {runDump && selectedNode?.outcome === "failed" && (
                <div className="toolbar">
                  <button
                    className="primary"
                    title={`Read ${runDump.dump.executable} · pid ${runDump.dump.pid}`}
                    onClick={() =>
                      onInspect({
                        target: { kind: "dump", path: runDump.dump.path },
                        root: { kind: "exceptions" },
                        reason: `${runDump.dump.executable} · pid ${runDump.dump.pid}, a dump written while this run was going — not confirmed to be ${selectedCase.fullName}'s failure`,
                      })
                    }
                  >
                    Inspect objects
                  </button>
                  <span className="muted" style={{ fontSize: 11 }}>
                    A dump was written while this run was going —{" "}
                    <span className="mono">
                      {runDump.dump.executable} · pid {runDump.dump.pid}
                    </span>
                    . Nothing confirms it came from this run rather than from
                    something else running at the same time. It holds every
                    exception still on the heap when the test host exited, which
                    is after this test finished, so anything that only lived
                    inside it may already have gone.
                  </span>
                </div>
              )}

              {selectedCase.message && (
                <>
                  <div className="muted">Message</div>
                  <pre>{selectedCase.message}</pre>
                </>
              )}
              {selectedCase.stackTrace && (
                <>
                  <div className="muted">Stack trace</div>
                  <pre>{selectedCase.stackTrace}</pre>
                </>
              )}
              {selectedCase.stdout && (
                <>
                  <div className="muted">Output</div>
                  <pre>{selectedCase.stdout}</pre>
                </>
              )}
              {!selectedCase.message &&
                !selectedCase.stackTrace &&
                !selectedCase.stdout && (
                  <div className="muted">This test produced no output.</div>
                )}
            </div>
          )}
          <div style={{ display: selectedCase ? "none" : "block", height: "100%" }}>
            <OutputConsole ref={consoleRef} />
          </div>
        </div>
      </div>
    </div>
  );
}

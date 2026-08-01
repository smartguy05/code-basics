import { useEffect, useRef, useState } from "react";
import { OutputConsole, type ConsoleHandle } from "../components/OutputConsole";
import { TestTree } from "../components/TestTree";
import * as api from "../ipc/api";
import type {
  RunConfig,
  TestNode,
  TestOutcome,
  TestRunOutcome,
  Workspace,
} from "../ipc/types";

const OUTCOME_FILTERS: TestOutcome[] = ["passed", "failed", "skipped", "other"];

export function TestsView({ workspace }: { workspace: Workspace }) {
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

  const consoleRef = useRef<ConsoleHandle>(null);

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
    consoleRef.current?.clear();

    try {
      const result = await api.runTests(selectedConfig, onlyFailed, (event) =>
        consoleRef.current?.handle(event),
      );
      setOutcome(result);
      setSelectedNode(null);
    } catch (e) {
      setError(api.errorMessage(e));
    } finally {
      setRunning(false);
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

      {outcome && (
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
          <TestTree
            nodes={outcome?.tree ?? []}
            filter={filter}
            outcomes={outcomeFilter}
            selectedId={selectedNode?.id ?? null}
            onSelect={setSelectedNode}
          />
        </div>

        <div className="bottom">
          {selectedCase ? (
            <div className="failure-detail">
              <h3>{selectedCase.fullName}</h3>
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
          ) : (
            <OutputConsole ref={consoleRef} />
          )}
        </div>
      </div>
    </div>
  );
}

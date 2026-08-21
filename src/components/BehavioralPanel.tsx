import { useEffect, useRef, useState } from "react";
import { OutputConsole, type ConsoleHandle } from "./OutputConsole";
import * as api from "../ipc/api";
import type { BehavioralReport, ProcessEvent } from "../ipc/types";
import { behavioralScoreLine, deltaLine } from "./behavioralPanelLogic";
import { behavioralReportToPromptContext } from "./claimVerifyLogic";
import {
  clampPanelPosition,
  loadPanelLayout,
  savePanelLayout,
  type PanelLayout,
  type PanelSize,
} from "./reviewLayoutLogic";

/**
 * The before/after run in its own floating window — the runtime twin of the
 * agent {@link ReviewPanel}.
 *
 * The run streams two full test/console/http passes (HEAD vs the working tree),
 * which is far too much to condense into the intent sidebar's one line. So it
 * runs here instead: a live console while the two sides execute, and the
 * assembled {@link BehavioralReport} laid out in full when they finish — the
 * report the sidebar could only hint at. Like the agent panel it is a floating,
 * draggable, minimizable window hosted at the app level, so the run survives a
 * tab switch, and it is non-modal so the rest of the app stays usable.
 *
 * `verify` chains the run into a claim check: when the before/after finishes,
 * its evidence is handed to `onVerify`, which opens the agent panel primed to
 * judge the diff's claims against it — the same evidence a reviewer reads here.
 * `onReport` feeds the finished report back so the intent cards can still show
 * their per-card before/after badges.
 */
export function BehavioralPanel({
  configId,
  verify,
  onReport,
  onVerify,
  onClose,
}: {
  configId: string;
  /** After the run, hand its evidence to the agent claim-verifier. */
  verify: boolean;
  /** The finished report, so the intent cards can badge each card's deltas. */
  onReport: (report: BehavioralReport) => void;
  /** Open the agent panel with the run's evidence (only used when `verify`). */
  onVerify: (context: string) => void;
  onClose: () => void;
}) {
  const consoleRef = useRef<ConsoleHandle>(null);
  const panelRef = useRef<HTMLDivElement>(null);

  const [pos, setPos] = useState<PanelLayout | undefined>(() => {
    const saved = loadPanelLayout(localStorage);
    return saved.left !== undefined && saved.top !== undefined ? saved : undefined;
  });
  const [size] = useState<PanelSize | undefined>(() => {
    const saved = loadPanelLayout(localStorage);
    return saved.width !== undefined && saved.height !== undefined
      ? { width: saved.width, height: saved.height }
      : undefined;
  });

  const [phase, setPhase] = useState<"running" | "done" | "error">("running");
  const [report, setReport] = useState<BehavioralReport | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [minimized, setMinimized] = useState(false);
  // The run ended while the window was minimized: flash the pill so a result
  // that arrived off-screen still gets noticed.
  const [attention, setAttention] = useState(false);

  const title = verify ? "Verify claims — evidence" : "Before / after";

  // Start the run once, on mount. A fresh open is a freshly-keyed panel (see
  // App), so this never re-fires for the same run.
  useEffect(() => {
    let alive = true;
    consoleRef.current?.clear();
    setPhase("running");
    setReport(null);
    setError(null);

    void api
      .behavioralDiff(configId, null, (event: ProcessEvent) => {
        // Route every event through the console's own renderer (colours, the
        // command banner, the exit line) — the same view the Run tab gives.
        consoleRef.current?.handle(event);
      })
      .then((result) => {
        if (!alive) return;
        setReport(result);
        setPhase("done");
        onReport(result);
        if (verify) onVerify(behavioralReportToPromptContext(result));
        // A result that landed while minimized needs a nudge; a visible window
        // already shows it.
        setMinimized((min) => {
          if (min) setAttention(true);
          return min;
        });
      })
      .catch((e) => {
        if (!alive) return;
        setError(api.errorMessage(e));
        setPhase("error");
        setMinimized((min) => {
          setAttention(true);
          return min;
        });
      });

    return () => {
      alive = false;
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  const restore = () => {
    setMinimized(false);
    setAttention(false);
  };

  // Drag the panel by its header (same pointer plumbing as the agent panel; the
  // clamp decision is the shared, tested `reviewLayoutLogic`). A press that
  // never moves stays a click, so the buttons keep working.
  const onHeaderPointerDown = (e: React.PointerEvent<HTMLDivElement>) => {
    if (e.button !== 0) return;
    if ((e.target as HTMLElement).closest("button")) return;
    const panel = panelRef.current;
    if (!panel) return;

    const rect = panel.getBoundingClientRect();
    const grabX = e.clientX - rect.left;
    const grabY = e.clientY - rect.top;
    const header = e.currentTarget;
    header.setPointerCapture(e.pointerId);

    let latest: PanelLayout = { left: rect.left, top: rect.top };
    let moved = false;
    const onMove = (ev: PointerEvent) => {
      moved = true;
      const s = { width: panel.offsetWidth, height: panel.offsetHeight };
      const viewport = { width: window.innerWidth, height: window.innerHeight };
      latest = clampPanelPosition({ left: ev.clientX - grabX, top: ev.clientY - grabY }, s, viewport);
      setPos(latest);
    };
    const onUp = () => {
      header.releasePointerCapture(e.pointerId);
      header.removeEventListener("pointermove", onMove);
      header.removeEventListener("pointerup", onUp);
      if (moved) savePanelLayout(localStorage, latest);
    };
    header.addEventListener("pointermove", onMove);
    header.addEventListener("pointerup", onUp);
  };

  const status =
    phase === "running"
      ? "Running both sides…"
      : phase === "error"
        ? "Run failed"
        : report
          ? behavioralScoreLine(report.scorecard)
          : "Finished";

  return (
    <>
      {minimized && (
        <button
          className={`review-pill${attention ? " attention" : ""}`}
          onClick={restore}
          title={attention ? "The before/after run finished" : "Restore the before/after window"}
        >
          {phase === "running" && <span className="review-spinner" aria-hidden />}
          <span>
            {title} — {attention ? "finished" : status}
          </span>
        </button>
      )}

      <div
        className="review-panel"
        hidden={minimized}
        ref={panelRef}
        style={{
          ...(pos ? { left: pos.left, top: pos.top, right: "auto", bottom: "auto" } : {}),
          ...(size ? { width: size.width, height: size.height } : {}),
        }}
      >
        <div className="review-header" onPointerDown={onHeaderPointerDown}>
          <strong>{title}</strong>
          <span className="faint" style={{ fontSize: 12 }}>
            {phase === "running" && (
              <span className="review-spinner" aria-hidden style={{ marginRight: 6 }} />
            )}
            {status}
          </span>
          <span style={{ flex: 1 }} />
          <button onClick={() => setMinimized(true)} title="Minimize (keeps running)">
            —
          </button>
          <button onClick={onClose} title="Close">
            ✕
          </button>
        </div>

        {error && <div className="warning">{error}</div>}

        <div className="review-console">
          <OutputConsole ref={consoleRef} />
        </div>

        {report && <BehavioralReportView report={report} verify={verify} />}
      </div>
    </>
  );
}

/** The assembled report, laid out in full below the console. */
function BehavioralReportView({
  report,
  verify,
}: {
  report: BehavioralReport;
  verify: boolean;
}) {
  const nothing =
    report.attributions.length === 0 &&
    report.unattributed.length === 0 &&
    report.warnings.length === 0;

  return (
    <div className="behavioral-report">
      <div className="behavioral-report-score">{behavioralScoreLine(report.scorecard)}</div>

      {report.tests && (
        <div className="behavioral-report-line">
          Tests: before {report.tests.summaryBefore.passed} passed /{" "}
          {report.tests.summaryBefore.failed} failed → after {report.tests.summaryAfter.passed}{" "}
          passed / {report.tests.summaryAfter.failed} failed
        </div>
      )}

      {report.attributions.length > 0 && (
        <div className="behavioral-report-section">
          <div className="behavioral-report-heading">Attributed to intent cards</div>
          {report.attributions.map((card) => (
            <div key={card.groupId} className="behavioral-report-card">
              <div className="faint" style={{ fontSize: 11 }}>
                card {card.groupId} · {card.confidence} confidence
              </div>
              {card.deltas.map((delta, i) => {
                const line = deltaLine(delta);
                return (
                  <div key={`${line.text}:${i}`} className={`behavioral-delta ${line.tone}`}>
                    {line.text}
                  </div>
                );
              })}
            </div>
          ))}
        </div>
      )}

      {report.unattributed.length > 0 && (
        <div className="behavioral-report-section">
          <div className="behavioral-report-heading">Unattributed differences</div>
          {report.unattributed.map((delta, i) => {
            const line = deltaLine(delta);
            return (
              <div key={`${line.text}:${i}`} className={`behavioral-delta ${line.tone}`}>
                {line.text}
              </div>
            );
          })}
        </div>
      )}

      {report.warnings.length > 0 && (
        <div className="behavioral-report-section">
          <div className="behavioral-report-heading">Could not be gathered</div>
          {report.warnings.map((warning) => (
            <div key={warning} className="warning" style={{ fontSize: 11 }}>
              {warning}
            </div>
          ))}
        </div>
      )}

      {nothing && (
        <div className="muted" style={{ fontSize: 12 }}>
          No observable before/after differences were detected.
          {verify && " The claim check will report against this."}
        </div>
      )}
    </div>
  );
}

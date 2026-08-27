import { useState } from "react";

/**
 * Confirmation shown right before a capture attaches to a *running* process —
 * the moment the pause-and-memory-spike cost is actually about to be paid,
 * replacing the always-on banner that used to sit over the Objects tab.
 *
 * A plain modal reusing the shared `modal-backdrop`/`modal`/`modal-body` markup
 * (as `SetupPrompt`/`RiderImportDialog` do) with a two-button `actions` footer
 * (as `PlanPreview`). The "Don't warn me again" checkbox is reported on confirm;
 * the decision to persist and re-gate lives in `inspectLogic`, not here — this
 * shell only renders and reports.
 */
export function AttachConfirm({
  onConfirm,
  onCancel,
}: {
  /** Proceed with the attach; `dontWarnAgain` is the checkbox state. */
  onConfirm: (dontWarnAgain: boolean) => void;
  onCancel: () => void;
}) {
  const [dontWarnAgain, setDontWarnAgain] = useState(false);

  return (
    <div className="modal-backdrop" onClick={onCancel}>
      <div
        className="modal"
        role="dialog"
        aria-modal="true"
        aria-label="Attach to running process?"
        onClick={(e) => e.stopPropagation()}
      >
        <div className="modal-body">
          <h3 style={{ marginTop: 0 }}>Attach to running process?</h3>
          <p>
            Capturing copies the process&apos;s memory: expect a brief pause of the order of a
            second and a memory spike roughly the size of its heap while the copy exists. On a
            machine already short of memory, that is a real cost — worth knowing before, not after.
          </p>
          <p>
            <strong>The application is not stopped.</strong> The suspending attach that freezes
            every thread is never requested, so a service being inspected keeps serving while it is
            read.
          </p>
          <label style={{ display: "flex", alignItems: "center", gap: 8, marginTop: 12 }}>
            <input
              type="checkbox"
              checked={dontWarnAgain}
              onChange={(e) => setDontWarnAgain(e.target.checked)}
            />
            Don&apos;t warn me again
          </label>
          <div className="actions" style={{ display: "flex", justifyContent: "flex-end", gap: 8, marginTop: 16 }}>
            <button onClick={onCancel}>Cancel</button>
            <button className="primary" onClick={() => onConfirm(dontWarnAgain)}>
              Attach
            </button>
          </div>
        </div>
      </div>
    </div>
  );
}

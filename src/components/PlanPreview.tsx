import type { InstallPlan } from "../ipc/types";

/**
 * The confirmation view for an {@link InstallPlan}: what will be written, with
 * the exact final contents of each file, before anything touches disk. Shared
 * by intent capture, the quality gate, and the first-open setup prompt — all
 * write outside the caller's control, into files a team shares, so the change is
 * always shown first.
 */
export function PlanPreview({
  plan,
  busy,
  onConfirm,
  onCancel,
  confirmLabel = "Write these files",
}: {
  plan: InstallPlan;
  busy: boolean;
  onConfirm: () => void;
  onCancel: () => void;
  confirmLabel?: string;
}) {
  return (
    <div style={{ marginTop: 8 }}>
      <div style={{ fontSize: 12, marginBottom: 4 }}>
        This will write {plan.writes.length} file
        {plan.writes.length === 1 ? "" : "s"}:
      </div>

      {plan.caveats?.map((caveat) => (
        <div key={caveat} className="warning" style={{ fontSize: 11, marginBottom: 4 }}>
          {caveat}
        </div>
      ))}

      {plan.writes.map((write) => (
        <details key={write.path}>
          <summary style={{ fontSize: 11, cursor: "pointer" }}>
            {write.path}
            {write.mergesExisting && (
              <span className="badge" style={{ marginLeft: 4 }}>
                merges into existing
              </span>
            )}
          </summary>
          <pre>{write.content}</pre>
        </details>
      ))}

      <div className="actions">
        <button disabled={busy} onClick={onConfirm}>
          {confirmLabel}
        </button>
        <button disabled={busy} onClick={onCancel}>
          Cancel
        </button>
      </div>
    </div>
  );
}

import { Component, type ReactNode } from "react";

interface ErrorBoundaryState {
  error: Error | null;
}

/**
 * Last line of defence: an uncaught error in render or an effect unmounts
 * React's whole tree, which the user experiences as the app going blank.
 * This shows the error instead, copyable for a bug report, without losing
 * the running backend (processes keep running; Reload just restarts the UI).
 */
export class ErrorBoundary extends Component<{ children: ReactNode }, ErrorBoundaryState> {
  state: ErrorBoundaryState = { error: null };

  static getDerivedStateFromError(error: Error): ErrorBoundaryState {
    return { error };
  }

  render() {
    const { error } = this.state;
    if (!error) return this.props.children;

    const detail = `${error.message}\n\n${error.stack ?? ""}`;

    return (
      <div className="empty" style={{ paddingTop: 80 }}>
        <h2 style={{ marginBottom: 4 }}>Something broke in the UI</h2>
        <p className="muted">
          The backend is fine — running processes keep running. Reload to get
          the interface back, and please report what you clicked along with
          this error.
        </p>

        <pre
          className="mono"
          style={{
            textAlign: "left",
            display: "inline-block",
            maxWidth: 640,
            maxHeight: 260,
            overflow: "auto",
            padding: 12,
            border: "1px solid var(--border-strong)",
            borderRadius: 4,
            background: "var(--bg-inset)",
            whiteSpace: "pre-wrap",
            wordBreak: "break-word",
          }}
        >
          {detail}
        </pre>

        <div style={{ marginTop: 12, display: "flex", gap: 8, justifyContent: "center" }}>
          <button onClick={() => void navigator.clipboard.writeText(detail)}>
            Copy error
          </button>
          <button className="primary" onClick={() => window.location.reload()}>
            Reload UI
          </button>
        </div>
      </div>
    );
  }
}

import { useState } from "react";
import type {
  SqlCandidate,
  SqlConnectionView,
  SqlDiscovery,
  SqlTestOutcome,
} from "../ipc/types";
import { groupConnections, statusLine } from "../views/sqlLogic";
import { ContextMenu } from "./ContextMenu";
import { candidateBlocker, describeDisplay, secretOrigin } from "./sqlPickerLogic";

/**
 * Pick a database to run against, or adopt one the workspace already mentions.
 *
 * Two populations, kept visibly apart, because they are different claims:
 *
 *  - **Saved connections** — things the user set up, grouped this-codebase-first
 *    like the launcher's recents. Selecting one is what the console runs on.
 *  - **Found in this codebase** — discovery candidates read out of
 *    `appsettings.json`, user secrets and `.env` files. These are *offered and
 *    never pre-selected*: the workspace naming a connection string is not the
 *    user asking to connect to it. Each shows the file and key it came from, and
 *    one whose value is still a variable reference, or whose engine nobody
 *    determined, is drawn as **not connectable** rather than as a row that will
 *    fail when clicked.
 *
 * **A connection string is never displayed.** Only the redacted
 * `SqlConnectionDisplay` the backend sends crosses IPC at all, and where its
 * `confidence` is `refused` this says so instead of showing a half-filled
 * description — a partially parsed string may be quoting a slice of a password.
 *
 * Purely presentational: every branch that carries a rule (the grouping, why a
 * candidate cannot be connected, how a display is worded) is in the tested
 * `sqlLogic`. Per-row actions use the shared `ContextMenu` rather than a fourth
 * hand-rolled copy.
 */
export interface SqlConnectionPickerProps {
  /** The active codebase root — the grouping key, nothing more. */
  root: string | null;
  connections: SqlConnectionView[];
  /** Null before the scan has run; a scan that found nothing is an empty list. */
  discovery: SqlDiscovery | null;
  discovering?: boolean;
  /** The saved connection currently in use, or null when none is chosen. */
  selectedId: string | null;
  onSelect: (id: string) => void;
  /** Save a discovered candidate as a connection. Only ever offered for a connectable one. */
  onAdopt: (candidate: SqlCandidate) => void;
  onTest: (connection: SqlConnectionView) => void;
  onDelete: (connection: SqlConnectionView) => void;
  /**
   * Consent to writes, which the backend accepts only through
   * `sql_set_allow_writes` — never as part of saving a profile.
   */
  onSetAllowWrites: (connection: SqlConnectionView, allowWrites: boolean) => void;
  onRefreshDiscovery: () => void;
  /**
   * The last connection test and which connection it was for. Worded by
   * `sqlLogic.statusLine`, which keeps all eleven outcomes apart — this
   * component never collapses them.
   */
  testOutcome?: { id: string; outcome: SqlTestOutcome } | null;
  /** A failure from a command this picker triggered. */
  error?: string | null;
  onClose: () => void;
}

/** Which row's context menu is open, and where. */
type MenuState = { connection: SqlConnectionView; x: number; y: number };

export function SqlConnectionPicker({
  root,
  connections,
  discovery,
  discovering = false,
  selectedId,
  onSelect,
  onAdopt,
  onTest,
  onDelete,
  onSetAllowWrites,
  onRefreshDiscovery,
  testOutcome = null,
  error = null,
  onClose,
}: SqlConnectionPickerProps) {
  const [menu, setMenu] = useState<MenuState | null>(null);

  const groups = groupConnections(connections, root);

  const savedRow = (connection: SqlConnectionView) => {
    const origin = secretOrigin(connection.secret);
    const tested =
      testOutcome !== null && testOutcome.id === connection.id
        ? statusLine(testOutcome.outcome)
        : null;
    return (
      <div
        className={`sql-conn-row${connection.id === selectedId ? " selected" : ""}`}
        key={connection.id}
        onClick={() => onSelect(connection.id)}
        onContextMenu={(e) => {
          e.preventDefault();
          setMenu({ connection, x: e.clientX, y: e.clientY });
        }}
      >
        <span className="sql-conn-name">{connection.name}</span>

        {connection.engine === null ? (
          <span className="sql-conn-meta sql-conn-unknown" title="No engine was determined for this connection.">
            engine not determined
          </span>
        ) : (
          <span className="badge">{connection.engine}</span>
        )}

        {/* Where the string is defined — a file and a key, which is not a
            secret, and is the only thing that can be said about a reference. */}
        {origin !== null && (
          <span className="sql-conn-meta" title={origin}>
            {origin}
          </span>
        )}

        {connection.holdsASecret && (
          <span
            className="sql-conn-meta sql-conn-holds-secret"
            title="This profile stores the connection string itself rather than pointing at a file."
          >
            stores a secret
          </span>
        )}

        {connection.allowWrites && (
          <span
            className="sql-conn-meta sql-conn-writes"
            title="Writes are allowed on this connection."
          >
            writes allowed
          </span>
        )}

        {tested !== null && (
          <span
            className={`sql-conn-test${tested.tone === "ok" ? " ok" : " bad"}`}
            title={tested.detail ?? undefined}
          >
            {tested.text}
          </span>
        )}

        <button
          className="sql-conn-action"
          title="Test this connection"
          onClick={(e) => {
            e.stopPropagation();
            onTest(connection);
          }}
        >
          Test
        </button>
      </div>
    );
  };

  const savedSection = (title: string, entries: SqlConnectionView[]) => {
    if (entries.length === 0) return null;
    return (
      <div className="sql-conn-section">
        <div className="sql-conn-section-title">{title}</div>
        {entries.map(savedRow)}
      </div>
    );
  };

  const candidates = discovery?.candidates ?? [];
  const discoveryWarnings = discovery?.warnings ?? [];

  const candidateRow = (candidate: SqlCandidate) => {
    const blocker = candidateBlocker(candidate.state);
    const described = describeDisplay(candidate.display);
    return (
      <div
        className={`sql-cand-row${blocker === null ? "" : " blocked"}`}
        key={candidate.id}
      >
        <span className="sql-cand-name">{candidate.name}</span>

        {/* The source file and key. This is what makes a discovered row
            checkable by the person reading it. */}
        <span className="sql-cand-origin" title={candidate.origin}>
          {candidate.origin}
        </span>
        {candidate.project !== null && (
          <span className="sql-cand-meta">{candidate.project}</span>
        )}

        {candidate.engine !== null && <span className="badge">{candidate.engine}</span>}

        {/* Never the connection string — only the redacted description, and
            only when the backend was willing to describe it. */}
        <span
          className={`sql-cand-display${described.refused ? " refused" : ""}`}
          title={described.text}
        >
          {described.text}
        </span>

        {blocker === null ? (
          <button
            className="sql-cand-action"
            title="Save this as a connection"
            onClick={() => onAdopt(candidate)}
          >
            Add
          </button>
        ) : (
          // Not a disabled Add button: there is nothing to connect *to* yet, and
          // saying why is the whole content of the row.
          <span className="sql-cand-blocked" title={blocker}>
            {blocker}
          </span>
        )}
      </div>
    );
  };

  return (
    <>
      <div className="sql-conn-overlay" onMouseDown={onClose}>
        <div className="sql-conn-picker" onMouseDown={(e) => e.stopPropagation()}>
          <div className="sql-conn-header">
            <strong>Connections</strong>
            <span style={{ flex: 1 }} />
            <button onClick={onRefreshDiscovery} title="Scan this codebase again">
              ↻ Rescan
            </button>
            <button onClick={onClose} title="Close">
              ✕
            </button>
          </div>

          {error !== null && <div className="sql-conn-error">{error}</div>}

          <div className="sql-conn-body">
            {connections.length === 0 ? (
              <div className="sql-conn-empty">
                No saved connections. Add one from what was found below, if anything was.
              </div>
            ) : (
              <>
                {savedSection("This codebase", groups.thisCodebase)}
                {savedSection("Not tied to a codebase", groups.global)}
                {savedSection("Other codebases", groups.otherCodebases)}
              </>
            )}

            <div className="sql-conn-section sql-cand-section">
              <div className="sql-conn-section-title">Found in this codebase</div>
              <div className="sql-cand-note">
                Read out of files in this workspace. Nothing here is selected, and nothing is
                connected to, until you add it.
              </div>

              {discovery === null ? (
                <div className="sql-cand-empty">
                  {discovering ? "Scanning…" : "Not scanned yet."}
                </div>
              ) : candidates.length === 0 ? (
                <div className="sql-cand-empty">Nothing found.</div>
              ) : (
                candidates.map(candidateRow)
              )}

              {/* Everything the scan saw and did not list. Hiding these makes a
                  partial scan look like a complete one. */}
              {discoveryWarnings.length > 0 && (
                <ul className="sql-cand-warnings">
                  {discoveryWarnings.map((warning, index) => (
                    <li key={index}>
                      <span className="sql-warn-icon">⚠</span> {warning}
                    </li>
                  ))}
                </ul>
              )}
            </div>
          </div>
        </div>
      </div>

      {menu !== null && (
        <ContextMenu x={menu.x} y={menu.y} onClose={() => setMenu(null)}>
          <div
            className="dropdown-item"
            onClick={() => {
              onTest(menu.connection);
              setMenu(null);
            }}
          >
            Test connection
          </div>
          <div
            className="dropdown-item"
            onClick={() => {
              onSetAllowWrites(menu.connection, !menu.connection.allowWrites);
              setMenu(null);
            }}
          >
            {menu.connection.allowWrites ? "Disallow writes" : "Allow writes"}
          </div>
          <div
            className="dropdown-item danger"
            onClick={() => {
              onDelete(menu.connection);
              setMenu(null);
            }}
          >
            Delete connection
          </div>
        </ContextMenu>
      )}
    </>
  );
}

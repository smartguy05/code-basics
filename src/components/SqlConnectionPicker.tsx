import { useState } from "react";
import type {
  SqlCandidate,
  SqlConnectionView,
  SqlDiscovery,
  SqlEngine,
  SqlTestOutcome,
} from "../ipc/types";
import { groupConnections, statusLine } from "../views/sqlLogic";
import { ContextMenu } from "./ContextMenu";
import {
  candidateBlocker,
  candidateConnectionLabel,
  candidateSourceDetail,
  describeDisplay,
  manualConnectionError,
  secretOrigin,
  savedConnectionLabel,
  type ManualConnectionDraft,
} from "./sqlPickerLogic";

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
  onAdopt: (candidate: SqlCandidate, engineOverride?: SqlEngine) => void;
  /** Test and, only on success, save a manually entered connection. */
  onAddManual: (draft: ManualConnectionDraft) => Promise<SqlTestOutcome>;
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
  onAddManual,
  onTest,
  onDelete,
  onSetAllowWrites,
  onRefreshDiscovery,
  testOutcome = null,
  error = null,
  onClose,
}: SqlConnectionPickerProps) {
  const [menu, setMenu] = useState<MenuState | null>(null);
  const [manualOpen, setManualOpen] = useState(false);
  const [manualDraft, setManualDraft] = useState<ManualConnectionDraft>({
    name: "",
    engine: null,
    connectionString: "",
    global: false,
  });
  const [showConnectionString, setShowConnectionString] = useState(false);
  const [addingManual, setAddingManual] = useState(false);
  const [manualOutcome, setManualOutcome] = useState<SqlTestOutcome | null>(null);
  const [candidateEngines, setCandidateEngines] = useState<Record<string, SqlEngine | "">>({});

  const groups = groupConnections(connections, root);
  const manualError = manualConnectionError(manualDraft);
  const manualStatus = manualOutcome === null ? null : statusLine(manualOutcome);

  const updateManual = (patch: Partial<ManualConnectionDraft>) => {
    setManualDraft((draft) => ({ ...draft, ...patch }));
    setManualOutcome(null);
  };

  const toggleManual = () => {
    if (manualOpen) {
      setManualDraft({ name: "", engine: null, connectionString: "", global: false });
      setShowConnectionString(false);
      setManualOutcome(null);
    }
    setManualOpen((open) => !open);
  };

  const submitManual = () => {
    if (manualError !== null || addingManual) return;
    setAddingManual(true);
    setManualOutcome(null);
    void onAddManual(manualDraft)
      .then(setManualOutcome)
      .catch(() => {})
      .finally(() => setAddingManual(false));
  };

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
        <span className="sql-conn-identity">
          <span className="sql-conn-name" title={savedConnectionLabel(connection)}>
            {savedConnectionLabel(connection)}
          </span>
          {origin !== null && (
            <span className="sql-conn-meta" title={origin}>
              {origin}
            </span>
          )}
        </span>

        {connection.engine === null ? (
          <span className="sql-conn-meta sql-conn-unknown" title="No engine was determined for this connection.">
            engine not determined
          </span>
        ) : (
          <span className="badge">{connection.engine}</span>
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
            <span>{tested.text}</span>
            {tested.detail !== null && <span className="sql-conn-test-detail">{tested.detail}</span>}
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
    const chosenEngine = candidateEngines[candidate.id] ?? "";
    const effectiveEngine = candidate.engine ?? (chosenEngine === "" ? null : chosenEngine);
    return (
      <div className="sql-cand-row" key={candidate.id}>
        <span className="sql-cand-identity">
          <span className="sql-cand-name" title={candidateConnectionLabel(candidate)}>
            {candidateConnectionLabel(candidate)}
          </span>
          <span className="sql-cand-origin" title={candidateSourceDetail(candidate)}>
            {candidateSourceDetail(candidate)}
          </span>
          <span
            className={`sql-cand-display${described.refused ? " refused" : ""}`}
            title={described.text}
          >
            {described.text}
          </span>
          {blocker !== null && (
            <span className="sql-cand-blocked" title={blocker}>
              {blocker}
            </span>
          )}
        </span>

        {candidate.engine !== null ? (
          <span className="badge">{candidate.engine}</span>
        ) : (
          <select
            className="sql-cand-engine"
            value={chosenEngine}
            title={blocker ?? undefined}
            aria-label={`Database engine for ${candidate.name}`}
            onChange={(event) =>
              setCandidateEngines((choices) => ({
                ...choices,
                [candidate.id]: event.target.value as SqlEngine | "",
              }))
            }
          >
            <option value="">Choose engine…</option>
            <option value="postgres">PostgreSQL</option>
            <option value="sqlServer">SQL Server</option>
            <option value="sqlite">SQLite</option>
          </select>
        )}
        <button
          className="sql-cand-action"
          title={
            candidate.state.kind === "ready"
              ? "Save this as a connection"
              : "Save this reference; it cannot connect until its value is populated"
          }
          disabled={effectiveEngine === null}
          onClick={() => {
            if (effectiveEngine !== null) onAdopt(candidate, effectiveEngine);
          }}
        >
          Add
        </button>
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
            <button onClick={toggleManual}>
              {manualOpen ? "Cancel add" : "+ Add connection"}
            </button>
            <button
              onClick={onRefreshDiscovery}
              title="Scan this codebase again"
              disabled={discovering}
            >
              {discovering ? "Scanning…" : "↻ Rescan"}
            </button>
            <button onClick={onClose} title="Close">
              ✕
            </button>
          </div>

          {error !== null && <div className="sql-conn-error">{error}</div>}

          {manualOpen && (
            <form
              className="sql-manual-form"
              onSubmit={(event) => {
                event.preventDefault();
                submitManual();
              }}
            >
              <div className="sql-conn-section-title">Add a connection manually</div>
              <label>
                Name
                <input
                  value={manualDraft.name}
                  onChange={(event) => updateManual({ name: event.target.value })}
                  autoFocus
                />
              </label>
              <label>
                Database engine
                <select
                  value={manualDraft.engine ?? ""}
                  onChange={(event) =>
                    updateManual({
                      engine:
                        event.target.value === ""
                          ? null
                          : (event.target.value as ManualConnectionDraft["engine"]),
                    })
                  }
                >
                  <option value="">Choose an engine…</option>
                  <option value="postgres">PostgreSQL</option>
                  <option value="sqlServer">SQL Server</option>
                  <option value="sqlite">SQLite</option>
                </select>
              </label>
              <label>
                Connection string
                <span className="sql-manual-secret">
                  <input
                    type={showConnectionString ? "text" : "password"}
                    value={manualDraft.connectionString}
                    onChange={(event) => updateManual({ connectionString: event.target.value })}
                    autoComplete="off"
                    spellCheck={false}
                  />
                  <button
                    type="button"
                    onClick={() => setShowConnectionString((shown) => !shown)}
                  >
                    {showConnectionString ? "Hide" : "Show"}
                  </button>
                </span>
              </label>
              <label className="sql-manual-scope">
                <input
                  type="checkbox"
                  checked={manualDraft.global}
                  onChange={(event) => updateManual({ global: event.target.checked })}
                />
                Make available to all codebases
              </label>
              <div className="sql-cand-note">
                The connection string is stored in your user configuration outside this codebase
                and is never displayed after saving. Writes start disabled.
              </div>
              {manualStatus !== null && (
                <div
                  className={`sql-conn-test${manualStatus.tone === "ok" ? " ok" : " bad"}`}
                  title={manualStatus.detail ?? undefined}
                >
                  <span>{manualStatus.text}</span>
                  {manualStatus.detail !== null && (
                    <span className="sql-conn-test-detail">{manualStatus.detail}</span>
                  )}
                </div>
              )}
              <div className="sql-manual-actions">
                <span>{manualError}</span>
                <button
                  type="submit"
                  className="primary"
                  disabled={manualError !== null || addingManual}
                >
                  {addingManual ? "Testing…" : "Test and add"}
                </button>
              </div>
            </form>
          )}

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
              {discovery !== null && !discovering && (
                <div className="sql-cand-note">
                  Latest scan found {candidates.length} candidate{candidates.length === 1 ? "" : "s"}.
                </div>
              )}

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
        <ContextMenu x={menu.x} y={menu.y} zIndex={302} onClose={() => setMenu(null)}>
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

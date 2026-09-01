import { useEffect, useRef, useState } from "react";
import { EditorState, type Extension } from "@codemirror/state";
import { drawSelection, dropCursor, EditorView, keymap, lineNumbers } from "@codemirror/view";
import { defaultKeymap, history, historyKeymap, indentWithTab } from "@codemirror/commands";
import { highlightSelectionMatches, search, searchKeymap } from "@codemirror/search";
import { editorColors } from "../components/language";
import { SqlConnectionPicker } from "../components/SqlConnectionPicker";
import {
  savedConnectionLabel,
  type ManualConnectionDraft,
} from "../components/sqlPickerLogic";
import { SqlResultGrid } from "../components/SqlResultGrid";
import * as api from "../ipc/api";
import type {
  SqlCandidate,
  SqlColumnView,
  SqlConnectionView,
  SqlDiscovery,
  SqlObjectView,
  SqlTestOutcome,
  Workspace,
} from "../ipc/types";
import {
  applyEvent,
  initialSqlState,
  runDisabledReason,
  statusLine,
  type SqlState,
  type SqlStatement,
} from "./sqlLogic";
import {
  enforcementBadge,
  keepSelection,
  mintQueryId,
  phaseLine,
  profileFromCandidate,
  profileFromManual,
  runTarget,
  selectedConnection,
  statementTitle,
  stopLine,
  stoppedNote,
  writesConfirm,
  type SqlPhaseLine,
  type StoppedNote,
  type WritesConfirm,
} from "./sqlViewLogic";

/**
 * The SQL console: a connection bar, an editor, and the results of the last run.
 *
 * # Why there is no SQL syntax highlighting
 *
 * `@codemirror/lang-sql` is not a dependency of this project and cannot be
 * installed here, so the editor is the repository's ordinary CodeMirror setup
 * with no language mode: line numbers, history, a drawn caret and selection,
 * and `@codemirror/search` so **Ctrl+F** finds within the query. That is a
 * deliberate trade — an editor with a working find and a visible caret is worth
 * more than coloured keywords, and inventing a highlighter by hand would be a
 * second, worse SQL parser sitting next to the guard's real one. Nothing about
 * the console's correctness depends on it.
 *
 * Two CodeMirror facts from `CLAUDE.md` apply and are honoured below: the caret
 * needs `&.cm-focused .cm-cursor { display: block }` or the WebView paints none,
 * and a chord must be bound **ahead of** `defaultKeymap` with `preventDefault`
 * or the WebView swallows it — which is why **Ctrl+Enter** (run) and
 * **Ctrl+Shift+Enter** (run selection) are listed first.
 *
 * # What this component does not decide
 *
 * Everything with a rule in it is elsewhere and tested: the streamed reducer,
 * the cells, the cap notice and the Run guard are `views/sqlLogic.ts`; the
 * read-only badge, the consent copy, what a Run press submits, the phase and
 * stop wording and the candidate-to-profile conversion are
 * `views/sqlViewLogic.ts`. In particular this file never re-words a
 * `SqlValue`, a `SqlTestOutcome` or a `SqlStopOutcome` — the backend went to
 * real trouble to keep those answers apart.
 */
export function SqlView({ workspace }: { workspace: Workspace }) {
  const [connections, setConnections] = useState<SqlConnectionView[]>([]);
  const [selectedId, setSelectedId] = useState<string | null>(null);
  const [pickerOpen, setPickerOpen] = useState(false);
  const [discovery, setDiscovery] = useState<SqlDiscovery | null>(null);
  const [discovering, setDiscovering] = useState(false);
  const [testOutcome, setTestOutcome] = useState<{
    id: string;
    outcome: SqlTestOutcome;
  } | null>(null);
  const [confirm, setConfirm] = useState<{
    connection: SqlConnectionView;
    next: boolean;
    copy: WritesConfirm;
  } | null>(null);
  const [objects, setObjects] = useState<SqlObjectView[]>([]);
  const [objectsLoading, setObjectsLoading] = useState(false);
  const [objectsError, setObjectsError] = useState<string | null>(null);
  const [explorerOpen, setExplorerOpen] = useState(true);
  const [explorerWidth, setExplorerWidth] = useState(250);
  const explorerDragRef = useRef<{ startX: number; startWidth: number } | null>(null);
  const [tableColumns, setTableColumns] = useState<
    Record<string, { expanded: boolean; loading: boolean; error: string | null; columns: SqlColumnView[]; openColumns: string[] }>
  >({});

  const [state, setState] = useState<SqlState>(initialSqlState);
  /**
   * Why the last Run press did nothing, or what the last Stop did. Shown rather
   * than swallowed: a click that silently does nothing is indistinguishable
   * from the app being broken.
   */
  const [notice, setNotice] = useState<SqlPhaseLine | null>(null);
  const [error, setError] = useState<string | null>(null);

  const connection = selectedConnection(connections, selectedId);
  const badge = enforcementBadge(connection);

  const loadObjects = (connectionId: string) => {
    setObjectsLoading(true);
    setObjectsError(null);
    setTableColumns({});
    return api
      .sqlListObjects(connectionId)
      .then(setObjects)
      .catch((e) => {
        setObjects([]);
        setObjectsError(api.errorMessage(e));
      })
      .finally(() => setObjectsLoading(false));
  };

  const tableKey = (object: SqlObjectView) =>
    `${object.kind}:${object.schema ?? ""}:${object.name}`;

  const toggleTable = (object: SqlObjectView) => {
    if (connection === null) return;
    const key = tableKey(object);
    const current = tableColumns[key];
    if (current !== undefined) {
      setTableColumns((all) => ({ ...all, [key]: { ...current, expanded: !current.expanded } }));
      return;
    }
    setTableColumns((all) => ({
      ...all,
      [key]: { expanded: true, loading: true, error: null, columns: [], openColumns: [] },
    }));
    api
      .sqlListColumns(connection.id, object.schema, object.name)
      .then((columns) =>
        setTableColumns((all) => ({
          ...all,
          [key]: { expanded: true, loading: false, error: null, columns, openColumns: [] },
        })),
      )
      .catch((e) =>
        setTableColumns((all) => ({
          ...all,
          [key]: {
            expanded: true,
            loading: false,
            error: api.errorMessage(e),
            columns: [],
            openColumns: [],
          },
        })),
      );
  };

  const toggleColumn = (key: string, name: string) => {
    setTableColumns((all) => {
      const table = all[key];
      if (table === undefined) return all;
      const openColumns = table.openColumns.includes(name)
        ? table.openColumns.filter((column) => column !== name)
        : [...table.openColumns, name];
      return { ...all, [key]: { ...table, openColumns } };
    });
  };

  useEffect(() => {
    if (selectedId === null) {
      setObjects([]);
      setObjectsError(null);
      return;
    }
    void loadObjects(selectedId);
    // Loading is intentionally keyed only by the saved profile identity.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [selectedId]);

  // ------------------------------------------------------------------
  // The editor
  // ------------------------------------------------------------------

  const hostRef = useRef<HTMLDivElement | null>(null);
  const viewRef = useRef<EditorView | null>(null);
  const [sql, setSql] = useState("");

  // The chords fire from inside CodeMirror, which captured its extensions once
  // on mount. Everything they need therefore reads through a ref rather than a
  // closed-over value, or Ctrl+Enter would run against whichever connection was
  // selected when the editor was created.
  const runRef = useRef<(mode: "all" | "selection") => void>(() => {});

  useEffect(() => {
    if (hostRef.current === null) return;

    const extensions: Extension[] = [
      lineNumbers(),
      history(),
      drawSelection(),
      dropCursor(),
      search({ top: true }),
      highlightSelectionMatches(),
      keymap.of([
        // Ahead of `defaultKeymap` and with `preventDefault`, for the reason
        // Ctrl+/ is in `FileEditor`: the WebView claims the chord otherwise and
        // CodeMirror never sees it.
        {
          key: "Mod-Enter",
          preventDefault: true,
          run: () => {
            runRef.current("all");
            return true;
          },
        },
        {
          key: "Mod-Shift-Enter",
          preventDefault: true,
          run: () => {
            runRef.current("selection");
            return true;
          },
        },
        ...searchKeymap,
        indentWithTab,
        ...defaultKeymap,
        ...historyKeymap,
      ]),
      EditorView.updateListener.of((update) => {
        if (update.docChanged) setSql(update.state.doc.toString());
      }),
      EditorView.theme({
        "&": { height: "100%" },
        ".cm-scroller": { overflow: "auto" },
        // The drawn caret. `drawSelection()` hides the native one, and
        // CodeMirror's own `.cm-cursor` stays `display: none` until a strict
        // focused child-combinator chain matches — which does not hold in this
        // WebView. The looser two-class selector below outranks that hide.
        ".cm-cursor, .cm-dropCursor": {
          borderLeftColor: "var(--text)",
          borderLeftWidth: "2px",
        },
        "&.cm-focused .cm-cursor": {
          display: "block",
          borderLeftColor: "var(--text)",
          borderLeftWidth: "2px",
        },
        "&.cm-focused .cm-selectionBackground, .cm-selectionBackground": {
          backgroundColor: "rgba(90, 120, 220, 0.35)",
        },
      }),
      ...editorColors,
    ];

    const view = new EditorView({
      state: EditorState.create({ doc: "", extensions }),
      parent: hostRef.current,
    });
    viewRef.current = view;
    return () => {
      view.destroy();
      viewRef.current = null;
    };
  }, []);

  // ------------------------------------------------------------------
  // Connections
  // ------------------------------------------------------------------

  const loadConnections = () =>
    api
      .sqlListConnections()
      .then((rows) => {
        setConnections(rows);
        // A connection deleted elsewhere must not stay selected: the bar would
        // keep naming a database while the next Run failed against an id the
        // store has forgotten.
        setSelectedId((current) => keepSelection(rows, current));
        setError(null);
      })
      .catch((e) => setError(api.errorMessage(e)));

  useEffect(() => {
    void loadConnections();
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  const refreshDiscovery = () => {
    setDiscovering(true);
    api
      .sqlDiscover(workspace.root)
      .then((found) => {
        setDiscovery(found);
        setError(null);
      })
      .catch((e) => setError(api.errorMessage(e)))
      .finally(() => setDiscovering(false));
  };

  // Scanned when the picker is first opened rather than on mount: discovery
  // reads files, and a tab nobody has looked at should not be walking the
  // workspace.
  useEffect(() => {
    if (pickerOpen && discovery === null && !discovering) refreshDiscovery();
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [pickerOpen]);

  const adopt = (candidate: SqlCandidate, engineOverride?: NonNullable<SqlConnectionView["engine"]>) => {
    const profile = profileFromCandidate(
      candidate,
      workspace.root,
      connections.map((c) => c.id),
      Date.now(),
      engineOverride,
    );
    api
      .sqlSaveConnection(profile)
      .then((rows) => {
        setConnections(rows);
        setSelectedId(profile.id);
        setError(null);
      })
      .catch((e) => setError(api.errorMessage(e)));
  };

  const addManual = async (draft: ManualConnectionDraft): Promise<SqlTestOutcome> => {
    if (draft.engine === null) {
      throw new Error("Choose a database engine.");
    }
    try {
      const outcome = await api.sqlTestConnectionString(draft.engine, draft.connectionString);
      if (outcome.kind !== "ok") {
        setError(null);
        return outcome;
      }
      const profile = profileFromManual(
        { ...draft, engine: draft.engine },
        workspace.root,
        connections.map((connection) => connection.id),
        Date.now(),
      );
      const rows = await api.sqlSaveConnection(profile);
      setConnections(rows);
      setSelectedId(profile.id);
      setPickerOpen(false);
      setError(null);
      return outcome;
    } catch (cause) {
      setError(api.errorMessage(cause));
      throw cause;
    }
  };

  const test = (target: SqlConnectionView) =>
    api
      .sqlTestConnection(target.id)
      .then((outcome) => {
        setTestOutcome({ id: target.id, outcome });
        setError(null);
      })
      .catch((e) => setError(api.errorMessage(e)));

  const remove = (target: SqlConnectionView) =>
    api
      .sqlDeleteConnection(target.id)
      .then((rows) => {
        setConnections(rows);
        setSelectedId((current) => keepSelection(rows, current));
      })
      .catch((e) => setError(api.errorMessage(e)));

  /**
   * The consent action. Never applied straight from a click: turning writes on
   * is routed through a confirmation that says what the guard is and is not,
   * and — on SQLite — what stronger protection is being given up. Turning them
   * back off needs no confirmation (`writesConfirm` returns null) and is applied
   * immediately.
   */
  const requestAllowWrites = (target: SqlConnectionView, next: boolean) => {
    const copy = writesConfirm(target, next);
    if (copy === null) {
      void applyAllowWrites(target, next);
      return;
    }
    setConfirm({ connection: target, next, copy });
  };

  const applyAllowWrites = (target: SqlConnectionView, next: boolean) =>
    api
      .sqlSetAllowWrites(target.id, next)
      .then((rows) => {
        setConnections(rows);
        setError(null);
      })
      .catch((e) => setError(api.errorMessage(e)));

  // ------------------------------------------------------------------
  // Running
  // ------------------------------------------------------------------

  const queryIdRef = useRef<string | null>(null);
  const seqRef = useRef(0);
  // The run reads the live phase, not the one captured when the chord was
  // bound: `runDisabledReason` refuses a second run while one is in flight.
  const stateRef = useRef(state);
  stateRef.current = state;

  const run = (mode: "all" | "selection") => {
    const view = viewRef.current;
    const full = view === null ? sql : view.state.doc.toString();
    const range = view?.state.selection.main;
    const selection =
      view === null || range === undefined ? "" : view.state.sliceDoc(range.from, range.to);

    const target = runTarget(mode, full, selection);
    if (target.kind === "refused") {
      setNotice({ tone: "warn", text: target.reason });
      return;
    }

    const active = selectedConnection(connections, selectedId);
    const blocked = runDisabledReason({
      connection: active,
      sql: target.sql,
      phase: stateRef.current.phase,
    });
    if (blocked !== null || active === null) {
      setNotice({ tone: "warn", text: blocked ?? "Choose a connection first." });
      return;
    }

    // Minted here because the first streamed events land before `invoke`
    // resolves — an id that arrived with the promise would identify rows that
    // had already been delivered, and Stop would have nothing to aim at during
    // exactly the window a user reaches for it.
    seqRef.current += 1;
    const queryId = mintQueryId(seqRef.current, Date.now());
    queryIdRef.current = queryId;
    setNotice(null);
    setState({ ...initialSqlState(), phase: { kind: "running" } });

    void api
      .sqlExecute(queryId, active.id, target.sql, (event) =>
        setState((current) => applyEvent(current, event)),
      )
      .catch((e) =>
        // A rejected `invoke` is a failure that never became an event, so it is
        // folded in as one rather than dropped beside the run: the phase must
        // not sit at `running` for a query that will never report.
        setState((current) =>
          applyEvent(current, {
            kind: "failed",
            statementIndex: null,
            message: api.errorMessage(e),
          }),
        ),
      )
      .finally(() => {
        if (queryIdRef.current === queryId) queryIdRef.current = null;
      });
  };
  runRef.current = run;

  const stop = () => {
    const queryId = queryIdRef.current;
    if (queryId === null) {
      setNotice({ tone: "idle", text: "Nothing is running." });
      return;
    }
    api
      .sqlCancel(queryId)
      .then((outcome) => setNotice(stopLine(outcome)))
      .catch((e) => setError(api.errorMessage(e)));
  };

  const running = state.phase.kind === "running";
  const disabled = runDisabledReason({ connection, sql, phase: state.phase });
  const phase = phaseLine(state.phase);
  const tested =
    testOutcome !== null && connection !== null && testOutcome.id === connection.id
      ? statusLine(testOutcome.outcome)
      : null;

  return (
    <div className="sql-view">
      <div className="sql-bar">
        <button data-command="sql.connections" className="sql-conn-button" onClick={() => setPickerOpen(true)}>
          {connection === null ? "Choose a connection…" : savedConnectionLabel(connection)}
        </button>

        {connection !== null && connection.engine !== null && (
          <span className="badge">{connection.engine}</span>
        )}

        {/* What is actually standing between the user and a write. Four
            renderings and not two: a driver-enforced read-only connection is a
            stronger promise than the guard's text check, and the two must be
            tellable apart at a glance. */}
        {badge !== null && (
          <span className={`sql-enforcement sql-enforcement-${badge.tone}`} title={badge.detail}>
            {badge.label}
          </span>
        )}

        {connection !== null && (
          <button
            data-command="sql.toggle-writes"
            className="sql-writes-toggle"
            onClick={() => requestAllowWrites(connection, !connection.allowWrites)}
            title={
              connection.allowWrites
                ? "Disallow writes on this connection"
                : "Allow writes on this connection — you will be asked to confirm"
            }
          >
            {connection.allowWrites ? "Disallow writes" : "Allow writes…"}
          </button>
        )}

        {connection !== null && (
          <button onClick={() => void test(connection)} title="Open the connection and close it">
            Test
          </button>
        )}

        <button
          onClick={() => setExplorerOpen((open) => !open)}
          disabled={connection === null}
          title="Show or hide database objects"
        >
          {explorerOpen ? "Hide objects" : "Objects"}
        </button>

        <span className="sql-bar-spacer" />

        <button
          data-command="sql.run"
          className="primary"
          onClick={() => run("all")}
          disabled={disabled !== null}
          title={disabled ?? "Run everything in the editor (Ctrl+Enter)"}
        >
          Run
        </button>
        <button
          onClick={() => run("selection")}
          disabled={running || connection === null}
          title="Run only the selected text (Ctrl+Shift+Enter)"
        >
          Run selection
        </button>
        <button data-command="sql.stop" onClick={stop} disabled={!running} title="Stop reading and drop the connection">
          Stop
        </button>
      </div>

      {/* The disabled Run button's reason, spelled out. A greyed control that
          cannot say why is a dead end — and each reason has a different next
          action. */}
      {disabled !== null && <div className="sql-bar-reason">{disabled}</div>}

      {badge !== null && <div className={`sql-bar-detail sql-tone-${badge.tone}`}>{badge.detail}</div>}

      {tested !== null && (
        <div className={`sql-bar-detail sql-tone-${tested.tone}`} title={tested.detail ?? undefined}>
          {tested.text}
        </div>
      )}

      {error !== null && <div className="sql-bar-detail sql-tone-error">{error}</div>}

      <div className="sql-work-area">
        {explorerOpen && connection !== null && (
          <aside
            className="sql-object-explorer"
            aria-label="Database object explorer"
            style={{ width: explorerWidth }}
          >
            <div className="sql-object-header">
              <div className="sql-object-title">
                <span className="sql-object-title-icon" aria-hidden="true" />
                <div>
                  <strong>Database objects</strong>
                  <span>{connection.name}</span>
                </div>
              </div>
              <button
                className="sql-object-refresh"
                onClick={() => void loadObjects(connection.id)}
                disabled={objectsLoading}
                title="Refresh database objects"
              >
                <span aria-hidden="true">&#8635;</span>
                <span>{objectsLoading ? "Loading..." : "Refresh"}</span>
              </button>
            </div>
            <div className="sql-object-kind">
              <span>Tables</span>
              {!objectsLoading && objectsError === null && <span className="sql-object-count">{objects.length}</span>}
            </div>
            {objectsError !== null && <div className="sql-object-error">{objectsError}</div>}
            {!objectsLoading && objectsError === null && objects.length === 0 && (
              <div className="sql-object-empty">No tables found.</div>
            )}
            <div className="sql-object-list">
              {objects.map((object) => {
                const qualified = object.schema === null ? object.name : `${object.schema}.${object.name}`;
                const key = tableKey(object);
                const table = tableColumns[key];
                return (
                  <div className="sql-object-node" key={key}>
                    <button
                      className="sql-object-row"
                      title={qualified}
                      aria-expanded={table?.expanded === true}
                      onClick={() => toggleTable(object)}
                    >
                      <span className="sql-object-chevron" data-open={table?.expanded === true} aria-hidden="true" />
                      <span className="sql-object-icon" aria-hidden="true" />
                      <span className="sql-object-label">
                        {object.schema !== null && <span className="sql-object-schema">{object.schema}.</span>}
                        <span>{object.name}</span>
                      </span>
                    </button>
                    {table?.expanded && (
                      <div className="sql-column-list">
                        {table.loading && <div className="sql-object-empty">Loading columns...</div>}
                        {table.error !== null && <div className="sql-object-error">{table.error}</div>}
                        {!table.loading && table.error === null && table.columns.length === 0 && (
                          <div className="sql-object-empty">No columns found.</div>
                        )}
                        {table.columns.map((column) => {
                          const open = table.openColumns.includes(column.name);
                          return (
                            <div className="sql-column-node" key={`${column.ordinal}:${column.name}`}>
                              <button
                                className="sql-column-row"
                                aria-expanded={open}
                                onClick={() => toggleColumn(key, column.name)}
                              >
                                <span className="sql-object-chevron" data-open={open} aria-hidden="true" />
                                <span className="sql-column-icon" aria-hidden="true" />
                                <span className="sql-column-name">{column.name}</span>
                              </button>
                              {open && <ColumnDetails column={column} />}
                            </div>
                          );
                        })}
                      </div>
                    )}
                  </div>
                );
              })}
            </div>
            <div
              className="sql-object-resizer"
              role="separator"
              aria-label="Resize database object explorer"
              aria-orientation="vertical"
              aria-valuemin={170}
              aria-valuemax={600}
              aria-valuenow={explorerWidth}
              tabIndex={0}
              onPointerDown={(event) => {
                explorerDragRef.current = { startX: event.clientX, startWidth: explorerWidth };
                event.currentTarget.setPointerCapture(event.pointerId);
              }}
              onPointerMove={(event) => {
                const drag = explorerDragRef.current;
                if (drag === null) return;
                setExplorerWidth(Math.min(600, Math.max(170, drag.startWidth + event.clientX - drag.startX)));
              }}
              onPointerUp={(event) => {
                explorerDragRef.current = null;
                event.currentTarget.releasePointerCapture(event.pointerId);
              }}
              onPointerCancel={() => {
                explorerDragRef.current = null;
              }}
              onKeyDown={(event) => {
                if (event.key !== "ArrowLeft" && event.key !== "ArrowRight") return;
                event.preventDefault();
                setExplorerWidth((width) =>
                  Math.min(600, Math.max(170, width + (event.key === "ArrowLeft" ? -16 : 16))),
                );
              }}
            />
          </aside>
        )}

        <div className="sql-console-main">
          <div className="sql-editor" ref={hostRef} />

          <div className="sql-results">
        <div className="sql-results-bar">
          <span className={`sql-phase sql-tone-${phase.tone}`}>{phase.text}</span>
          {notice !== null && (
            <span className={`sql-phase sql-tone-${notice.tone}`}>{notice.text}</span>
          )}
        </div>

        {/* A connection that never opened has no statement to hang its failure
            on, so it is reported on its own rather than beside a result. */}
        {state.connectionError !== null && (
          <div className="sql-statement-error">{state.connectionError}</div>
        )}

        {state.statements.length === 0 && state.connectionError === null && (
          <div className="sql-results-empty">
            Nothing to show yet. Ctrl+Enter runs the editor; Ctrl+Shift+Enter runs the selection.
          </div>
        )}

        {state.statements.map((statement) => (
          <StatementResult
            key={statement.statementIndex}
            statement={statement}
            running={running}
            /* The stop verdict is a property of the *run*, so the phase and the
               last index are what `stoppedNote` needs; the grid is told the
               answer, never left to work it out. */
            stopped={stoppedNote(
              state.phase,
              statement.statementIndex,
              state.statements[state.statements.length - 1]?.statementIndex ?? statement.statementIndex,
            )}
          />
        ))}
          </div>
        </div>
      </div>

      {pickerOpen && (
        <SqlConnectionPicker
          root={workspace.root}
          connections={connections}
          discovery={discovery}
          discovering={discovering}
          selectedId={selectedId}
          onSelect={(id) => {
            setSelectedId(id);
            setPickerOpen(false);
          }}
          onAdopt={adopt}
          onAddManual={addManual}
          onTest={(target) => void test(target)}
          onDelete={(target) => void remove(target)}
          onSetAllowWrites={requestAllowWrites}
          onRefreshDiscovery={refreshDiscovery}
          testOutcome={testOutcome}
          error={error}
          onClose={() => setPickerOpen(false)}
        />
      )}

      {confirm !== null && (
        <WritesConfirmModal
          copy={confirm.copy}
          onCancel={() => setConfirm(null)}
          onConfirm={() => {
            void applyAllowWrites(confirm.connection, confirm.next);
            setConfirm(null);
          }}
        />
      )}
    </div>
  );
}

function ColumnDetails({ column }: { column: SqlColumnView }) {
  const size =
    column.maxLength !== null
      ? `length ${column.maxLength}`
      : column.numericPrecision !== null
        ? `precision ${column.numericPrecision}${column.numericScale === null ? "" : `, scale ${column.numericScale}`}`
        : null;
  return (
    <dl className="sql-column-details">
      <div><dt>Type</dt><dd><code>{column.dataType}</code></dd></div>
      <div><dt>Nullable</dt><dd><span className={`sql-column-badge ${column.nullable ? "is-nullable" : ""}`}>{column.nullable === null ? "Not reported" : column.nullable ? "Yes" : "No"}</span></dd></div>
      <div><dt>Position</dt><dd>{column.ordinal}</dd></div>
      {size !== null && <div><dt>Size</dt><dd>{size}</dd></div>}
      {column.primaryKey !== null && <div><dt>Primary key</dt><dd><span className={`sql-column-badge ${column.primaryKey ? "is-key" : ""}`}>{column.primaryKey ? "Yes" : "No"}</span></dd></div>}
      {column.defaultValue !== null && <div><dt>Default</dt><dd><code>{column.defaultValue}</code></dd></div>}
    </dl>
  );
}

/**
 * One statement's result: its notices, its refusal or its error, then its grid.
 *
 * The three are rendered separately and never as one "problem" line, because
 * they are three different facts: a notice means the statement is running (an
 * allowed write, say), a refusal means nothing was sent, and an error means the
 * database answered with one.
 */
function StatementResult({
  statement,
  running,
  stopped,
}: {
  statement: SqlStatement;
  running: boolean;
  /** `stoppedNote`'s verdict for this result set — see its doc for who gets one. */
  stopped: StoppedNote | null;
}) {
  const title = statementTitle(statement.statementIndex);
  return (
    <div className="sql-statement">
      {statement.notices.map((message, index) => (
        <div className="sql-statement-notice" key={index}>
          {message}
        </div>
      ))}

      {statement.refusal !== null && (
        <div className="sql-statement-refusal">
          <strong>{title} was not sent.</strong> {statement.refusal}
        </div>
      )}

      {statement.error !== null && (
        <div className="sql-statement-error">
          <strong>{title} failed.</strong> {statement.error}
        </div>
      )}

      {/* `columns === null` means no `columns` event has arrived — not that the
          statement had none. A grid drawn from that would be premature, so the
          absence is stated instead. */}
      {statement.columns === null ? (
        statement.refusal === null &&
        statement.error === null && (
          <div className="sql-results-empty">
            {title}: {running ? "waiting for the first rows…" : "no columns were reported."}
            {/* There is no grid to carry the stop, so it is said here — a
                stopped statement that never reported columns must not look
                like one that legitimately had none. */}
            {stopped !== null && <span className="sql-grid-stopped-note"> {stopped.header}</span>}
          </div>
        )
      ) : (
        <SqlResultGrid
          columns={statement.columns}
          rows={statement.rows}
          rowCap={statement.completion?.rowCap ?? null}
          stopped={stopped}
          rowsAffected={statement.completion?.rowsAffected ?? null}
          elapsedMs={statement.completion?.elapsedMs ?? null}
          title={title}
          running={running && statement.completion === null}
        />
      )}
    </div>
  );
}

/**
 * The consent modal, modelled on `AttachConfirm`: the shared
 * `modal-backdrop`/`modal`/`modal-body` markup with a two-button footer.
 *
 * Every word of it comes from `writesConfirm`, so what the guard is — and, on
 * SQLite, what is being given up — is a tested decision rather than a literal
 * in a view that could drift into claiming more than the backend does.
 */
function WritesConfirmModal({
  copy,
  onConfirm,
  onCancel,
}: {
  copy: WritesConfirm;
  onConfirm: () => void;
  onCancel: () => void;
}) {
  return (
    <div className="modal-backdrop" onClick={onCancel}>
      <div
        className="modal"
        role="dialog"
        aria-modal="true"
        aria-label={copy.title}
        onClick={(e) => e.stopPropagation()}
      >
        <div className="modal-body">
          <h3 style={{ marginTop: 0 }}>{copy.title}</h3>
          <p>{copy.guard}</p>
          {copy.driverGiveUp !== null && (
            <p>
              {/* Both halves come from `writesConfirm`. The lead sentence used
                  to be a literal here and was the one string in this modal
                  nothing pinned — and it rendered for any engine with a driver
                  give-up, so a weaker future guarantee would have inherited the
                  word "protected" untested. */}
              {copy.driverGiveUpLead !== null && <strong>{copy.driverGiveUpLead}</strong>}{" "}
              {copy.driverGiveUp}
            </p>
          )}
          <div className="actions sql-confirm-actions">
            <button onClick={onCancel}>Cancel</button>
            <button className="primary danger" onClick={onConfirm}>
              {copy.confirmLabel}
            </button>
          </div>
        </div>
      </div>
    </div>
  );
}

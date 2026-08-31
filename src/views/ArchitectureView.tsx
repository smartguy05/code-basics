import { useCallback, useEffect, useRef, useState } from "react";
import { Sidebar } from "../components/Sidebar";
import { DiagramCanvas } from "./architecture/DiagramCanvas";
import { DiagramEditor } from "./architecture/DiagramEditor";
import { diagramEntries, type DiagramEntry } from "./architecture/architectureLogic";
import { emptyGraphKind } from "./architecture/emptyStateLogic";
import { copyDiagramName } from "./architecture/copyLogic";
import { loadViewport, saveViewport, viewportKey } from "./architecture/viewportLogic";
import * as api from "../ipc/api";
import type { ArchGraph, DiagramDerivation, DiagramFile, Workspace } from "../ipc/types";

/**
 * The Architecture tab: a list of diagrams of this workspace, and the picture
 * the selected one draws.
 *
 * # Two named built-ins, not a level selector
 *
 * The list opens on "Project map" and "Component map" and the saved diagrams
 * follow underneath. They are two *questions*, not two magnifications: the
 * project map is what is in this repository, the component map is what the
 * system consists of at run time, and the second drops every
 * `projectReference` arrow and adds data stores that appear nowhere in the
 * first. The argument in full — and the ordering, and the sentence each one
 * carries — lives in `architectureLogic.ts`, which is where it can be tested.
 *
 * # Three not-ready states, kept apart
 *
 * This follows `InspectView`, for the same reason it does: a view that renders
 * the same grey box while it is working, while it has a real answer that is
 * empty, and while something has gone wrong teaches the user that the grey box
 * means nothing.
 *
 * * **Loading** — a spinner and disabled controls.
 * * **Empty is an answer — and it is two of them.** `arch_component_graph`
 *   deliberately returns nothing when no HIGH-strength signal exists and never
 *   falls back to the project map to avoid it, so a repository of class
 *   libraries *has* no components. That is said in those words, with the
 *   reason, and not as a failure. But an empty graph carrying warnings is the
 *   other answer entirely — *candidates were found and every one was refused*
 *   — and telling that user nothing was found is simply false. The two get
 *   different words, and the second lists its reasons; `emptyStateLogic` tells
 *   them apart.
 * * **Error** — whatever the command said, through `api.errorMessage`.
 *
 * # The warnings are part of the diagram
 *
 * `ArchGraph.warnings` is where every reference the deriver read and refused to
 * draw ends up — an unresolvable project reference, a workspace membership it
 * would not infer, a relation no edge kind can express, and, on the component
 * map, every candidate the signal gate turned down. Until now they reached a
 * person only as `%%` comments inside the Mermaid source, which Mermaid does
 * not render, so the picture looked complete and was not. They are a
 * requirement of this feature, not decoration.
 *
 * They are drawn by `DiagramCanvas`, which counts them in its own toolbar and
 * lists them under the picture — beside the diagram they qualify rather than in
 * a band at the top of a tab. This view's job is to make sure they *arrive*:
 * every load puts something in `warnings`, including the one thing a stored
 * file can report about itself (`DiagramFile.warning`, front matter that could
 * not be read). Duplicating the panel here would give the same list two places
 * to disagree.
 *
 * The one exception is the empty state, and it exists because the canvas is
 * *not mounted* there: a graph with no nodes and a non-empty `warnings` list
 * would otherwise drop the only thing the derivation had to say, in the single
 * case where the warnings are the whole answer rather than a footnote to a
 * picture. That branch lists them itself. It is mutually exclusive with the
 * canvas, so there are still never two lists on screen to disagree.
 *
 * # Nothing is cached
 *
 * Every selection re-derives. The inputs are manifests the user edits while
 * the workspace stays open, and a stale arrow asserts a dependency that may
 * since have been deleted; `commands/architecture.rs` refuses to cache for the
 * same reason, and holding the result here would put the staleness back.
 */
export interface ArchitectureViewProps {
  workspace: Workspace;
  /**
   * Open a workspace-relative file, revealing a line when one is known.
   *
   * The same signature as `App`'s `requestOpenFile`, so it can be passed
   * straight through: this view has no editor of its own and clicking a box
   * belongs in the one the Run tab already owns.
   */
  onOpenFile?: (path: string, name: string, line?: number) => void;
}

/** What the canvas is currently showing, or why it is showing nothing. */
interface Loaded {
  /**
   * The entry this was loaded for.
   *
   * Carried so that nothing is ever drawn under another diagram's name. The
   * selection changes the instant it is clicked and the load resolves later, so
   * without this the previous picture sits under the new title for as long as
   * the derivation takes — and, worse, the editor would open on it and save the
   * wrong body into the right file.
   */
  id: string;
  /** The diagram text: rendered Mermaid for a built-in, the file for a saved one. */
  source: string;
  /**
   * The graph behind it, or `null` for a saved diagram.
   *
   * A built-in's nodes were minted by `cb-core`, so a click is a lookup. A
   * saved diagram's node ids are whatever their author typed and there is no
   * graph to look them up in — the canvas matches those against the symbol
   * index instead, exactly and uniquely or not at all.
   */
  graph: ArchGraph | null;
  warnings: string[];
  /**
   * A stored file's own provenance, or `null` for a built-in.
   *
   * `null` is not "unknown": the canvas falls back to `graph.derivation`, which
   * is the authoritative answer for a derived map. Only a saved file needs this
   * passed, because its provenance lives on the `DiagramFile` and not in the
   * graph — there is no graph.
   */
  derivation: DiagramDerivation | null;
  /** An inferred diagram a person has since changed. */
  edited: boolean;
}

export function ArchitectureView({ workspace, onOpenFile }: ArchitectureViewProps) {
  const [saved, setSaved] = useState<DiagramFile[]>([]);
  const [selectedId, setSelectedId] = useState<string>("builtin:project");
  const [loaded, setLoaded] = useState<Loaded | null>(null);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [listError, setListError] = useState<string | null>(null);

  const [editing, setEditing] = useState(false);
  const [dirty, setDirty] = useState(false);
  /** A diagram the user asked for while an unsaved edit is open. */
  const [pendingSelect, setPendingSelect] = useState<DiagramEntry<DiagramFile> | null>(null);
  /** Set when closing the editor would throw an unsaved edit away. */
  const [pendingClose, setPendingClose] = useState(false);

  /** The name being typed for a copy of a built-in, or `null` when not naming one. */
  const [copyName, setCopyName] = useState<string | null>(null);
  const [copyError, setCopyError] = useState<string | null>(null);

  /**
   * Which load is the current one.
   *
   * Selections are cheap to make and the derivations are not equally quick, so
   * a click on the component map followed by a click back can resolve out of
   * order. Rendering the older answer would put a diagram under a name that is
   * not its own, which is the one failure a diagram must not have.
   */
  const loadSeq = useRef(0);

  const entries = diagramEntries(saved);
  const selected = entries.find((entry) => entry.id === selectedId) ?? entries[0] ?? null;

  const refreshList = useCallback(async () => {
    try {
      setSaved(await api.archListDiagrams());
      setListError(null);
    } catch (e) {
      // The built-ins do not come from this call, so the tab still works; the
      // list is what is incomplete, and it says so where the list is.
      setListError(api.errorMessage(e));
    }
  }, []);

  const load = useCallback(async (entry: DiagramEntry<DiagramFile>) => {
    const seq = (loadSeq.current += 1);
    setLoading(true);
    setError(null);
    try {
      if (entry.builtin === "project") {
        const [graph, source] = await Promise.all([
          api.archProjectGraph(),
          api.archRenderGraph(),
        ]);
        if (seq !== loadSeq.current) return;
        setLoaded({ id: entry.id, source, graph, warnings: graph.warnings, derivation: null, edited: false });
      } else if (entry.builtin === "component") {
        const [graph, source] = await Promise.all([
          api.archComponentGraph(),
          api.archRenderComponentGraph(),
        ]);
        if (seq !== loadSeq.current) return;
        setLoaded({ id: entry.id, source, graph, warnings: graph.warnings, derivation: null, edited: false });
      } else if (entry.file) {
        const file = entry.file;
        const source = await api.archReadDiagram(file.name);
        if (seq !== loadSeq.current) return;
        setLoaded({
          id: entry.id,
          source,
          graph: null,
          // A stored diagram carries no `warnings` list; the one thing the
          // store can report about it is that its front matter could not be
          // read, and that is exactly as load-bearing here.
          warnings: file.warning === null ? [] : [file.warning],
          derivation: file.derivation,
          edited: file.edited,
        });
      }
    } catch (e) {
      if (seq !== loadSeq.current) return;
      setLoaded(null);
      setError(api.errorMessage(e));
    } finally {
      if (seq === loadSeq.current) setLoading(false);
    }
  }, []);

  useEffect(() => {
    void refreshList();
    // A rescan re-reads the manifests, which is precisely what these diagrams
    // are derived from, so the picture is derived again with them.
  }, [refreshList, workspace]);

  useEffect(() => {
    if (!selected) return;
    void load(selected);
    setCopyName(null);
    setPendingClose(false);
    setCopyError(null);
    // Keyed on the selection and on the workspace object, not on the entry
    // (which is rebuilt on every render).
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [selectedId, saved.length, workspace, load]);

  /**
   * Change diagram, unless doing so would silently throw away an edit.
   *
   * The editor is keyed by file name and a different diagram is a different
   * editor, so switching destroys the buffer. Losing someone's typing to a
   * click on a list is not a trade this view gets to make on their behalf, so
   * the switch is held and named instead.
   */
  function choose(entry: DiagramEntry<DiagramFile>) {
    if (entry.id === selectedId) return;
    if (dirty) {
      setPendingSelect(entry);
      return;
    }
    setEditing(false);
    setSelectedId(entry.id);
  }

  function discardAndSwitch() {
    const entry = pendingSelect;
    setPendingSelect(null);
    if (!entry) return;
    setDirty(false);
    setEditing(false);
    setSelectedId(entry.id);
  }

  /**
   * Store the picture a built-in is currently drawing as a diagram of the
   * user's own.
   *
   * # Why a built-in has no editor at all
   *
   * Not read-only, not editable — absent, and the difference matters. A
   * built-in is **not a file**: `arch_project_graph` and `arch_component_graph`
   * derive it from the manifests on every call and store nothing, so there is
   * no name to write to and nothing an edit could be saved into. A read-only
   * CodeMirror over generated text would be a box that looks exactly like the
   * editor beside it and refuses every keystroke, which teaches the user that
   * this app's editors sometimes silently do not work.
   *
   * Even where a derived diagram does exist on disk it is regenerated and
   * gitignored, which is why `store::write` *promotes* an edited one out of
   * that directory rather than overwriting it in place. Copying is the same
   * move made explicitly, one step earlier.
   *
   * What the copy is, and what it is not, is said in the interface: it is the
   * picture as it is at this moment. It stops tracking the manifests the
   * instant it is written, so a project added tomorrow appears in the built-in
   * and not in the copy. That is the whole point of keeping one — a diagram
   * you have annotated is worth more than an accurate one you cannot draw on —
   * but it is not something to discover later.
   *
   * A name already taken is refused rather than resolved — `store::write`
   * would treat it as an edit of that file and overwrite its body, so guessing
   * here would destroy a diagram somebody drew. That check, and the extension
   * rule it depends on, live in `copyLogic.ts` where they are tested.
   */
  async function saveCopy() {
    if (!shown) return;
    const chosen = copyDiagramName(copyName ?? "", saved);
    if (!chosen.ok) {
      setCopyError(chosen.reason);
      return;
    }
    const name = chosen.name;

    try {
      // A validation error would mean the copy was stored and does not render
      // — which cannot happen to text this app just rendered, but is reported
      // rather than dropped if it ever does.
      //
      // What is handed over is a built-in's rendered source, which carries no
      // front matter, so `arch_write_diagram` validates exactly the body that
      // `store::write` is about to wrap. That is the same frame `DiagramEditor`
      // checks in once the copy is opened, so the two panes cannot say
      // different things about one file — they did before `frontMatterLogic`
      // existed, and that is what it is for.
      const problem = await api.archWriteDiagram(name, shown.source);
      setCopyError(
        problem === null
          ? null
          : `Saved, but ${name} does not render: ${problem.detail} (line ${problem.line}).`,
      );
      setCopyName(null);
      await refreshList();
      setSelectedId(`saved:${name}`);
      setEditing(true);
    } catch (e) {
      setCopyError(api.errorMessage(e));
    }
  }

  /**
   * What is loaded, but only if it belongs to what is selected.
   *
   * Everything below reads this rather than `loaded`. A selection changes
   * immediately and its load lands later, so for that interval `loaded` holds
   * the *previous* diagram: drawing it would put one picture under another's
   * name, and opening the editor on it would save one diagram's body into
   * another diagram's file.
   */
  const shown = loaded !== null && loaded.id === selectedId ? loaded : null;
  /**
   * Which empty answer this is, or `null` when there is a picture.
   *
   * "Empty" is two different pieces of news and only one of them is an
   * absence. A derivation that found candidates and refused every one of them
   * produces exactly the same zero nodes, with its reasons in `warnings` — and
   * in that case the warnings are not commentary on a picture, they *are* the
   * answer. `emptyStateLogic` carries the argument and the tests.
   */
  const emptyKind = emptyGraphKind(shown?.graph?.nodes.length ?? null, shown?.warnings ?? []);

  /**
   * Where this diagram was last being looked at.
   *
   * The tab is mounted conditionally — leaving it destroys the canvas — and the
   * canvas is additionally keyed per diagram, so without this every return to a
   * picture starts at the fit view again. Keyed on the workspace as well as the
   * diagram: two repositories both have a project map and they are not the same
   * drawing. Reading and validating it is `viewportLogic`'s, tested there.
   */
  const vpKey = viewportKey(workspace.root, selectedId);

  return (
    <>
      {/* `file-list` for the shared row metrics; `diagram-list` for the two
          places this list is not a list of paths — see styles.css. */}
      <Sidebar className="file-list diagram-list">
        <div className="group-label">Derived from this workspace</div>
        {entries
          .filter((entry) => entry.source === "builtin")
          .map((entry) => (
            <button
              key={entry.id}
              className={`row ${entry.id === selectedId ? "selected" : ""}`}
              onClick={() => choose(entry)}
              title={entry.description ?? undefined}
              style={{ display: "block" }}
            >
              <span style={{ display: "block" }}>{entry.label}</span>
              {/* The sentence, not just the name: the two answer different
                  questions and nothing about the names says which. */}
              <span className="faint" style={{ display: "block", fontSize: 11 }}>
                {entry.description}
              </span>
            </button>
          ))}

        <div className="group-label">Saved in this workspace</div>
        {listError !== null && (
          <div className="warning" style={{ fontSize: 12 }}>
            The stored diagrams could not be listed, so this part of the list is
            incomplete: {listError}
          </div>
        )}
        {entries.filter((entry) => entry.source === "saved").length === 0 &&
          listError === null && (
            <div className="muted" style={{ padding: "4px 8px", fontSize: 12 }}>
              None yet. Save a copy of a derived map to start one, or let an
              agent write one into <code>.code-basics/diagrams/</code>.
            </div>
          )}
        {entries
          .filter((entry) => entry.source === "saved")
          .map((entry) => (
            <button
              key={entry.id}
              className={`row ${entry.id === selectedId ? "selected" : ""}`}
              onClick={() => choose(entry)}
              title={entry.file?.path ?? entry.label}
            >
              <span className="path">{entry.label}</span>
              {dirty && entry.id === selectedId && (
                <span className="dirty-dot" title="Unsaved changes — Ctrl+S to save">
                  ●
                </span>
              )}
              {entry.file?.warning != null && (
                <span title={entry.file.warning}>⚠</span>
              )}
            </button>
          ))}
      </Sidebar>

      <div className="main">
        <div className="toolbar">
          <span style={{ fontSize: 13 }}>{selected?.label ?? "No diagram"}</span>
          {loading && <span className="spinner" />}

          <button
            data-command="architecture.refresh"
            onClick={() => {
              if (selected) void load(selected);
              void refreshList();
            }}
            disabled={loading}
            title="Derive the picture again from the manifests as they are on disk right now. Nothing here is cached."
          >
            Refresh
          </button>

          {selected?.source === "saved" && (
            <button
              data-command="architecture.edit"
              onClick={() => {
                // Closing destroys the buffer exactly as switching does, so it
                // asks for the same reason: an edit is not something to lose to
                // a mis-click on a toolbar.
                if (editing && dirty) setPendingClose(true);
                else setEditing((open) => !open);
              }}
              disabled={loading || shown === null}
              title="Edit the stored Mermaid source"
            >
              {editing ? "Close editor" : "Edit"}
            </button>
          )}

          {selected?.source === "builtin" && copyName === null && (
            <button
              onClick={() => {
                setCopyError(null);
                setCopyName(selected.builtin === "component" ? "component-map" : "project-map");
              }}
              disabled={loading || shown === null}
              title="Store this picture as a diagram of your own, which you can then edit"
            >
              Save a copy…
            </button>
          )}
          {selected?.source === "builtin" && copyName !== null && (
            <>
              <input
                autoFocus
                value={copyName}
                onChange={(e) => setCopyName(e.target.value)}
                onKeyDown={(e) => {
                  if (e.key === "Enter") void saveCopy();
                  if (e.key === "Escape") setCopyName(null);
                }}
                placeholder="diagram-name"
                style={{ width: 180 }}
              />
              <button className="primary" onClick={() => void saveCopy()}>
                Save copy
              </button>
              <button onClick={() => setCopyName(null)}>Cancel</button>
            </>
          )}

          <span style={{ flex: 1 }} />

          {/* The origin badge, the warning count and the list itself are drawn
              by the canvas below, beside the picture they are about. A second
              copy here would be one more thing to keep in agreement. */}
        </div>

        {error !== null && <div className="error">{error}</div>}

        {copyError !== null && <div className="warning">{copyError}</div>}

        {selected?.source === "builtin" && copyName !== null && (
          <div className="warning">
            <strong>A copy is a snapshot, not a second view of the same thing.</strong>{" "}
            The map above is derived from the manifests every time you open it;
            a copy is the picture as it is right now and stops there. A project
            added tomorrow will appear in the derived map and not in the copy —
            which is the point of keeping one you can annotate, but is worth
            knowing before you do.
          </div>
        )}

        {pendingClose && (
          <div className="warning">
            <strong>{selected?.label} has unsaved changes.</strong> Closing the
            editor discards them. Press Ctrl+S in the editor to save first.
            <div style={{ marginTop: 6, display: "flex", gap: 6 }}>
              <button onClick={() => setPendingClose(false)}>Keep editing</button>
              <button
                onClick={() => {
                  setPendingClose(false);
                  setDirty(false);
                  setEditing(false);
                }}
              >
                Discard the changes and close
              </button>
            </div>
          </div>
        )}

        {pendingSelect !== null && (
          <div className="warning">
            <strong>{selected?.label} has unsaved changes.</strong> Switching
            diagrams closes this editor and the edit goes with it. Press Ctrl+S
            in the editor to save first.
            <div style={{ marginTop: 6, display: "flex", gap: 6 }}>
              <button onClick={() => setPendingSelect(null)}>Stay here</button>
              <button onClick={discardAndSwitch}>
                Discard the changes and open {pendingSelect.label}
              </button>
            </div>
          </div>
        )}

        <div className="content split">
          {/* A flex column, not the default block: `DiagramCanvas` is a
              `.main` that fills its parent with `flex: 1`, and in a block box
              it would have no height to fill and collapse to nothing. */}
          <div
            className="top"
            style={{ overflow: "hidden", display: "flex", flexDirection: "column", minHeight: 0 }}
          >
            {shown === null && error === null ? (
              <div className="empty">
                <span className="spinner" style={{ display: "inline-block", marginRight: 8 }} />
                Deriving this diagram…
              </div>
            ) : error !== null ? (
              // The error is already stated above; the canvas area says why it
              // is blank rather than sitting there empty.
              <div className="empty">Nothing could be drawn — see the message above.</div>
            ) : emptyKind !== null ? (
              <div
                className="empty"
                style={{ maxWidth: 620, margin: "0 auto", overflow: "auto" }}
              >
                {emptyKind === "allRefused" ? (
                  // Something *was* found. Saying "none found" here would be
                  // false, and would throw away the only information this
                  // derivation produced — so the reasons are listed, in the
                  // one branch where the canvas that normally lists them is
                  // not on screen to do it. The two branches are mutually
                  // exclusive by construction, so this is not a second copy of
                  // that panel: it is the only one that can appear.
                  <>
                    <p>
                      <strong>
                        Nothing could be drawn — but not because nothing was
                        found.
                      </strong>
                    </p>
                    <p>
                      {selected?.builtin === "component"
                        ? "Every candidate this map found was refused, so no box and no arrow survived. A refusal is deliberate — a component asserted on a weak signal is a claim about how this system runs that nobody made — and each one is named below."
                        : "Everything this map found was refused rather than drawn, and each refusal is named below."}{" "}
                      Fixing what a reason describes, and deriving again, is
                      what turns one of these into a box.
                    </p>
                    <ul style={{ textAlign: "left", margin: "8px 0 0", paddingLeft: 20 }}>
                      {shown?.warnings
                        .filter((warning) => warning.trim() !== "")
                        .map((warning, index) => (
                          <li key={`${index}:${warning}`} style={{ fontSize: 12, marginTop: 4 }}>
                            {warning}
                          </li>
                        ))}
                    </ul>
                  </>
                ) : selected?.builtin === "component" ? (
                  <>
                    <p>
                      <strong>No components were found in this workspace.</strong>
                    </p>
                    <p>
                      This is an answer, not a failure. The component map is
                      built only from things the code says out loud — a service
                      declared in a manifest, a data store named in a
                      configuration file — and no service or data-store
                      declaration was found here, nor was anything found and
                      turned down. A repository of class libraries and tools
                      genuinely has no components.
                    </p>
                    <p>
                      It is deliberately never filled in from the project map
                      instead: those answer different questions, and showing one
                      under the other&apos;s name would be the wrong picture
                      rather than a missing one.
                    </p>
                  </>
                ) : (
                  <>
                    <p>
                      <strong>No projects were detected in this workspace.</strong>
                    </p>
                    <p>
                      The project map is derived from manifests — a{" "}
                      <code>.csproj</code>, a <code>package.json</code>, a{" "}
                      <code>.sln</code> — and the scan found none, so there is
                      nothing to draw. Open a repository that holds one, or
                      press Rescan if the projects were added since this
                      workspace opened.
                    </p>
                  </>
                )}
              </div>
            ) : shown !== null ? (
              <DiagramCanvas
                // Remounted per diagram: a canvas keeps pan and zoom, and a
                // second diagram inheriting the first one's viewport would open
                // scrolled to a corner of a picture that is not the same shape.
                key={selectedId}
                source={shown.source}
                graph={shown.graph}
                warnings={shown.warnings}
                derivation={shown.derivation}
                edited={shown.edited}
                initialView={loadViewport(localStorage, vpKey)}
                onViewChange={(next) => saveViewport(localStorage, vpKey, next)}
                onOpenNode={(target) => {
                  const name = target.path.split("/").pop() ?? target.path;
                  onOpenFile?.(target.path, name, target.line);
                }}
                onError={setError}
              />
            ) : (
              // Unreachable: the two built-ins are always in the list, so
              // something is always selected, and the three branches above
              // cover every state that selection can be in.
              null
            )}
          </div>

          {editing && selected?.source === "saved" && selected.file && shown !== null && (
            <div className="bottom">
              <DiagramEditor
                // Keyed by file, so a different diagram is a different editor
                // with its own undo history — and so the buffer can never be
                // rebound under someone mid-edit.
                key={selected.file.name}
                name={selected.file.name}
                initialText={shown.source}
                onDirtyChange={setDirty}
                onSaved={(text) => {
                  // The canvas re-renders from the saved text, and the list is
                  // re-read because a save can move the file: editing a derived
                  // diagram promotes it out of the regenerated directory.
                  setLoaded((current) => (current === null ? current : { ...current, source: text }));
                  void refreshList();
                }}
              />
            </div>
          )}
        </div>
      </div>
    </>
  );
}

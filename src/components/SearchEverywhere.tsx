import { useCallback, useEffect, useRef, useState } from "react";
import * as api from "../ipc/api";
import {
  actionableIds,
  dropUnactionable,
  groupHits,
  highlightSpans,
  indexNote,
  nextIndex,
  resultsState,
  searchKey,
  type SearchScope,
} from "./searchLogic";
import type { SearchHit, SymbolIndexStatus, Workspace } from "../ipc/types";
import { registerCommand } from "../shortcuts";

/**
 * The search palette: one overlay over the whole app that finds a file, a
 * symbol or a run configuration and hands the choice back to whoever can act
 * on it.
 *
 * This file is a rendering shell, deliberately. Every decision it appears to
 * make — which keydown opened it, where the arrow keys move, how a label is cut
 * into highlighted runs — is a call into `searchLogic.ts`, which is pure and
 * tested; and the ranking, the scope filtering and the `Foo:123` line suffix are
 * all `cb-core`'s and are not touched here at all. What is left is state,
 * effects and JSX, which is the part the repo does not test.
 *
 * It renders nothing at all until it is opened, but it is mounted the whole time
 * the workspace is: the window-level key listener is the only way the palette
 * can be reached, so an unmounted palette is an unopenable one.
 */

/** How long a keystroke waits before it costs a backend call. */
const DEBOUNCE_MS = 80;

/** How many hits to ask for. Bounds the ranking in `cb-core`, not just the list. */
const LIMIT = 50;

/** How often the indexing state is re-read while a build is in flight. */
const STATUS_POLL_MS = 700;

/** The scopes, in the order Tab walks them. */
const SCOPES: SearchScope[] = ["all", "files", "symbols", "actions"];

const SCOPE_LABELS: Record<SearchScope, string> = {
  all: "All",
  files: "Files",
  symbols: "Symbols",
  actions: "Actions",
};

const PLACEHOLDERS: Record<SearchScope, string> = {
  all: "Search files, symbols and configurations…",
  files: "Search files…",
  symbols: "Search symbols…",
  actions: "Search run configurations…",
};

/** The file name a tab should carry, from either separator a path may use. */
function baseName(path: string): string {
  const parts = path.split(/[\\/]/);
  return parts[parts.length - 1] || path;
}

export function SearchEverywhere({
  workspace,
  active,
  onOpenFile,
  onRunAction,
}: {
  /**
   * The workspace this palette searches, passed down rather than fetched: with
   * several codebases open, `current_workspace` would answer with whichever tab
   * is active, which need not be the one this palette belongs to.
   */
  workspace: Workspace;
  /**
   * Whether this palette's workspace is the foreground tab. Only the active tab
   * binds the global Shift-Shift / Ctrl+N listener — otherwise every open
   * codebase would race to open its own palette on one keystroke.
   */
  active: boolean;
  /** Open a workspace-relative file, optionally revealing a 1-based line. */
  onOpenFile: (path: string, name: string, line?: number) => void;
  /** Select the configuration behind an action hit. It is never started here. */
  onRunAction: (actionId: string) => void;
}) {
  const [open, setOpen] = useState(false);
  const [scope, setScope] = useState<SearchScope>("all");
  const [query, setQuery] = useState("");
  const [hits, setHits] = useState<SearchHit[]>([]);
  const [selected, setSelected] = useState(0);
  const [error, setError] = useState<string | null>(null);
  const [status, setStatus] = useState<SymbolIndexStatus | null>(null);
  /**
   * Which search the hits in hand answer — {@link searchKey} of the scope and
   * query they were fetched under, or null when nothing has been fetched.
   *
   * Held beside `hits` rather than derived from them because a result list
   * carries no record of what was asked for. Without it the palette cannot tell
   * "this is the answer" from "this is the previous answer", and it reopens
   * under a new scope showing the old one's rows.
   */
  const [resultKey, setResultKey] = useState<string | null>(null);
  /**
   * The configuration ids an action row may name, or null until the workspace
   * has been read. See {@link actionableIds} for why the palette needs it at
   * all: it ranks over every configuration, and only application ones can be
   * acted on by the tab that receives the choice.
   */
  const [actionable, setActionable] = useState<ReadonlySet<string> | null>(null);

  const inputRef = useRef<HTMLInputElement>(null);

  /**
   * The rendered rows, indexed exactly as the flattened hit list is, so the
   * selection can be scrolled into view.
   *
   * Rebuilt every render: the list is short, the array is truncated to the row
   * count below, and holding a node from a previous result set would scroll to
   * a row that is no longer on screen.
   */
  const rowRefs = useRef<(HTMLButtonElement | null)[]>([]);

  /**
   * Set when the pointer, not the keyboard, moved the selection.
   *
   * Hovering already puts the row under the user's eye, and scrolling to it
   * would slide the list out from under the cursor — which then lands on a
   * different row and moves the selection again. The flag is cleared by the
   * scroll effect that skips because of it, so the next arrow key scrolls
   * normally.
   */
  const pointerMoved = useRef(false);

  /**
   * The most recent query this component asked for.
   *
   * Compared on arrival so a slow reply to an old query cannot overwrite a
   * newer result. Without it the palette is only as correct as the backend is
   * ordered: two searches are two independent promises, and the shorter query —
   * which matches more and therefore ranks more — is the one likely to come
   * back late and land on top of what the user has since typed.
   */
  const sequence = useRef(0);

  const close = useCallback(() => {
    setOpen(false);
    // A stale reply from the search that was in flight when the palette closed
    // must not repopulate it the next time it opens.
    sequence.current += 1;
    setError(null);
    // Neither may the reply that already arrived. The next open is very often a
    // different scope — Esc out of Ctrl+Shift+A, in with Ctrl+Shift+N — and
    // these rows were ranked over a population that scope excludes.
    //
    // The query text is deliberately kept. Reopening on what you last searched
    // for is the behaviour Rider has and it is genuinely useful, `openAt`
    // already selects the text so the next keystroke replaces it, and it costs
    // nothing in correctness now that the rows are gone: the search effect
    // re-runs on open, and until its answer arrives `resultsState` reports
    // `pending` and no rows are drawn.
    setHits([]);
    setResultKey(null);
  }, []);

  /** Open at `next`, or just switch scope if it is already up. */
  const openAt = useCallback((next: SearchScope) => {
    setScope(next);
    setSelected(0);
    setOpen(true);
    // The input mounts in this same commit, so focus waits a tick for it.
    setTimeout(() => inputRef.current?.select(), 0);
  }, []);

  useEffect(() => {
    if (!active) return;
    const registrations = [
      registerCommand("search.all", () => openAt("all")),
      registerCommand("search.symbols", () => openAt("symbols")),
      registerCommand("search.files", () => openAt("files")),
      registerCommand("search.actions", () => openAt("actions")),
    ];
    return () => registrations.forEach((unregister) => unregister());
  }, [openAt, active]);

  /** Bumped to restart the status read after a rebuild is asked for. */
  const [statusToken, setStatusToken] = useState(0);

  // What the index holds. An empty result list during a build is not "no
  // matches", and saying nothing would let the user read it as one — so the
  // state is read on open and followed until it settles.
  useEffect(() => {
    if (!open) return;
    let cancelled = false;
    let timer: ReturnType<typeof setTimeout> | undefined;

    const read = () => {
      api
        .symbolIndexStatus()
        .then((next) => {
          if (cancelled) return;
          setStatus(next);
          // Only a build in flight earns another call. A settled index does not
          // change while the palette is up, and polling it would be a backend
          // round trip a second for two numbers that will not move.
          if (next.building) timer = setTimeout(read, STATUS_POLL_MS);
        })
        .catch(() => {
          // The palette still works without a status line; the search itself
          // reports its own failure.
        });
    };

    read();
    return () => {
      cancelled = true;
      if (timer) clearTimeout(timer);
    };
  }, [open, statusToken]);

  // Which action rows are worth offering, derived from this tab's own workspace.
  // Recomputed when its configs change — a rescan, an import or a deleted
  // configuration hands down a fresh `workspace` prop — so the set never offers a
  // configuration the workspace no longer has, and never one from another tab.
  useEffect(() => {
    setActionable(actionableIds(workspace.configs));
  }, [workspace.configs]);

  // The search itself, debounced.
  useEffect(() => {
    if (!open) return;

    // An empty query matches everything with score 0, so the backend would
    // answer with an arbitrary fifty rows of the workspace. Showing those as
    // "results" would be this file inventing a ranking nothing computed.
    if (query.trim() === "") {
      sequence.current += 1;
      setHits([]);
      setResultKey(null);
      setSelected(0);
      setError(null);
      return;
    }

    const key = searchKey(scope, query);
    const timer = setTimeout(() => {
      const ticket = (sequence.current += 1);
      api
        .searchEverywhere(query, scope, LIMIT)
        .then((found) => {
          if (ticket !== sequence.current) return;
          setHits(found);
          // Recorded together with the rows, from the scope and query this
          // request was made for — not from the state as it reads on arrival,
          // which is what the user has typed since.
          setResultKey(key);
          setSelected(0);
          setError(null);
        })
        .catch((e: unknown) => {
          if (ticket !== sequence.current) return;
          setHits([]);
          // No key: a failed search has produced no answer, and claiming "No
          // matches." underneath the error message would be a second, invented
          // one.
          setResultKey(null);
          setError(api.errorMessage(e));
        });
    }, DEBOUNCE_MS);

    return () => clearTimeout(timer);
  }, [open, query, scope]);

  // What is on screen, in three steps: drop the rows nothing could act on, ask
  // whether the survivors answer the search being displayed, and only then
  // group them. Rows are drawn from `flat` alone, so a `pending` palette has no
  // rows at all and Enter in that window cannot reach the previous query's.
  const usable = dropUnactionable(hits, actionable);
  const currentKey = searchKey(scope, query);
  const state = resultsState(query, resultKey, currentKey, usable.length);
  const sections = groupHits(state === "hits" ? usable : []);
  const flat = sections.flatMap((section) => section.hits);
  // The stored index can outrun the list — the workspace arrives and removes
  // action rows, or a shorter result set lands. `nextIndex` with no movement is
  // the same normalisation the arrow keys already get.
  const selectedIndex = nextIndex(selected, 0, flat.length);

  rowRefs.current.length = flat.length;

  // Keep the selected row on screen. `block: "nearest"` scrolls the least that
  // will do, so a row already visible is untouched and the wrap from the last
  // row back to the first jumps the list back to the top — which is the case
  // that used to leave Enter acting on a row scrolled out of sight.
  useEffect(() => {
    if (pointerMoved.current) {
      pointerMoved.current = false;
      return;
    }
    rowRefs.current[selectedIndex]?.scrollIntoView({ block: "nearest" });
  }, [selectedIndex, resultKey, flat.length]);

  /** Act on a hit and close. A file opens; an action selects its configuration. */
  function choose(hit: SearchHit) {
    if (hit.kind === "action") {
      if (hit.actionId) onRunAction(hit.actionId);
    } else if (hit.path) {
      onOpenFile(hit.path, baseName(hit.path), hit.line ?? undefined);
    } else {
      return; // nothing to open; leave the palette up rather than close on nothing
    }
    close();
  }

  function onInputKeyDown(event: React.KeyboardEvent<HTMLInputElement>) {
    switch (event.key) {
      case "Escape":
        event.preventDefault();
        close();
        return;
      case "ArrowDown":
        event.preventDefault();
        setSelected((current) => nextIndex(current, 1, flat.length));
        return;
      case "ArrowUp":
        event.preventDefault();
        setSelected((current) => nextIndex(current, -1, flat.length));
        return;
      case "Tab": {
        // Tab is the scope cycle, so it must not move focus out of the input —
        // the palette has one control and losing focus from it strands the
        // keyboard user in an overlay they can only leave with the mouse.
        event.preventDefault();
        const at = SCOPES.indexOf(scope);
        const step = event.shiftKey ? -1 : 1;
        // `nextIndex` answers within range for a non-empty list, but the
        // compiler cannot know that of a plain array index; falling back to
        // `all` keeps the widest scope rather than an undefined one.
        setScope(SCOPES[nextIndex(at, step, SCOPES.length)] ?? "all");
        setSelected(0);
        return;
      }
      case "Enter": {
        event.preventDefault();
        const hit = flat[selectedIndex];
        if (hit) choose(hit);
        return;
      }
      default:
    }
  }

  if (!open) return null;

  const building = status?.building === true;
  const note = indexNote(status);

  let counter = -1;

  return (
    <div className="modal-backdrop palette-backdrop" onMouseDown={close}>
      <div
        className="palette"
        role="dialog"
        aria-label="Search everywhere"
        onMouseDown={(event) => event.stopPropagation()}
      >
        <div className="palette-scopes">
          {SCOPES.map((option) => (
            <button
              key={option}
              className={option === scope ? "active" : ""}
              onClick={() => {
                setScope(option);
                setSelected(0);
                inputRef.current?.focus();
              }}
            >
              {SCOPE_LABELS[option]}
            </button>
          ))}
          <span style={{ flex: 1 }} />
          <span className="palette-hint">Tab to change scope · Esc to close</span>
        </div>

        <input
          ref={inputRef}
          className="palette-input"
          autoFocus
          spellCheck={false}
          placeholder={PLACEHOLDERS[scope]}
          value={query}
          onChange={(event) => setQuery(event.target.value)}
          onKeyDown={onInputKeyDown}
        />

        {error && <div className="error">{error}</div>}
        {note && <div className="palette-note">{note}</div>}

        <div className="palette-results">
          {state === "prompt" && (
            <div className="palette-empty">
              Type to search. A trailing <span className="mono">:123</span> jumps to
              that line.
            </div>
          )}
          {/* "Searching…" and not silence: an empty results area under a typed
              query reads as "nothing matched", which is the one thing that is
              not yet known. The error message covers this line when a search
              failed, so it is not shown twice. */}
          {state === "pending" && !error && !building && (
            <div className="palette-empty">Searching…</div>
          )}
          {state === "empty" && !building && (
            <div className="palette-empty">No matches.</div>
          )}

          {sections.map((section) => (
            <div key={section.kind}>
              <div className="group-label">{section.title}</div>
              {section.hits.map((hit) => {
                counter += 1;
                const index = counter;
                return (
                  <button
                    key={`${hit.kind}:${hit.path ?? hit.actionId ?? ""}:${hit.label}:${hit.line ?? ""}`}
                    className={`palette-row${index === selectedIndex ? " selected" : ""}`}
                    ref={(element) => {
                      rowRefs.current[index] = element;
                    }}
                    // Selection follows the pointer so the keyboard and the
                    // mouse never disagree about which row Enter would take.
                    // The flag tells the scroll effect this move came from the
                    // mouse, which must not drag the list under the cursor.
                    onMouseMove={() => {
                      if (index !== selectedIndex) pointerMoved.current = true;
                      setSelected(index);
                    }}
                    onClick={() => choose(hit)}
                  >
                    <span className="palette-label">
                      {highlightSpans(hit.label, hit.positions).map((span, i) =>
                        span.hit ? (
                          <mark key={i}>{span.text}</mark>
                        ) : (
                          <span key={i}>{span.text}</span>
                        ),
                      )}
                      {hit.line != null && (
                        <span className="palette-line mono">:{hit.line}</span>
                      )}
                    </span>
                    {/* `other` means the one-line scan abstained; drawing the
                        word "other" would dress that up as a classification. */}
                    {hit.symbolKind && hit.symbolKind !== "other" && (
                      <span className="palette-kind">{hit.symbolKind}</span>
                    )}
                    <span className="palette-detail mono">{hit.detail}</span>
                  </button>
                );
              })}
            </div>
          ))}
        </div>

        <div className="palette-footer">
          <span className="palette-hint">
            {status
              ? `${status.files} file${status.files === 1 ? "" : "s"} · ${status.symbols} symbol${
                  status.symbols === 1 ? "" : "s"
                } indexed`
              : "Reading the index…"}
          </span>
          <span style={{ flex: 1 }} />
          <button
            disabled={building}
            title="Discard the symbol index and walk the workspace again"
            onClick={() => {
              api
                .rebuildSymbolIndex()
                .then(() => setStatusToken((n) => n + 1))
                .catch((e: unknown) => setError(api.errorMessage(e)));
            }}
          >
            Rebuild index
          </button>
        </div>
      </div>
    </div>
  );
}

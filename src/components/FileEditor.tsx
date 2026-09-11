import { useEffect, useRef, useState } from "react";
import { EditorState, type Extension } from "@codemirror/state";
import { drawSelection, dropCursor, EditorView, keymap, lineNumbers } from "@codemirror/view";
import {
  defaultKeymap,
  history,
  historyKeymap,
  indentWithTab,
  toggleComment,
} from "@codemirror/commands";
import { gotoLine, highlightSelectionMatches, openSearchPanel, search, searchKeymap } from "@codemirror/search";
import { editorColors, languageFor } from "./language";
import {
  EMPTY_SECRETS,
  sourceEnablesLsp,
  sourceLanguageHint,
  type EditableSource,
} from "./editorSourceLogic";
import { lineToPos } from "./searchLogic";
import {
  acceptedNewName,
  bufferVerdict,
  documentBoundsVerdict,
  identifierAt,
  placeRenameField,
  posAt,
  provisionalRenameWarning,
  renameOffer,
  renameReadiness,
  replacedTextMatches,
  shouldHandleRename,
  type RenameReport,
} from "./renameLogic";
import { registerCommand } from "../shortcuts";
import { onEditorFontSizeChange } from "../editorFontSize";
import * as api from "../ipc/api";
import type { DeclarationAnchor, Highlight, RangeEdit, RenameResult, Target } from "../ipc/types";
import {
  ANCHOR_RETRY_LIMIT,
  ANCHOR_RETRY_MS,
  INERT_LOCATION_REASON,
  availabilityPhrase,
  clearUsageAnswers,
  countUsageRows,
  definitionAction,
  emptyGroupNote,
  failedUsageResult,
  groupUsages,
  newUsageAnswers,
  partialAnswerNote,
  placeMenu,
  recordUsageAnswer,
  retainUsageJobs,
  shouldAskUsages,
  shouldRetryAnchors,
  snippetParts,
  usageCacheKey,
  usageCountLabel,
  usageRowView,
  usageStateFor,
  visibleAnchors,
  type DefinitionGroup,
  type Placement,
  type UsageGroup,
  type UsageJob,
} from "./usagesLogic";
import {
  setUsageRows,
  usagesExtension,
  type GotoRequest,
  type UsageRowClick,
  type UsageRowSpec,
  type VisibleLines,
} from "./usagesExtension";

/**
 * How long an edit is allowed to settle before the servers are told about it.
 *
 * A `didChange` per keystroke is both wasteful and pointless — nothing can be
 * asked about a buffer the user is still typing into — and the debounce is also
 * what orders the sync against the requests: a usages query is not issued at all
 * while a change is still owed to the server (see {@link requestVisible}).
 */
const CHANGE_DEBOUNCE_MS = 250;

/**
 * How many references queries may be in flight at once.
 *
 * A references query is a workspace-wide search, and a screenful of a C# file is
 * easily thirty declarations. Roslyn answers them serially anyway; the point of
 * the bound is that scrolling never queues hundreds of requests whose answers
 * arrive after their document version has already been superseded.
 */
const MAX_INFLIGHT_USAGES = 4;

/**
 * The one overlay this editor can show, as a union.
 *
 * A union rather than three booleans because exactly one of them may be open and
 * because each carries different data: the usages list is per anchor, the goto
 * picker is per position, and a note is just words. `null` is closed.
 */
type Menu =
  | {
      kind: "usages";
      place: Placement;
      anchor: DeclarationAnchor;
      groups: UsageGroup[];
      total: number;
      truncated: boolean;
      /** The `ready`-with-a-caveat message, when the backend sent one. */
      message: string | null;
    }
  | {
      kind: "definition";
      place: Placement;
      groups: DefinitionGroup[];
      message: string | null;
      /** So a list from a server that is not settled can say that it may be short. */
      provisional: string | null;
    }
  | { kind: "note"; place: Placement; message: string };

/**
 * The inline rename field, while it is open.
 *
 * A React `<input>` absolutely positioned inside `.editor-frame`, **not** a
 * CodeMirror widget. `usagesExtension`'s own module doc records the finding: DOM
 * injected inside CodeMirror fights its event handling and is torn out by the
 * next viewport update, which is worse for an input that needs sustained focus
 * and whose text changes on every keystroke by design. A document-mutating
 * approach is worse still — it would fire `docChanged`, bump `docVersion` and arm
 * the flush timer, desynchronising the very thing the ordering guarantee
 * protects.
 */
interface RenameField {
  place: Placement;
  /** The identifier read out of the buffer. Also the backend's token check. */
  oldName: string;
  value: string;
  /** Where the question was aimed: 1-based line, 0-based UTF-16 character. */
  line: number;
  character: number;
  /** The document version the field was opened over; re-checked at Enter. */
  docVersion: number;
  /** The server's qualification of its own `ready` answer, when it made one. */
  caveat: string | null;
  /** Whether the user has already been shown {@link caveat} and pressed Enter. */
  confirmed: boolean;
  /** Why the last Enter did nothing. */
  error: string | null;
}

/**
 * Another editor's rename, handed to this one to apply to its own buffer.
 *
 * Carries `oldName` because the stale-mirror check is per edit and needs it: the
 * backend runs the same rule over the closed files it reads and cannot run it
 * here, since it has no text for a buffer that has not been saved.
 */
export interface PendingRenameEdits {
  path: string;
  oldName: string;
  edits: RangeEdit[];
}

/** A quiet corner badge: why this file has no inline rows. */
interface EditorNote {
  text: string;
  detail: string | null;
}

/** The file name for a tab label. A display helper, not a decision. */
function baseName(path: string): string {
  const parts = path.split(/[\\/]/);
  return parts[parts.length - 1] || path;
}

/**
 * A plain CodeMirror editor over one workspace file.
 *
 * Loads the file once on mount and saves with Ctrl+S (`Mod-s`). Kept mounted
 * while hidden — like the console sessions — so undo history, scroll position
 * and unsaved changes survive switching between file tabs.
 *
 * It is also the whole client of the language-server surface, because the
 * `didOpen`/`didClose` pair it owes a server is exactly this component's
 * lifetime: mounted per file tab, unmounted when the tab closes. On top of that
 * it draws the inline usages rows (via `usagesExtension`, decided by
 * `usagesLogic`), the usages dropdown and the middle-click goto picker.
 *
 * **Every language-server call here tolerates rejection.** No server is a normal
 * state, and a failing `invoke` must never take the file the user is reading off
 * the screen — so `error` (which replaces the editor with a message) is reserved
 * for the file read and write, and everything LSP-shaped degrades to a row or a
 * corner badge that says what happened.
 *
 * The tab's backing is an {@link EditableSource}, not a bare path: a workspace file
 * reads/writes through `fs_*` and drives the language-server surface, while a
 * project's user secrets read/write through the secrets commands and skip the
 * server entirely (see {@link lspEnabled}). Everything else — line numbers, find,
 * Ctrl+S save, dirty tracking — is source-independent.
 */
export function FileEditor({
  source,
  onDirtyChange,
  revealLine = null,
  revealToken = 0,
  onNavigate,
  pendingEdits = null,
  pendingEditsToken = 0,
  onPendingEditsConsumed,
  onRenameApplied,
  onRenameNote,
}: {
  /** What backs this tab. A FileEditor is keyed by its identity and never rebinds. */
  /**
   * What backs this tab. Deliberately not `EditorSource`: a diff is rendered by
   * `DiffPane`, and letting one reach the read/write ternaries below would
   * overwrite the real file with the diff buffer. See `EditableSource`.
   */
  source: EditableSource;
  onDirtyChange: (dirty: boolean) => void;
  /**
   * A 1-based line to put the cursor on and scroll to, or null for none.
   *
   * Clamped with `lineToPos` before it reaches CodeMirror, because the number
   * comes from a symbol index that is a snapshot and from a `:123` suffix the
   * user is free to invent. `doc.line()` throws out of range, and that throw
   * would land inside the editor as the palette closes — an unrecoverable-
   * looking crash for a stale line number.
   */
  revealLine?: number | null;
  /**
   * Which request `revealLine` belongs to.
   *
   * This component is keyed by path and deliberately stays mounted while
   * hidden, so a second jump into a file that is already open changes neither
   * the mount nor, when the line is the same, `revealLine`. Reacting to the
   * token is what makes that second jump happen at all; it is also what stops
   * an unrelated re-render from replaying the first one and dragging the cursor
   * back from wherever the user has since scrolled.
   */
  revealToken?: number;
  /**
   * Open another file at a line — a usage, or a definition.
   *
   * The editor never opens anything itself: the tab strip and the request token
   * live in `RunView`/`App`, and a second opener here would be a second model of
   * which files are open. `line` is **1-based**, as everywhere in this app.
   */
  onNavigate: (path: string, name: string, line: number) => void;
  /**
   * A rename another editor performed, sliced to this tab's own file.
   *
   * The request-and-consume monotonic-token pattern `pendingOpen`/`reveal`
   * already use, and for the same reason: a second rename of the same symbol
   * changes no field a consumer could compare, so a plain value would be applied
   * once and then ignored for ever. `RunView` owns the map and slices it on
   * `sameWorkspaceFile`, never on `file.id === path` — a diff tab carries a
   * `path` too, and applying a rename into a diff buffer writes it over the real
   * file.
   */
  pendingEdits?: PendingRenameEdits | null;
  pendingEditsToken?: number;
  /**
   * This tab has taken its share of a rename, so the map may forget the path.
   *
   * Without it the entry outlives the tab: `appliedEditsToken` is a per-mount
   * ref, so closing this file and reopening it re-consumed a rename that
   * finished minutes ago. See `consumeRenameEdits`.
   */
  onPendingEditsConsumed?: (path: string) => void;
  /**
   * A rename finished. The result is handed up rather than acted on here,
   * because only `RunView` knows which files are open and can therefore fan the
   * open-buffer half out — and only it can tell whether a `buffers` entry found
   * an editor at all.
   */
  onRenameApplied?: (rename: {
    oldName: string;
    newName: string;
    result: RenameResult;
  }) => void;
  /** Something worth telling the user about, en route to the notification host. */
  onRenameNote?: (report: RenameReport) => void;
}) {
  /**
   * The tab's stable identity: the build effect's dependency, and — for a
   * workspace file, the only source that talks to a server — the path every LSP
   * call and every `usageCacheKey` uses. For secrets the server is never touched,
   * so this value only ever serves as the effect key.
   */
  const identity = source.kind === "secrets" ? `secrets:${source.project}` : source.path;
  /** Whether this tab participates in the language-server surface. Secrets do not. */
  const lspEnabled = sourceEnablesLsp(source);

  /** Read the file's text, from whichever backend the source names. */
  const readSource = (): Promise<string> =>
    source.kind === "secrets"
      ? api.readProjectSecrets(source.project).then((s) => s.content ?? EMPTY_SECRETS)
      : api.fsReadFile(source.path);

  /** Persist the file's text through whichever backend the source names. */
  const writeSource = (text: string): Promise<void> =>
    source.kind === "secrets"
      ? api.writeProjectSecrets(source.project, text).then(() => {})
      : api.fsWriteFile(source.path, text);

  const hostRef = useRef<HTMLDivElement>(null);
  /** The positioned ancestor the dropdowns are placed within. */
  const wrapRef = useRef<HTMLDivElement>(null);
  const viewRef = useRef<EditorView | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [menu, setMenu] = useState<Menu | null>(null);
  const [note, setNote] = useState<EditorNote | null>(null);
  const [renameField, setRenameField] = useState<RenameField | null>(null);
  /** The last `pendingEditsToken` this tab applied, so a repeat is not re-run. */
  const appliedEditsToken = useRef(0);

  /** The jump asked for, and the token of the last one actually performed. */
  const wantedReveal = useRef<{ line: number; token: number } | null>(null);
  const revealedToken = useRef<number | null>(null);

  /**
   * Perform the pending jump, if there is one and the editor exists yet.
   *
   * Called from two places on purpose. The file is loaded asynchronously, so on
   * a first open the reveal effect runs before there is any view to dispatch
   * into; `build` calls this again once there is one. On a later jump into an
   * already-open file the view is there and the effect does it directly.
   */
  const applyReveal = () => {
    const view = viewRef.current;
    const wanted = wantedReveal.current;
    if (!view || !wanted) return;
    if (revealedToken.current === wanted.token) return;
    revealedToken.current = wanted.token;

    const line = view.state.doc.line(lineToPos(view.state.doc.lines, wanted.line));
    view.dispatch({
      selection: { anchor: line.from },
      effects: EditorView.scrollIntoView(line.from, { y: "center" }),
    });
    view.focus();
  };

  useEffect(() => {
    wantedReveal.current = revealLine == null ? null : { line: revealLine, token: revealToken };
    applyReveal();
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [revealLine, revealToken]);

  // Read through refs so the editor is not torn down when they change.
  const handlers = useRef({
    onDirtyChange,
    onNavigate,
    onRenameApplied,
    onRenameNote,
    onPendingEditsConsumed,
  });
  handlers.current = {
    onDirtyChange,
    onNavigate,
    onRenameApplied,
    onRenameNote,
    onPendingEditsConsumed,
  };
  const dirty = useRef(false);

  // -------------------------------------------------------------------------
  // Language-server state.
  //
  // All of it in refs, none of it in React state: it changes on every scroll
  // and every answer, it is consumed by an imperative `dispatch` into
  // CodeMirror rather than by a render, and putting it in state would re-render
  // the component (and so the editor host) dozens of times a second. The only
  // React state the feature owns is the overlay, which really is rendered.
  // -------------------------------------------------------------------------

  /**
   * Which mount these refs belong to.
   *
   * Bumped by the build effect's cleanup, so an answer that arrives after the
   * file was closed — or after the same tab was rebuilt — is dropped instead of
   * being written into the next document's cache.
   */
  const gen = useRef(0);
  /**
   * The document version every cache key is stamped with.
   *
   * Incremented on every `docChanged`, which is what makes a count discardable:
   * an edit anywhere can add or remove a call site, so a number computed over
   * older text is not an answer about this text. Deliberately *not* the LSP
   * version — this one only has to change, not to match anything.
   */
  const docVersion = useRef(0);
  /**
   * The version of the text the server has actually been told about.
   *
   * The gap between this and {@link docVersion} is the whole reason a count can be
   * wrong: `lspChangeDocument` is a round trip, the user keeps typing during it,
   * and a query issued in that window is answered about the *previous* text while
   * being filed under the *current* version's key — a confident number about a
   * document nobody is looking at. `requestVisible` therefore refuses to ask
   * anything at all unless these two agree.
   */
  const syncedVersion = useRef(0);
  const anchors = useRef<DeclarationAnchor[]>([]);
  // One store rather than a map and a set: which answers are this version's final
  // word — and which are worth going back for — is a decision, and it lives in
  // `usagesLogic` where a test can see it.
  const answers = useRef(newUsageAnswers());
  const queue = useRef<UsageJob[]>([]);
  const active = useRef(0);
  const visible = useRef<VisibleLines | null>(null);
  const changeTimer = useRef<number | null>(null);
  const anchorTimer = useRef<number | null>(null);
  /** True once `lspOpenDocument` has succeeded; nothing else may be sent before. */
  const opened = useRef(false);
  /**
   * True from the instant a `didOpen` is *sent*, which is when the `didClose`
   * becomes owed.
   *
   * Not the same as {@link opened}, and the difference is a leak: `opened` is set
   * inside the promise's `.then`, behind the generation check, so a tab closed
   * during a slow open (routine while Roslyn is starting) never sets it — while the
   * backend did receive the `didOpen` and would keep that file's unsaved buffer
   * open for ever, answering later references queries with call sites from text
   * that exists nowhere.
   */
  const openSent = useRef(false);
  /**
   * Why the server's copy of this buffer is not current, or `null`.
   *
   * While this is set no usages are requested at all. A count computed against
   * text the server never received is the wrong-answer failure this whole
   * subsystem is built to avoid, and "no number yet" is the honest alternative.
   */
  const syncError = useRef<string | null>(null);

  /**
   * Redraw every inline row from the current anchors and what is known about
   * them.
   *
   * The three states come straight from the cache: an answer, a request in
   * flight, or nothing asked yet. No phrasing and no counting happen here —
   * `usageRowView` owns both, which is why a row for a starting server cannot
   * accidentally be given a zero.
   */
  const pushRows = () => {
    const view = viewRef.current;
    if (!view) return;
    // Nothing will be answered while the server's copy of the buffer is stale, and
    // the anchor lines themselves were derived from the text before the edit — so
    // rows drawn now would sit above the wrong declarations *and* never fill in.
    // An empty set is the honest picture; the corner badge says why.
    if (syncError.current !== null) {
      view.dispatch({ effects: setUsageRows.of([]) });
      return;
    }
    const version = docVersion.current;
    const rows: UsageRowSpec[] = anchors.current.map((anchor) => {
      const key = usageCacheKey(identity, anchor.id, version);
      return { anchor, view: usageRowView(usageStateFor(answers.current, key)) };
    });
    view.dispatch({ effects: setUsageRows.of(rows) });
  };

  /** Drain the queue up to the concurrency bound. */
  const pump = () => {
    while (active.current < MAX_INFLIGHT_USAGES) {
      const job = queue.current.shift();
      if (!job) return;
      const mine = gen.current;
      active.current += 1;
      // `selectionLine`/`character`, not `line`: the row is drawn at the start
      // of the declaration but the question has to be aimed at the identifier.
      api
        .lspFindUsages(identity, job.anchor.selectionLine, job.anchor.character)
        .then((result) => {
          if (gen.current === mine) recordUsageAnswer(answers.current, job.key, result);
        })
        .catch((e) => {
          if (gen.current === mine) {
            recordUsageAnswer(answers.current, job.key, failedUsageResult(api.errorMessage(e)));
          }
        })
        .finally(() => {
          active.current -= 1;
          answers.current.inFlight.delete(job.key);
          if (gen.current !== mine) return;
          if (job.version === docVersion.current) pushRows();
          pump();
        });
    }
  };

  /**
   * Ask about the anchors near the viewport, and about nothing else.
   *
   * Cached keys are skipped, which is what makes scrolling away and back free —
   * the viewport is deliberately absent from `usageCacheKey`, because scrolling
   * changes nothing about the answer.
   *
   * **Nothing is asked while the server's copy of the buffer is not current.** The
   * guard is here rather than in the callers: there are three of them, only one had
   * it, and the two that did not (the anchors reply, and a click on a row whose
   * answer has been invalidated) are reachable in exactly the window where an edit
   * is still in flight. A query issued then is answered about the previous text and
   * filed under this version's key, where nothing will ever correct it.
   */
  const requestVisible = () => {
    if (!opened.current || syncError.current !== null) return;
    if (changeTimer.current !== null) return;
    if (docVersion.current !== syncedVersion.current) return;
    const span = visible.current;
    if (!span) return;
    const version = docVersion.current;
    const wanted = visibleAnchors(anchors.current, span.firstVisibleLine, span.lastVisibleLine);

    // Prune before enqueuing, or a drag-scroll through a long file puts the rows
    // now on screen behind every declaration it passed — each of them a
    // workspace-wide search, answered serially, four at a time.
    const { keep, dropped } = retainUsageJobs(
      queue.current,
      new Set(wanted.map((anchor) => anchor.id)),
      version,
    );
    queue.current = keep;
    for (const key of dropped) answers.current.inFlight.delete(key);

    for (const anchor of wanted) {
      const key = usageCacheKey(identity, anchor.id, version);
      // Not "is there an answer": a `loading`, `starting` or `failed` answer is
      // shown and still asked again, up to a bound. Caching those as final left
      // every row frozen on its reason after a mid-session server restart.
      if (!shouldAskUsages(answers.current, key)) continue;
      answers.current.inFlight.add(key);
      queue.current.push({ anchor, key, version });
    }
    pushRows();
    pump();
  };

  /**
   * Fetch the declaration anchors, retrying while the server is still coming up.
   *
   * `starting` and `loading` are the two outcomes that will become a different
   * answer on their own — Roslyn takes tens of seconds to load a solution — so
   * they are retried, and every other outcome is final and becomes the corner
   * badge. Retrying `failed` or `notConfigured` would be a poll for a server
   * that is not going to appear.
   */
  const loadAnchors = (attempt = 0) => {
    const mine = gen.current;
    // A chain per settled edit, all writing to one timer slot, is a growing
    // background poll whose earlier links can no longer be cancelled. The newest
    // chain supersedes the others; cleanup then really does cancel the one that is
    // left. (Harmless at the recursive call: it clears its own handle first.)
    if (anchorTimer.current !== null) {
      window.clearTimeout(anchorTimer.current);
      anchorTimer.current = null;
    }
    api
      .lspDeclarationAnchors(identity)
      .then((result) => {
        if (gen.current !== mine) return;
        anchors.current = result.anchors;
        if (result.outcome === "ready") {
          // A `ready` answer may still carry a caveat, and it takes the same
          // short-phrase/long-detail shape as every other state on this path.
          // The badge is one nowrap line clipped at 45ch (`.usages-note`), and the
          // caveat's actionable clause is its *last* — "…so a count may be low" —
          // so putting the sentence in `text` truncated away the only part worth
          // reading and left "this answer was taken from a server that ne…".
          setNote(
            result.message === null
              ? null
              : { text: "Usages may be incomplete", detail: result.message },
          );
        } else if (shouldRetryAnchors(result.outcome, attempt)) {
          const phrase = availabilityPhrase(result.outcome);
          setNote({ text: phrase.text, detail: result.message });
          anchorTimer.current = window.setTimeout(() => {
            anchorTimer.current = null;
            loadAnchors(attempt + 1);
          }, ANCHOR_RETRY_MS);
        } else if (result.outcome === "starting" || result.outcome === "loading") {
          // The retries ran out. Leaving the badge on "loading…" would be a
          // present-tense claim with nothing behind it: the user reads it as work
          // in progress and waits for a state that will never be re-read. Say that
          // waiting stopped, and how to start again.
          setNote({
            text: "Usages gave up waiting",
            detail:
              `${availabilityPhrase(result.outcome).text} — it has taken longer than ` +
              `${Math.round((ANCHOR_RETRY_LIMIT * ANCHOR_RETRY_MS) / 1000)} seconds and nothing ` +
              "is checking any more. Close and reopen this tab to try again." +
              (result.message === null ? "" : ` (${result.message})`),
          });
        } else {
          const phrase = availabilityPhrase(result.outcome);
          setNote({ text: phrase.text, detail: result.message });
        }
        pushRows();
        requestVisible();
      })
      .catch((e) => {
        if (gen.current !== mine) return;
        setNote({ text: "Usages unavailable", detail: api.errorMessage(e) });
      });
  };

  /**
   * Send the buffer now and answer when the server has it.
   *
   * Split out of {@link flushChange} for the rename, which cannot use a
   * fire-and-forget flush: F2 must open its field **only** once the server holds
   * this exact text, so it needs the promise, and it needs a rejection it can
   * refuse with rather than a silently-swallowed one. `flushChange` keeps the
   * debounce and the not-opened-yet reschedule; this is only the send.
   *
   * Rejects with whatever the call rejected with, *after* recording it as
   * {@link syncError} and taking the rows down — so both callers see the same
   * state and only the wording differs.
   */
  const sendChange = (): Promise<void> => {
    if (!lspEnabled) return Promise.resolve();
    const view = viewRef.current;
    if (!view) return Promise.resolve();
    if (!opened.current) {
      return Promise.reject(
        new Error("the language server has not been told about this file yet"),
      );
    }
    const mine = gen.current;
    // Captured before the send, not read in the reply: the user goes on typing
    // during the round trip, and what landed on the server is this text.
    const sent = docVersion.current;
    return api
      .lspChangeDocument(identity, view.state.doc.toString())
      .then(() => {
        if (gen.current !== mine) return;
        syncError.current = null;
        syncedVersion.current = sent;
        // The anchors move and can appear or disappear with the edit, so they
        // are re-derived rather than reused.
        loadAnchors();
      })
      .catch((e) => {
        // The generation guard covers the *state* writes, not the rejection: a
        // caller waiting on this promise is owed an answer whether or not this
        // mount is still the current one, and swallowing the error here would
        // resolve it successfully and let a rename proceed on a stale flush.
        if (gen.current !== mine) throw e;
        syncError.current = api.errorMessage(e);
        setNote({
          text: "Usages paused",
          detail:
            "This file could not be sent to the language server, so no usages are being " +
            `counted until the next edit gets through: ${syncError.current}`,
        });
        // Take the rows down with it. They were placed from anchors derived before
        // the edit and nothing will answer them, so leaving them there is a row
        // that reads exactly like "not asked yet" — for ever, above the wrong line.
        pushRows();
        // Rethrown rather than absorbed: `flushChange` ignores it (the badge above
        // is the report), and the rename path needs it as its refusal reason.
        throw e;
      });
  };

  /** Send the buffer, then re-derive anchors and counts against it. */
  const flushChange = () => {
    // No server for this source: nothing to send, and returning here stops the
    // "not opened yet, try again" reschedule below from spinning a timer forever.
    if (!lspEnabled) return;
    if (!viewRef.current) return;
    if (!opened.current) {
      // Typing inside the open round trip is routine — guaranteed while a server
      // is starting, since `opened` only flips when `lspOpenDocument` resolves.
      // Returning silently drops that edit permanently: nothing re-sends it, so
      // the server keeps the text as it was read off disk while the rows claim the
      // current version. Wait and try again instead.
      if (changeTimer.current !== null) window.clearTimeout(changeTimer.current);
      changeTimer.current = window.setTimeout(() => {
        changeTimer.current = null;
        flushChange();
      }, CHANGE_DEBOUNCE_MS);
      return;
    }
    void sendChange().catch(() => {
      /* already recorded as `syncError` and shown as the corner badge */
    });
  };

  // -------------------------------------------------------------------------
  // Overlay placement and the two click paths.
  // -------------------------------------------------------------------------

  /**
   * Read the wrapper's rectangle and hand the arithmetic to
   * `usagesLogic.placeMenu`.
   *
   * The measurement is the only part that needs a DOM; the clamping is not, and it
   * has edges (a pane narrower than the menu) that a test should be able to reach.
   */
  const place = (x: number, y: number): Placement => {
    const rect = wrapRef.current?.getBoundingClientRect();
    return placeMenu(x, y, rect ?? null);
  };

  /** Open a target, if it is somewhere that can be opened. */
  const jumpTo = (target: Pick<Target, "path" | "line">) => {
    if (target.path === null) return;
    setMenu(null);
    handlers.current.onNavigate(target.path, baseName(target.path), target.line);
  };

  const openUsages = (click: UsageRowClick) => {
    if (click.view.action.kind !== "dropdown") return;
    const key = usageCacheKey(identity, click.anchor.id, docVersion.current);
    const held = usageStateFor(answers.current, key);
    const answered = held.status === "answered" ? held.result : null;
    const at = place(click.rect.left, click.rect.bottom);
    if (!answered) {
      // The document changed between the row being drawn and being clicked, so
      // the list that goes with that count no longer describes this text. Whether
      // anything is being done about that depends on the sync: while it is broken
      // `requestVisible` is a no-op, and promising a recomputation that is not
      // happening is worse than the count being stale.
      setMenu({
        kind: "note",
        place: at,
        message:
          syncError.current === null
            ? "The file changed since this count was taken; it is being recomputed."
            : "The file changed since this count was taken, and it cannot be recounted: " +
              `this file could not be sent to the language server (${syncError.current}).`,
      });
      requestVisible();
      return;
    }
    setMenu({
      kind: "usages",
      place: at,
      anchor: click.anchor,
      groups: groupUsages(answered.usages),
      total: click.view.action.total,
      truncated: answered.truncated,
      message: answered.message,
    });
  };

  const goto = (request: GotoRequest) => {
    const mine = gen.current;
    const at = place(request.x, request.y);
    setMenu({ kind: "note", place: at, message: "Looking for the definition…" });
    api
      .lspGotoDefinition(identity, request.line, request.character)
      .then((result) => {
        if (gen.current !== mine) return;
        const action = definitionAction(result);
        if (action.kind === "jump") {
          setMenu(null);
          jumpTo(action.target);
        } else if (action.kind === "pick") {
          setMenu({
            kind: "definition",
            place: at,
            groups: action.groups,
            message: action.message,
            provisional: partialAnswerNote(action.outcome, action.message),
          });
        } else {
          setMenu({ kind: "note", place: at, message: action.message });
        }
      })
      .catch((e) => {
        if (gen.current !== mine) return;
        setMenu({ kind: "note", place: at, message: api.errorMessage(e) });
      });
  };

  // -------------------------------------------------------------------------
  // F2 — rename symbol.
  //
  // The ordering guarantee lives here and nowhere else: the ranges a rename
  // answers with are applied to *this* text, so they must have been computed
  // from *this* text. `renameReadiness` decides; this only obeys it.
  // -------------------------------------------------------------------------

  /** A refusal, in the same overlay the goto path uses for one. */
  const showRenameNote = (message: string) => {
    const view = viewRef.current;
    const at = view ? view.coordsAtPos(view.state.selection.main.head) : null;
    setMenu({ kind: "note", place: place(at?.left ?? 0, at?.bottom ?? 0), message });
  };

  /**
   * Ask the server whether this position can be renamed, then open the field.
   *
   * The prefill comes from the **buffer**, not from the answer: real Roslyn
   * replies with a bare range and no placeholder (measured against 2.140.9), so
   * `identifierAt` is the live path here rather than a fallback, and without it
   * the box opens empty. A `placeholder` is taken when a server does send one.
   */
  const openRenameField = () => {
    const view = viewRef.current;
    if (!view) return;
    const head = view.state.selection.main.head;
    const docLine = view.state.doc.lineAt(head);
    const character = head - docLine.from;
    const found = identifierAt(docLine.text, character);
    if (!found) {
      showRenameNote("Put the caret on a name to rename it.");
      return;
    }

    // Anchored on the identifier's own start rather than the caret, so the field
    // does not jump about depending on where in the word F2 was pressed.
    const coords = view.coordsAtPos(docLine.from + found.start);
    const rect = wrapRef.current?.getBoundingClientRect();
    const at = placeRenameField(
      coords?.left ?? 0,
      coords?.bottom ?? 0,
      rect ? { left: rect.left, top: rect.top, width: rect.width, height: rect.height } : null,
    );

    const mine = gen.current;
    const version = docVersion.current;
    api
      .lspPrepareRename(identity, docLine.number, character)
      .then((prepare) => {
        if (gen.current !== mine) return;
        const offer = renameOffer(prepare);
        if (offer.kind === "refuse") {
          showRenameNote(offer.reason);
          return;
        }
        setMenu(null);
        setRenameField({
          place: at,
          oldName: found.text,
          value: offer.placeholder ?? found.text,
          line: docLine.number,
          character,
          docVersion: version,
          caveat: offer.caveat,
          confirmed: false,
          error: null,
        });
      })
      .catch((e) => {
        if (gen.current !== mine) return;
        showRenameNote(api.errorMessage(e));
      });
  };

  /**
   * F2. Refuses, flushes, or opens — in that order.
   *
   * The flush is the whole point: the debounce is cancelled and the buffer sent
   * *now*, and the field opens only from that promise's `.then`. A rejection is
   * the refusal reason, which is why `sendChange` rethrows.
   */
  const startRename = () => {
    if (!viewRef.current) return;
    const readiness = renameReadiness({
      lspEnabled,
      opened: opened.current,
      changePending: changeTimer.current !== null,
      docVersion: docVersion.current,
      syncedVersion: syncedVersion.current,
      syncError: syncError.current,
    });
    if (readiness.kind === "refuse") {
      showRenameNote(readiness.reason);
      return;
    }
    if (readiness.kind === "ready") {
      openRenameField();
      return;
    }
    if (changeTimer.current !== null) {
      window.clearTimeout(changeTimer.current);
      changeTimer.current = null;
    }
    const mine = gen.current;
    sendChange()
      .then(() => {
        if (gen.current !== mine) return;
        openRenameField();
      })
      .catch((e) => {
        if (gen.current !== mine) return;
        showRenameNote(
          "This file could not be sent to the language server, so a rename would be computed " +
            `against text it has never seen: ${api.errorMessage(e)}`,
        );
      });
  };

  /** Enter in the rename field. */
  const commitRename = (field: RenameField) => {
    const accepted = acceptedNewName(field.value, field.oldName);
    if (accepted.kind === "reject") {
      setRenameField({ ...field, error: accepted.reason });
      return;
    }
    // Re-checked at Enter, not only at F2: a reveal, a jump or another editor's
    // rename can move this document while the focus is in the input, and the
    // position the question is aimed at was read from the older text.
    if (
      field.docVersion !== docVersion.current ||
      docVersion.current !== syncedVersion.current ||
      changeTimer.current !== null
    ) {
      setRenameField({
        ...field,
        error:
          "The file changed while you were typing, so this position may no longer be the " +
          "symbol. Press Escape and try again.",
      });
      return;
    }
    // The one case that gets a confirmation, because the user chose no preview
    // for the normal path and this is not the normal path: a server promoted at
    // the readiness ceiling may have **missed call sites**, which leaves the old
    // name behind in files that then do not compile.
    if (field.caveat !== null && !field.confirmed) {
      const warning = provisionalRenameWarning({ outcome: "ready", message: field.caveat });
      setRenameField({
        ...field,
        confirmed: true,
        error: `${warning ?? field.caveat} Press Enter again to rename anyway.`,
      });
      return;
    }

    setRenameField(null);
    viewRef.current?.focus();
    const oldName = field.oldName;
    const newName = accepted.name;
    api
      .lspRename(identity, field.line, field.character, oldName, newName)
      .then((result) => {
        // Deliberately **not** guarded on the generation. The writes have already
        // happened by the time this resolves, so a tab closed mid-rename must not
        // discard the report — it is the only record that files on disk changed.
        // Nothing below touches this component's state, so a late call is safe.
        handlers.current.onRenameApplied?.({ oldName, newName, result });
      })
      .catch((e) => {
        handlers.current.onRenameNote?.({
          kind: "error",
          title: `${oldName} was not renamed`,
          detail: api.errorMessage(e),
        });
      });
  };

  /**
   * Apply another editor's rename to this buffer, as one transaction.
   *
   * One transaction so Ctrl+Z in *this* tab reverts *this* file's part of the
   * rename in a single step. A multi-file rename is not one undo and this app
   * does not pretend it is — see the notification `renameSummary` writes.
   *
   * The stale-mirror check runs here because this is the only layer that has the
   * buffer's text. It refuses the file only when **no** edit lands on the old
   * name: a single non-matching edit is routinely legitimate (a rename inside a
   * comment, a TypeScript shorthand property being expanded), and refusing over
   * one would refuse correct renames.
   */
  const applyRenameEdits = (pending: PendingRenameEdits) => {
    const view = viewRef.current;
    if (!view) {
      handlers.current.onRenameNote?.({
        kind: "error",
        title: "Part of a rename was not applied",
        detail:
          `${pending.path} is open but its editor is not ready, so ${pending.edits.length} ` +
          "edits were not applied and it still holds the old name.",
      });
      return;
    }
    const doc = view.state.doc;
    // Before any position arithmetic: `lineToPos` and `posAt` both clamp, so an
    // edit naming a line this buffer does not have would be applied *somewhere*
    // rather than refused. That is the condition `edits::apply` refuses outright
    // for a closed file, and the buffer is the copy holding unsaved work.
    const bounds = documentBoundsVerdict(doc.lines, pending.edits, pending.path);
    if (bounds.kind === "refuse") {
      handlers.current.onRenameNote?.({
        kind: "error",
        title: "Part of a rename was refused",
        detail: bounds.reason,
      });
      return;
    }
    const spans = pending.edits.map((edit) => {
      const startLine = doc.line(lineToPos(doc.lines, edit.startLine));
      const endLine = doc.line(lineToPos(doc.lines, edit.endLine));
      return {
        from: posAt(startLine.from, startLine.length, edit.startCharacter),
        to: posAt(endLine.from, endLine.length, edit.endCharacter),
        insert: edit.newText,
        startText: startLine.text,
        startCharacter: edit.startCharacter,
      };
    });
    const verdict = bufferVerdict(
      spans.map((span) =>
        replacedTextMatches(span.startText, span.startCharacter, pending.oldName),
      ),
      pending.path,
    );
    if (verdict.kind === "refuse") {
      handlers.current.onRenameNote?.({
        kind: "error",
        title: "Part of a rename was refused",
        detail: verdict.reason,
      });
      return;
    }
    if (spans.length > 0) {
      view.dispatch({
        changes: spans.map((span) => ({ from: span.from, to: span.to, insert: span.insert })),
      });
    }
    if (verdict.note !== null) {
      handlers.current.onRenameNote?.({
        kind: "warning",
        title: "Renamed, with something worth checking",
        detail: verdict.note,
      });
    }
  };

  /**
   * The callbacks the extension is given, read through a ref.
   *
   * The extension captures these once, when the editor is built, and the editor
   * is built once per file — so the functions handed to it must be indirections
   * rather than the closures of one render, or every callback would see the
   * state as it was at mount.
   */
  const lsp = useRef({
    openUsages,
    goto,
    pushRows,
    requestVisible,
    flushChange,
    startRename,
    applyRenameEdits,
  });
  lsp.current = {
    openUsages,
    goto,
    pushRows,
    requestVisible,
    flushChange,
    startRename,
    applyRenameEdits,
  };

  useEffect(() => {
    let cancelled = false;

    async function build() {
      let content: string;
      try {
        content = await readSource();
      } catch (e) {
        if (!cancelled) setError(api.errorMessage(e));
        return;
      }
      if (cancelled || !hostRef.current) return;

      const setDirty = (next: boolean) => {
        if (dirty.current === next) return;
        dirty.current = next;
        handlers.current.onDirtyChange(next);
      };

      const save = (view: EditorView) => {
        const text = view.state.doc.toString();
        writeSource(text)
          .then(() => {
            setDirty(false);
            setError(null);
          })
          .catch((e) => setError(api.errorMessage(e)));
        return true;
      };

      const extensions: Extension[] = [
        lineNumbers(),
        history(),
        // The caret and selection. Without these CodeMirror falls back to the
        // native caret, which the WebView does not paint — so the user has no
        // idea where they are typing. `drawSelection` renders its own `.cm-cursor`
        // (coloured in the theme below); `dropCursor` marks the drop point when
        // text is dragged.
        drawSelection(),
        dropCursor(),
        // Ctrl+F opens an in-file find panel at the top. The chord is claimed by
        // the app-level `console.find` binding (window, capture phase) and
        // `dispatchShortcut` `preventDefault`s it, so `searchKeymap` below never
        // sees it directly — the `console.find` handler registered further down
        // opens this panel programmatically when the caret is in this editor, and
        // `OutputConsole` declines while a file editor is focused so the two
        // never contend. `highlightSelectionMatches` underlines other copies of
        // the current selection, the same affordance the console search gives.
        search({ top: true }),
        highlightSelectionMatches(),
        keymap.of([
          { key: "Mod-s", run: save },
          // Ctrl+/ toggles line comments. `defaultKeymap` binds this too, but it
          // is listed explicitly and ahead of it with `preventDefault` so the
          // WebView cannot claim the chord first (it otherwise swallows it before
          // CodeMirror is reached). A no-op on a language without comment tokens,
          // e.g. JSON.
          { key: "Mod-/", run: toggleComment, preventDefault: true },
          // Ctrl+G jumps to a line, as in Rider. `searchKeymap` already binds
          // `gotoLine` to Alt-g; this adds the familiar chord ahead of it, with
          // `preventDefault` so the WebView cannot claim Ctrl+G first.
          { key: "Mod-g", run: gotoLine, preventDefault: true },
          ...searchKeymap,
          indentWithTab,
          ...defaultKeymap,
          ...historyKeymap,
        ]),
        EditorView.updateListener.of((update) => {
          if (!update.docChanged) return;
          setDirty(true);
          // Every count on screen was computed over the previous text. Bumping
          // the version makes them all cache misses, so the rows fall back to
          // their idle text at the next redraw rather than keeping a number
          // that is no longer about this document.
          docVersion.current += 1;
          // And no key of the previous version can ever be read again — the version
          // is in every key and the backend's anchor ids embed the declaration's
          // position, so an edit that shifts lines renames them too. Without this
          // the map grows by a screenful of answers, each up to 500 snippets, at
          // every typing pause for as long as the tab stays open.
          clearUsageAnswers(answers.current);
          // A secrets tab talks to no server, so there is nothing to flush and no
          // timer to arm — the rest of this listener (dirty + version bump) is
          // harmless and stays.
          if (!lspEnabled) return;
          if (changeTimer.current !== null) window.clearTimeout(changeTimer.current);
          changeTimer.current = window.setTimeout(() => {
            changeTimer.current = null;
            lsp.current.flushChange();
          }, CHANGE_DEBOUNCE_MS);
        }),
        EditorView.theme({
          "&": { height: "100%" },
          ".cm-scroller": { overflow: "auto" },
          // The drawn caret and selection, coloured for the app's dark theme.
          // CodeMirror's base caret is black (invisible here) and, more to the
          // point, `display: none` until a strict focused child-combinator chain
          // matches — which does not hold in this WebView, so the caret stayed
          // hidden even though the selection layer showed. The `display: block`
          // below rides a looser two-class selector that outranks the base hide,
          // forcing the caret on whenever the editor is focused.
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
        // The whole usages/goto surface is workspace-only: a secrets file is JSON
        // no server knows about, so its inline rows and click handlers are simply
        // not installed rather than installed and always answering "unavailable".
        ...(lspEnabled
          ? usagesExtension({
              onRowClick: (click) => lsp.current.openUsages(click),
              onGoto: (request) => lsp.current.goto(request),
              onVisibleLinesChange: (span) => {
                visible.current = span;
                lsp.current.pushRows();
                // No guard here: `requestVisible` refuses on its own while a change
                // is owed to the server. It has to, because two of its three callers
                // are not on this path.
                lsp.current.requestVisible();
              },
            })
          : []),
        ...languageFor(sourceLanguageHint(source)),
        ...editorColors,
      ];

      try {
        const view = new EditorView({
          state: EditorState.create({ doc: content, extensions }),
          parent: hostRef.current,
        });
        viewRef.current = view;
        setError(null);
        // A jump requested while the file was still loading.
        applyReveal();

        // The editor's own text, not the string just read from disk: they are
        // the same here today, and taking it from the buffer is what keeps this
        // correct if the editor ever starts with anything else.
        const mine = gen.current;
        const sent = docVersion.current;
        // A secrets tab opens no document and so owes no `didClose`; the teardown
        // below is guarded on `openSent`, which stays false here.
        if (!lspEnabled) return;
        // Set before the call, not after it resolves: from here on a `didClose` is
        // owed whatever happens to this component.
        openSent.current = true;
        api
          .lspOpenDocument(identity, view.state.doc.toString())
          .then(() => {
            if (gen.current !== mine) return;
            opened.current = true;
            syncedVersion.current = sent;
            loadAnchors();
          })
          .catch((e) => {
            if (gen.current !== mine) return;
            // No server, no workspace, an unsupported language: all normal, and
            // none of them may take the file off the screen.
            setNote({ text: "Usages unavailable", detail: api.errorMessage(e) });
          });
      } catch (e) {
        setError(e instanceof Error ? e.message : String(e));
      }
    }

    void build();
    return () => {
      cancelled = true;
      // Invalidate every in-flight answer before anything else: the callbacks
      // above compare against this and drop what arrives late.
      gen.current += 1;
      if (changeTimer.current !== null) window.clearTimeout(changeTimer.current);
      if (anchorTimer.current !== null) window.clearTimeout(anchorTimer.current);
      changeTimer.current = null;
      anchorTimer.current = null;
      queue.current = [];
      clearUsageAnswers(answers.current);
      anchors.current = [];
      visible.current = null;
      syncError.current = null;
      syncedVersion.current = 0;
      if (openSent.current) {
        opened.current = false;
        openSent.current = false;
        // A `didClose` is owed to the server whether or not anyone is listening
        // for the result — and owed from the moment the `didOpen` was *sent*, not
        // from the moment it resolved, or a tab closed inside a slow open leaves
        // the server holding an unsaved buffer for ever.
        api.lspCloseDocument(identity).catch(() => {
          /* the session is gone; the server was told or never knew */
        });
      }
      viewRef.current?.destroy();
      viewRef.current = null;
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [identity]);

  /**
   * F2, for whichever of these editors is actually on screen.
   *
   * Several `FileEditor`s are mounted at once inside `display: none` wrappers,
   * so every one of them registers this handler and `shouldHandleRename` is what
   * keeps exactly one of them acting. `executeCommand` walks handlers
   * newest-first and takes the first that does not return `false`, which is the
   * same fall-through `SqlView`'s own `run.run` handler uses.
   *
   * Visibility is `getClientRects().length > 0` and **not** `offsetParent`,
   * which is `null` for a `position: fixed` element. Measured on the frame
   * rather than on the CodeMirror host, because the Project tab hides the whole
   * editor area behind a diff tab.
   *
   * Registered once, and it reads the current closures through `lsp.current` —
   * the same indirection the extension's callbacks use, for the same reason.
   */
  useEffect(
    () =>
      registerCommand("refactor.rename", () => {
        const rendered = (wrapRef.current?.getClientRects().length ?? 0) > 0;
        // F2 is `allowInText`, so it arrives with focus in a terminal, the Notes
        // textarea or the search box — none of which unmount this editor. The
        // list is `askLogic.TEXT_ENTRY_ANCESTORS`'s rule, `.xterm` included for
        // the same measured reason; anything inside this editor's own frame is
        // this editor and does not count.
        const active = document.activeElement;
        const focusElsewhere =
          active !== null &&
          !(wrapRef.current?.contains(active) ?? false) &&
          active.closest('.xterm, input, textarea, select, [contenteditable="true"]') !== null;
        if (!shouldHandleRename({ rendered, focusElsewhere })) return false;
        lsp.current.startRename();
        return true;
      }),
    [],
  );

  /**
   * Ctrl+F → the in-file find panel.
   *
   * The chord is bound to `console.find` at the app level and `dispatchShortcut`
   * `preventDefault`s it (the "a bound key never reaches the WebView" rule), so
   * CodeMirror's own `searchKeymap` never sees it. This handler opens the panel
   * programmatically instead, using the same multi-handler fall-through as
   * `refactor.rename` and `SqlView`'s `run.run`: it acts only when the caret is
   * inside *this* editor's frame and otherwise returns `false`, and
   * `OutputConsole`'s `console.find` declines whenever any file editor is
   * focused — so exactly one of them acts regardless of registration order.
   */
  useEffect(
    () =>
      registerCommand("console.find", () => {
        const view = viewRef.current;
        if (!view) return false;
        const active = document.activeElement;
        if (active === null || !(wrapRef.current?.contains(active) ?? false)) return false;
        openSearchPanel(view);
        return true;
      }),
    [],
  );

  /**
   * Another editor's rename, sliced to this file.
   *
   * Keyed on the token rather than on the value: a second rename of the same
   * symbol produces an identical `pendingEdits` object and would otherwise never
   * be applied. The token is monotonic and owned by `RunView`.
   */
  useEffect(() => {
    if (!pendingEdits || pendingEditsToken === appliedEditsToken.current) return;
    appliedEditsToken.current = pendingEditsToken;
    lsp.current.applyRenameEdits(pendingEdits);
    // Consumed whatever the outcome was — applied, refused or reported. The
    // entry has had its one chance at this file, and a rename that has already
    // been reported must not be re-applied to a later mount of the same tab.
    handlers.current.onPendingEditsConsumed?.(pendingEdits.path);
  }, [pendingEdits, pendingEditsToken]);

  // CodeMirror caches character metrics; a CSS font-size change needs saying
  // out loud (see `editorFontSize.ts`).
  useEffect(() => onEditorFontSizeChange(() => viewRef.current?.requestMeasure()), []);

  // Escape closes the overlay. On the window in the capture phase, like the
  // console's Ctrl+F, because the focus may be inside CodeMirror — which has its
  // own Escape binding — or on the menu itself.
  useEffect(() => {
    if (!menu) return;
    const onKeyDown = (event: KeyboardEvent) => {
      if (event.key !== "Escape") return;
      event.preventDefault();
      event.stopPropagation();
      setMenu(null);
    };
    window.addEventListener("keydown", onKeyDown, true);
    return () => window.removeEventListener("keydown", onKeyDown, true);
  }, [menu]);

  if (error) {
    return (
      <div className="error" style={{ whiteSpace: "pre-wrap" }}>
        {error}
      </div>
    );
  }

  return (
    <div className="editor-frame" ref={wrapRef}>
      <div className="editor-host" ref={hostRef} />

      {note && (
        <div className="usages-note" title={note.detail ?? undefined}>
          {note.text}
        </div>
      )}

      {/* The rename field: an ordinary positioned `<input>`, deliberately not a
          CodeMirror widget and deliberately not a document mutation — see
          `RenameField`'s own doc for both reasons.

          Enter commits, and Escape **and blur** both cancel. Blur cancels
          because a blur can be a stray click somewhere else in the app, and
          committing a rename on a stray click is unrecoverable. */}
      {renameField && (
        <div className="rename-field" style={{ left: renameField.place.left, top: renameField.place.top }}>
          <input
            autoFocus
            className="rename-input"
            value={renameField.value}
            aria-label={`Rename ${renameField.oldName}`}
            onFocus={(event) => event.currentTarget.select()}
            onChange={(event) =>
              setRenameField({ ...renameField, value: event.target.value, error: null })
            }
            onBlur={() => setRenameField(null)}
            onKeyDown={(event) => {
              if (event.key === "Enter") {
                event.preventDefault();
                commitRename(renameField);
              } else if (event.key === "Escape") {
                event.preventDefault();
                // Stopped here so the window-level Escape listener that closes
                // the overlay does not also fire; this input owns the key while
                // it is open.
                event.stopPropagation();
                setRenameField(null);
                viewRef.current?.focus();
              }
            }}
          />
          {renameField.error !== null && (
            <div className="rename-error">{renameField.error}</div>
          )}
        </div>
      )}

      {menu && (
        <>
          <div className="dropdown-backdrop" onClick={() => setMenu(null)} />
          <div
            className="dropdown-menu usages-menu"
            style={{ left: menu.place.left, top: menu.place.top, maxHeight: menu.place.maxHeight }}
          >
            {menu.kind === "note" && <div className="usages-message">{menu.message}</div>}

            {menu.kind === "usages" && (
              <>
                <div className="usages-heading">
                  {menu.anchor.name}
                  {/* The row's own label, not a second pluralisation: the two are
                      one fact, and a count of zero is where they diverge first.
                      The caveat flag is passed for the same reason — a heading
                      reading "No usages" over a message saying the count may be
                      low is the row's old bug, one level down. */}
                  <span className="usages-count">
                    {usageCountLabel(menu.total, menu.message !== null)}
                  </span>
                </div>
                {/* `truncated` means the rows are fewer than the count above,
                    and saying so is the difference between a short list and a
                    wrong one. */}
                {menu.truncated && (
                  <div className="usages-message">
                    Showing the first {countUsageRows(menu.groups)} of{" "}
                    {/* "of at least 900" when the backend qualified the count.
                        Under a heading reading "at least 900 usages", stating 900
                        as the total contradicts the line above it. */}
                    {menu.message === null ? menu.total : `at least ${menu.total}`}.
                  </div>
                )}
                {menu.message !== null && <div className="usages-message">{menu.message}</div>}
                {/* The heading already says "No usages" at zero. A *positive* count
                    with nothing to list is a contradiction, and stating it beats
                    an empty menu under a number. */}
                {menu.groups.length === 0 && menu.total > 0 && (
                  <div className="usages-message">
                    No locations came back with that count, so there is nothing to list here.
                  </div>
                )}
                {menu.groups.map((group) => (
                  <div className="usages-group" key={`${group.path ?? "?"} ${group.label}`}>
                    <div className="usages-file">{group.label}</div>
                    {group.rows.map((row, i) => (
                      <UsageLine
                        key={`${row.usage.line} ${i}`}
                        line={row.usage.line}
                        snippet={row.usage.snippet}
                        highlight={row.usage.highlight}
                        openable={row.openable}
                        onOpen={() => jumpTo({ path: row.usage.path, line: row.usage.line })}
                      />
                    ))}
                  </div>
                ))}
              </>
            )}

            {menu.kind === "definition" && (
              <>
                {/* Shown once, above the groups. The backend sends one message
                    for three lists and names the group it concerns in prose, so
                    repeating it under each empty group would put an
                    implementations sentence under Type definitions. */}
                {menu.message !== null && <div className="usages-message">{menu.message}</div>}
                {/* A list can be non-empty and still provisional: one `outcome`
                    covers three lists, and a loading server answers `definition`
                    while it is still resolving implementations. */}
                {menu.provisional !== null && (
                  <div className="usages-message">{menu.provisional}</div>
                )}
                {menu.groups.map((group) => (
                  <div className="usages-group" key={group.label}>
                    <div className="usages-file">{group.label}</div>
                    {/* "None." only when there was nothing to have been refused —
                        one message covers three lists and names its group in prose,
                        so an empty group beside a message may be the refused one,
                        and saying "None." there is "unsupported reads as there are
                        none" by another route. */}
                    {group.empty && (
                      <div className="usages-message">{emptyGroupNote(menu.message)}</div>
                    )}
                    {group.targets.map((target, i) => (
                      <UsageLine
                        key={`${target.label} ${target.line} ${i}`}
                        label={target.label}
                        line={target.line}
                        snippet={target.snippet}
                        highlight={null}
                        openable={target.path !== null}
                        onOpen={() => jumpTo(target)}
                      />
                    ))}
                  </div>
                ))}
              </>
            )}
          </div>
        </>
      )}
    </div>
  );
}

/**
 * One clickable location: its line number, and its snippet with the match
 * underlined.
 *
 * Unopenable rows — a `source-generated:` or metadata document — are still
 * listed, because dropping them would contradict the count above, and they say
 * on hover why nothing happens when they are clicked.
 */
function UsageLine({
  label,
  line,
  snippet,
  highlight,
  openable,
  onOpen,
}: {
  /** Only the goto picker shows this; the usages list groups by file already. */
  label?: string;
  line: number;
  snippet: string;
  highlight: Highlight | null;
  openable: boolean;
  onOpen: () => void;
}) {
  const parts = snippetParts(snippet, highlight);
  return (
    <div
      className={`dropdown-item usages-row-item${openable ? "" : " inert"}`}
      role={openable ? "button" : undefined}
      aria-disabled={openable ? undefined : true}
      title={openable ? undefined : INERT_LOCATION_REASON}
      onClick={openable ? onOpen : undefined}
    >
      <span className="usages-line">{label ? `${label}:${line}` : line}</span>
      <span className="usages-snippet">
        {parts.before}
        <span className="usages-match">{parts.match}</span>
        {parts.after}
      </span>
    </div>
  );
}

/**
 * Every decision F2 makes, kept out of the editor so it can be tested.
 *
 * `FileEditor` is a CodeMirror host and cannot be tested at all — vitest runs in
 * the **node** environment, so there is no DOM here and nothing in this module
 * may touch one. What is left in the component is measurement (a caret's screen
 * coordinates, a wrapper's rectangle), a `dispatch`, and the promise plumbing.
 *
 * # The two rules this module exists to keep
 *
 * **Abstain rather than guess.** A rename is the one editor action whose wrong
 * answer is not a wrong number on screen but a corrupted file, so every function
 * here has a refusal branch and none of them has a fallback. In particular
 * {@link acceptedNewName} deliberately does **not** validate identifiers: a
 * per-language validator written here would be a second, worse opinion than the
 * language server's own, and it would refuse `@class` in C#, non-ASCII
 * identifiers, and `$` in JavaScript. Only the things that are wrong in *every*
 * language are rejected.
 *
 * **One rule, one place.** {@link refusalNote} borrows `usagesLogic`'s
 * {@link availabilityPhrase} instead of writing a second set of words for the
 * same six states — the "one rename, two destinations, one rule" lesson, which
 * is literally about this class of duplication.
 */

import { availabilityPhrase } from "./usagesLogic";
import type { MenuBounds, Placement } from "./usagesLogic";
import type { Availability, BufferEdits, PrepareRenameResult, RenameResult } from "../ipc/types";

// ---------------------------------------------------------------------------
// 1. Whether this editor answers F2 at all, and whether it is safe to ask yet.
// ---------------------------------------------------------------------------

/**
 * What one candidate editor knows about itself when the key is pressed.
 *
 * Deliberately not an element: see this module's own doc. `FileEditor` reads the
 * one fact off the DOM and this module says what it means.
 */
export interface RenameTarget {
  /**
   * Whether this editor is actually being drawn.
   *
   * `getClientRects().length > 0` is the right test and `offsetParent !== null`
   * is **not** — `offsetParent` is `null` for a `position: fixed` element, which
   * is half the app's floating surfaces. The same rule `pickCommandTarget` uses,
   * for the same reason.
   */
  rendered: boolean;
  /**
   * Whether the caret is in a text-entry surface *outside* this editor.
   *
   * A terminal, the Notes textarea, the Search Everywhere box, a rename field
   * in another editor. `refactor.rename` is registered `allowInText`, which it
   * has to be — the caret is inside `.cm-content` whenever F2 is meant — so the
   * key arrives with focus anywhere, and a floating panel is an overlay that
   * leaves the editor behind it fully rendered.
   */
  focusElsewhere: boolean;
}

/**
 * Whether this editor should be the one to act on `refactor.rename`.
 *
 * Several `FileEditor`s are mounted at once, inside `display: none` wrappers, so
 * every one of them registers a handler and exactly one must act. `executeCommand`
 * walks handlers newest-first and takes the first that does not return `false`,
 * so declining is how the others get out of the way (the `SqlView` `run.run`
 * fall-through precedent).
 *
 * **Being on screen is the whole test for *which* editor, and that is
 * deliberate.** Whether this tab talks to a language server at all is *not* a
 * term here: a secrets tab that is on screen is still the editor the user is looking at, and declining on its
 * behalf would let an off-screen editor answer the key and rename a symbol in a
 * file nobody can see. So an editor with no server *acts* and then *refuses out
 * loud* — see {@link renameReadiness}.
 *
 * The one thing added to that is {@link RenameTarget.focusElsewhere}, and it is
 * not a second visibility term: it says *somebody is typing somewhere else*.
 * Unlike F5 or F7, which act on a control without moving the caret, F2 opens an
 * autofocusing field — so acting while the user is in a terminal took the caret
 * out of the terminal and offered to rename a symbol in a file behind it, at a
 * position left over from whenever that editor was last clicked.
 */
export function shouldHandleRename(target: RenameTarget): boolean {
  return target.rendered && !target.focusElsewhere;
}

/**
 * What the editor knows about its own sync state when F2 arrives.
 *
 * These are the facts `FileEditor` already tracks for the usages surface; the
 * rename needs the same ones and needs them read in a particular order.
 */
export interface RenameEditorState {
  /** False for a source that talks to no server at all — a secrets tab. */
  lspEnabled: boolean;
  /** True once `lspOpenDocument` has resolved. Nothing may be asked before. */
  opened: boolean;
  /** True while a debounced `didChange` is still armed. */
  changePending: boolean;
  /** Bumped on every `docChanged`. */
  docVersion: number;
  /** The version the server was actually told about. */
  syncedVersion: number;
  /** Why the server's copy is not current, or `null`. */
  syncError: string | null;
}

/**
 * Whether it is safe to open the rename field, and what to do first if not.
 *
 * Three answers, and the middle one is the whole point of the ordering
 * guarantee: `"flush"` means *do not ask anything yet* — cancel the debounce,
 * send the buffer, and only open the field from that promise's `.then`.
 */
export type RenameReadiness =
  | { kind: "ready" }
  | { kind: "flush" }
  | { kind: "refuse"; reason: string };

/**
 * Can this editor ask for a rename right now?
 *
 * This is the frontend half of the ordering guarantee, and it is the same bug
 * class as `didchange-range-from-wrong-text` one level worse: there a stale
 * server mirror produced a wrong *count*, here it produces wrong **ranges**,
 * which are then applied to the user's files. A server answering about a buffer
 * two edits old returns positions that are entirely plausible and land on the
 * wrong text.
 *
 * The order of the branches is load-bearing:
 *
 * 1. **No server for this source** and **not opened yet** are refusals, not
 *    waits. There is nothing to flush to and nothing that will arrive.
 * 2. **A change is owed** — the debounce is armed, the versions disagree, or a
 *    previous flush failed — is `"flush"`. A previous failure is *retried* here
 *    rather than reported: a `syncError` is set precisely when the server's copy
 *    is stale, so the honest thing is to try again and let the second failure
 *    supply the reason (`FileEditor` refuses with `syncError` from the rejection).
 *    Reporting the old error without retrying would strand the feature on a
 *    single transient failure for the life of the tab.
 * 3. Only then `"ready"`.
 */
export function renameReadiness(state: RenameEditorState): RenameReadiness {
  if (!state.lspEnabled) {
    return {
      kind: "refuse",
      reason: "This tab is not backed by a language server, so it cannot rename anything.",
    };
  }
  if (!state.opened) {
    return {
      kind: "refuse",
      reason:
        "The language server has not been told about this file yet, so a rename would be " +
        "computed against text it has never seen. Try again in a moment.",
    };
  }
  if (
    state.changePending ||
    state.docVersion !== state.syncedVersion ||
    state.syncError !== null
  ) {
    return { kind: "flush" };
  }
  return { kind: "ready" };
}

// ---------------------------------------------------------------------------
// 2. The prepare answer, and the field's prefill.
// ---------------------------------------------------------------------------

/** A position in the app's own convention: 1-based line, 0-based UTF-16 column. */
export interface RenamePoint {
  line: number;
  character: number;
}

/** Whether a rename field opens, and over what. */
export type RenameOffer =
  | {
      kind: "offer";
      /** The span the server named, or `null` — it is allowed not to name one. */
      start: RenamePoint | null;
      end: RenamePoint | null;
      /** What the server suggested prefilling with, or `null`. Usually `null`. */
      placeholder: string | null;
      /** The server's own qualification of a `ready` answer, when it made one. */
      caveat: string | null;
    }
  | { kind: "refuse"; reason: string };

/**
 * What a `prepareRename` answer licenses.
 *
 * Three answers have to stay apart, and collapsing any pair of them is the
 * failure this whole subsystem refuses:
 *
 * * `outcome` is not `ready` — **nobody could be asked**. There is no server, it
 *   is still coming up, it died, or it does not offer rename. Say which.
 * * `ready` with `renameable: false` — the server looked and says this position
 *   is not a rename site. That is a real answer and it is a **refusal**, never an
 *   empty field: a box that opens and then silently renames nothing is worse than
 *   being told no.
 * * `ready` with `renameable: true` — go ahead, *even when all four position
 *   fields are `null`*. That is the `{"defaultBehavior": true}` shape and it
 *   means "rename works here, work the span out yourself". Treating it as a
 *   refusal would break every server that answers that way.
 *
 * The prefill is **not** taken from this answer. Real Roslyn replies with a bare
 * range and no placeholder, so the buffer is the live source for it
 * ({@link identifierAt}) and `placeholder` is an occasional bonus.
 */
export function renameOffer(prepare: PrepareRenameResult): RenameOffer {
  // `unsupported` is the one non-`ready` outcome that is not a refusal, because
  // it is not an answer *about this position* — it is the capability gate saying
  // this server has no `prepareRename`. `renameProvider` and its
  // `prepareProvider` are two separate facts, kept in two fields in
  // `protocol.rs` for exactly this reason, and a server declaring the bare
  // `"renameProvider": true` renames perfectly well while offering no prepare at
  // all. Refusing here disabled F2 for such a server entirely. Nothing is lost
  // by going on: the prefill is read out of the buffer by `identifierAt` (it has
  // to be — real Roslyn sends no placeholder), and the span was only ever a
  // bonus. If the server truly cannot rename either, the rename call says so in
  // its own words, which is a better sentence than one guessed in advance.
  if (prepare.outcome === "unsupported") {
    return { kind: "offer", start: null, end: null, placeholder: null, caveat: null };
  }
  if (prepare.outcome !== "ready") {
    return { kind: "refuse", reason: refusalNote(prepare) };
  }
  if (!prepare.renameable) {
    return {
      kind: "refuse",
      reason:
        prepare.message ??
        "The language server does not offer a rename at this position. Put the caret on the " +
          "symbol's name and try again.",
    };
  }
  const start =
    prepare.startLine === null || prepare.startCharacter === null
      ? null
      : { line: prepare.startLine, character: prepare.startCharacter };
  const end =
    prepare.endLine === null || prepare.endCharacter === null
      ? null
      : { line: prepare.endLine, character: prepare.endCharacter };
  return { kind: "offer", start, end, placeholder: prepare.placeholder, caveat: prepare.message };
}

/** One identifier found in a line of text, and where it sits. */
export interface FoundIdentifier {
  text: string;
  /** 0-based UTF-16 offsets into the line, half-open. */
  start: number;
  end: number;
}

/**
 * What every character a name may be made of, for the *reading* half only.
 *
 * Unicode letters and numbers plus `_` and `$` — wide on purpose. This is used
 * to read an identifier **out of the user's own buffer**, so being narrow is not
 * caution, it is a wrong answer: `[A-Za-z0-9_]` truncates `café` to `caf` and
 * would then prefill the field with a name the token check goes on to reject.
 *
 * Note the asymmetry with {@link acceptedNewName}, which validates nothing. This
 * is a reader, not a validator; it never refuses a name the user typed.
 */
const WORD = /[\p{L}\p{N}_$]/u;

/**
 * The identifier the caret is in, or `null`.
 *
 * `character` is a 0-based UTF-16 offset into `lineText`, exactly as CodeMirror
 * and the rest of this app spell a column.
 *
 * **This is the live path for C#, not a fallback.** Roslyn's `prepareRename`
 * returns a bare range with *no* placeholder (measured against 2.140.9), so
 * without this the rename box opens empty and the user retypes a name they can
 * already see. It is also the source of the `oldName` the backend's stale-mirror
 * check is made of.
 *
 * A caret sitting immediately **after** an identifier names that identifier —
 * pressing F2 with the caret at the end of a word is how people actually do
 * this, and the Rust side's `enclosing_identifier` takes the same view of an end
 * boundary. Everything else abstains: `null` rather than a guess at a nearby
 * word, because prefilling the field with the wrong symbol's name is how a
 * rename lands on something the user did not mean.
 */
export function identifierAt(lineText: string, character: number): FoundIdentifier | null {
  // Indexed by UTF-16 code unit rather than by code point, because that is what
  // a column means everywhere in this app: reading by code point would shift
  // every column on a line containing an emoji.
  const units = lineText.length;
  if (!Number.isFinite(character)) return null;
  const at = Math.max(0, Math.min(Math.floor(character), units));

  const isWord = (index: number): boolean =>
    index >= 0 && index < units && WORD.test(lineText.charAt(index));

  let from: number;
  if (isWord(at)) from = at;
  else if (isWord(at - 1)) from = at - 1;
  else return null;

  let start = from;
  while (isWord(start - 1)) start -= 1;
  let end = from + 1;
  while (isWord(end)) end += 1;
  return { text: lineText.slice(start, end), start, end };
}

/**
 * A CodeMirror document offset from a line and a UTF-16 column, clamped.
 *
 * Clamped like `searchLogic.lineToPos`, and for the same reason: the column
 * comes from a language server describing a document it may believe is a
 * different length, and CodeMirror throws on an out-of-range offset — a throw
 * that would land inside the editor and read as the app breaking. The **line**
 * is clamped by `lineToPos` before this is reached; this clamps only within it.
 *
 * `lineFrom` is the offset of the line's first character and `lineLength` its
 * length in UTF-16 units, both read off `doc.line(..)` by the caller. Nothing
 * about a document is needed beyond those two numbers, which is what makes this
 * testable without CodeMirror.
 */
export function posAt(lineFrom: number, lineLength: number, character: number): number {
  const column = Number.isFinite(character) ? Math.max(0, Math.floor(character)) : 0;
  return lineFrom + Math.min(column, Math.max(0, Math.floor(lineLength)));
}

// ---------------------------------------------------------------------------
// 3. The name the user typed.
// ---------------------------------------------------------------------------

/** The new name, or why it was not sent. */
export type AcceptedName = { kind: "ok"; name: string } | { kind: "reject"; reason: string };

/**
 * Whether to send what the user typed, **abstaining on every language rule**.
 *
 * Only four rejections, and each one is wrong in *every* language this app
 * opens:
 *
 * * nothing typed at all;
 * * whitespace inside the name, or a newline anywhere in it — no language spells
 *   one identifier with a space, and a pasted multi-line blob is a paste
 *   accident rather than a name;
 * * the old name again, which the server would answer with a perfectly
 *   successful edit set that changes nothing, reported as a rename that worked.
 *
 * Everything else goes to the server, and that is the deliberate part. A
 * per-language identifier validator here would be a second, worse opinion than
 * the language server's own — it would refuse `@class` and `@event` in C#,
 * `café` and `Ω` in anything with Unicode identifiers, `$scope` and `#private`
 * in JavaScript, and `r#type` in Rust. The server knows its own grammar and
 * refuses with its own words; this does not pretend to.
 *
 * Surrounding whitespace is **trimmed rather than rejected** — a name pasted
 * with a trailing space is unambiguous about what was meant, and trimming is
 * not a language rule. The trimmed value is what is returned, so the caller
 * sends this and never the raw text.
 */
export function acceptedNewName(raw: string, oldName: string): AcceptedName {
  const name = raw.trim();
  if (name === "") {
    return { kind: "reject", reason: "Type a new name." };
  }
  if (/\s/u.test(name)) {
    return {
      kind: "reject",
      reason: "A name cannot contain spaces or line breaks.",
    };
  }
  if (name === oldName) {
    return {
      kind: "reject",
      reason: "That is already the name. Type a different one, or press Escape to cancel.",
    };
  }
  return { kind: "ok", name };
}

// ---------------------------------------------------------------------------
// 4. Reasons, caveats, and the stale-mirror check over an open buffer.
// ---------------------------------------------------------------------------

/**
 * One sentence for a rename that has no answer.
 *
 * The words come from `usagesLogic.availabilityPhrase` rather than from a second
 * set written here: the six states mean the same thing to a rename as to a
 * usages count, and two phrasings of "the server is still loading" eventually
 * disagree about whether that means there was nothing to rename. The server's
 * own `message` is appended when it sent one, because it is always better than
 * anything phrased in advance.
 *
 * `ready` is reachable and is not treated as a bug: `availabilityPhrase` answers
 * "Usages unknown" for it, which is why this prefixes the sentence with what was
 * attempted rather than rendering the phrase bare.
 */
export function refusalNote(result: { outcome: Availability; message: string | null }): string {
  const phrase = availabilityPhrase(result.outcome).text;
  return result.message === null
    ? `${phrase}, so nothing was renamed.`
    : `${phrase}, so nothing was renamed. ${result.message}`;
}

/**
 * The qualification a `ready` answer carries, or `null`.
 *
 * A `ready` result with a `message` came from a server the readiness ceiling
 * promoted — it never finished priming the workspace — and for a rename that
 * does not mean a low count, it means **call sites may have been missed**. A file
 * left holding the old name does not compile, so this is the one case that gets
 * a confirmation step; the user explicitly chose no preview for the normal path
 * and this is not the normal path.
 *
 * **It takes anything carrying an outcome and a message, and that is the point.**
 * The caveat is known at two different times: `prepareRename` answers it *before*
 * Enter is honoured, which is when a confirmation is worth anything, and a
 * `rename` can only answer it *after* the writes have happened, when the same
 * sentence has to be reported instead. One function so the two cannot describe
 * the same condition two ways.
 */
export function provisionalRenameWarning(result: {
  outcome: Availability;
  message: string | null;
}): string | null {
  if (result.outcome !== "ready" || result.message === null) return null;
  return (
    `${result.message} Call sites may have been missed, which would leave the old name ` +
    "behind in files this rename did not touch."
  );
}

/**
 * Whether one edit lands on a token of the name being renamed.
 *
 * The stale-mirror check for an **open buffer** — the backend runs the same rule
 * over the closed files it reads, and cannot run it here because it has no text
 * for a buffer the editor has not saved.
 *
 * **It is a check on the enclosing token, not on the replaced text**, and that
 * correction is measured rather than theoretical: Roslyn's rename edits are
 * zero-width *insertions* carrying a minimal diff — renaming `Walker` to
 * `HeapWalker` inserts `"Heap"` at column 29 — so the text a Roslyn rename
 * replaces is the empty string every single time. A "the replaced text contains
 * the old identifier" rule would refuse every correct Roslyn rename. Expanding
 * outward from the edit's start position to the enclosing identifier answers
 * `Walker` for both an insertion inside the token and a whole-token replacement,
 * so one rule covers both server styles and still catches the case it exists
 * for: an edit aimed at text that is not the symbol at all.
 *
 * An empty `oldName` **abstains** — comparing against a name no token equals
 * would refuse every rename, and the backend documents the same abstention.
 */
export function replacedTextMatches(
  lineText: string,
  startCharacter: number,
  oldName: string,
): boolean {
  if (oldName === "") return true;
  const found = identifierAt(lineText, startCharacter);
  return found !== null && found.text === oldName;
}

/** What to do with one open buffer's edits. */
export type BufferVerdict =
  | { kind: "apply"; note: string | null }
  | { kind: "refuse"; reason: string };

/**
 * Whether every edit names a line this buffer actually has.
 *
 * **The stale-mirror check the frontend alone can make, and the one place this
 * side must not clamp.** `edits::apply` refuses exactly this condition for a
 * closed file — `EditError::OutOfDocument` — and the Rust module's own note
 * calls it "the stale-mirror bug in its most damaging form". An open buffer had
 * no equivalent: `lineToPos` clamps a line past the end of the document to the
 * last line and `posAt` clamps the column into it, so an edit computed against a
 * mirror in which this file was 150 lines longer was dropped into an arbitrary
 * column of the last line — into the one file in the workspace holding unsaved
 * work.
 *
 * {@link bufferVerdict} is not a substitute and cannot be made into one: it
 * refuses only when *no* edit matches, because a single non-matching edit is
 * routinely legitimate. A clamped edit is indistinguishable from a
 * rename-in-comment there, so it was applied with a note confidently describing
 * something that did not happen.
 *
 * Refusal is per **file**, like every other refusal in this feature: a partial
 * application is a file that does not compile. `docLines` is CodeMirror's
 * `doc.lines`, and a {@link RangeEdit} line is 1-based, so the range is
 * `1..=docLines`.
 */
export function documentBoundsVerdict(
  docLines: number,
  edits: readonly { startLine: number; endLine: number }[],
  path: string,
): BufferVerdict {
  const within = (line: number): boolean =>
    Number.isFinite(line) && Math.floor(line) >= 1 && Math.floor(line) <= docLines;
  const outside = edits.find((edit) => !within(edit.startLine) || !within(edit.endLine));
  if (outside === undefined) return { kind: "apply", note: null };
  const named = within(outside.startLine) ? outside.endLine : outside.startLine;
  return {
    kind: "refuse",
    reason:
      `${path} was left unchanged: the language server asked to change line ${named} of a ` +
      `buffer that has ${docLines}, so it computed these edits from a different version of ` +
      "this file. Save it, or close and reopen it, and rename again.",
  };
}

/**
 * Whether to apply an open buffer's edits, given which of them land on the old
 * name.
 *
 * **Refuse only when *no* edit in the file matches.** A non-matching edit can be
 * perfectly legitimate: Roslyn renames inside comments and strings when asked,
 * and TypeScript expands a shorthand property (`{ foo }` becomes
 * `{ newFoo: foo }`) — both touch text that is not the bare identifier. A file
 * where some edits match is a plausible answer and the disagreement belongs in a
 * note, not in a refusal. A file where *none* match is a server describing a
 * different version of this text, and applying that would corrupt it.
 *
 * An empty list is `apply` with nothing to say: the server named the file and
 * asked for no changes, which is legal and is not a refusal.
 */
export function bufferVerdict(matches: readonly boolean[], path: string): BufferVerdict {
  if (matches.length === 0) return { kind: "apply", note: null };
  const matched = matches.filter(Boolean).length;
  if (matched === 0) {
    return {
      kind: "refuse",
      reason:
        `${path} was left unchanged: none of the ${matches.length} edits the language server ` +
        "asked for land on the name being renamed, so they describe different text than this " +
        "buffer holds.",
    };
  }
  if (matched === matches.length) return { kind: "apply", note: null };
  const other = matches.length - matched;
  return {
    kind: "apply",
    note:
      `${other} of the ${matches.length} edits in ${path} do not land on the name itself — ` +
      "a comment, a string, or a shorthand property being expanded.",
  };
}

// ---------------------------------------------------------------------------
// 5. Reporting what happened.
// ---------------------------------------------------------------------------

/** A notification, in the shape `notificationLogic` and `App` already use. */
export interface RenameReport {
  kind: "error" | "warning" | "info";
  title: string;
  detail: string;
}

/**
 * The one sentence for a file left holding part of a rename.
 *
 * One function because it is needed on two paths — the non-`ready` outcome the
 * backend actually produces, and the `ready` one the type still permits — and
 * two spellings of this particular escalation would eventually disagree about
 * how serious it is.
 */
function strandedLine(failure: { path: string; detail: string }): string {
  return (
    `${failure.path} holds part of this rename and could not be restored — nothing here has ` +
    `its previous contents. Review it in git now. (${failure.detail})`
  );
}

/** Plural without the "1 files" tell. */
function count(n: number, one: string, many: string): string {
  return `${n} ${n === 1 ? one : many}`;
}

/**
 * What to tell the user after a rename, in one sentence plus the caveats.
 *
 * `receivedPaths` are the buffers an editor **actually applied**, which is not
 * the same list as `result.buffers`: an entry with no receiving editor is the one
 * window this design cannot close — the gap between the backend's writes and the
 * frontend's dispatch, one promise resolution wide — and it is **reported**
 * rather than dropped. Dropping it would leave a file holding the old name with
 * nothing anywhere saying so.
 *
 * Three things this refuses to soften:
 *
 * * **files written to disk are not undoable here.** Ctrl+Z in a tab reverts that
 *   tab's part and nothing more; the closed files were written straight out. So
 *   the notification says so and points at the Changes tab, which is one click
 *   from the line-level revert this app is genuinely good at.
 * * a `RenameFailure` with `unrecoverable` set is an **escalation**, not a row in
 *   a list: that file holds part of a rename and nothing anywhere has its
 *   previous contents.
 * * a `ready` answer carrying a message means call sites may have been **missed**.
 */
export function renameSummary(
  oldName: string,
  newName: string,
  result: RenameResult,
  receivedPaths: readonly string[],
): RenameReport {
  if (result.outcome !== "ready") {
    // `failures` is populated on **this** path and no other: `write_plan`'s
    // failure return is the only place that fills it, and it always carries
    // `outcome: Failed`. So returning the refusal sentence alone discarded every
    // path in it — and in the unrecoverable case the backend's message carries a
    // count and no path at all, which made `failures[i].path` the only place the
    // answer existed. An unrecoverable failure is also the one state where "was
    // not renamed" is a false title: that file holds part of a rename.
    const stranded = result.failures.filter((failure) => failure.unrecoverable);
    if (stranded.length === 0) {
      return { kind: "error", title: `${oldName} was not renamed`, detail: refusalNote(result) };
    }
    const parts = [refusalNote({ outcome: result.outcome, message: result.message })];
    for (const failure of stranded) parts.push(strandedLine(failure));
    return {
      kind: "error",
      title: `${oldName} was partly renamed and could not be undone`,
      detail: parts.join(" "),
    };
  }

  const received = new Set(receivedPaths);
  const orphaned = result.buffers.filter((buffer) => !received.has(buffer.path));
  const unrecoverable = result.failures.filter((failure) => failure.unrecoverable);
  const recoverable = result.failures.filter((failure) => !failure.unrecoverable);

  const files = result.written.length + receivedPaths.length;
  const parts: string[] = [];

  if (result.total === 0 && files === 0) {
    return {
      kind: "info",
      title: "Nothing to rename",
      detail:
        `The language server found no use of ${oldName} to change. Nothing was written and ` +
        "no buffer was edited.",
    };
  }

  parts.push(
    `${count(result.total ?? 0, "edit", "edits")} in ${count(files, "file", "files")}.`,
  );
  if (result.written.length > 0) {
    parts.push(
      `${count(result.written.length, "file was", "files were")} written straight to disk and ` +
        "cannot be undone here — Ctrl+Z only reverts the tab you are in. Use the Changes tab " +
        "to review or revert them.",
    );
  }
  for (const buffer of orphaned) {
    parts.push(
      `${buffer.path} has ${count(buffer.edits.length, "edit", "edits")} that no open editor ` +
        "received, so it still holds the old name. Open it and rename again.",
    );
  }
  for (const failure of recoverable) {
    parts.push(`${failure.path} was not changed: ${failure.detail}`);
  }
  for (const failure of unrecoverable) parts.push(strandedLine(failure));
  const caveat = provisionalRenameWarning(result);
  if (caveat !== null) parts.push(caveat);

  const kind: RenameReport["kind"] =
    unrecoverable.length > 0 || orphaned.length > 0 || recoverable.length > 0
      ? "error"
      : caveat !== null
        ? "warning"
        : "info";

  return { kind, title: `Renamed ${oldName} to ${newName}`, detail: parts.join(" ") };
}

/**
 * The paths in a result that an editor is expected to apply.
 *
 * Exists so the component and {@link renameSummary} cannot disagree about what
 * "was received" means — one is a list of what was asked for and the other of
 * what happened, and they are compared.
 */
export function bufferPaths(buffers: readonly BufferEdits[]): string[] {
  return buffers.map((buffer) => buffer.path);
}

/**
 * Forget one path's pending buffer edits, once an editor has taken them.
 *
 * The map `RunView` hands out was written once and never cleared, while
 * `FileEditor`'s "have I applied this token?" guard is a **per-mount** ref
 * starting at zero. So closing a renamed tab and reopening it later re-consumed
 * a finished rename: the fresh mount ran before its CodeMirror view existed and
 * raised an error — one that never auto-dismisses — saying the file "still holds
 * the old name", when it holds the new one and was saved. Winning that race
 * would have been no better: the buffer no longer contains the old name, so
 * every edit fails the token check and the refusal reads just as wrongly.
 *
 * The same reference comes back when there is nothing to forget, so a second
 * consume costs no render.
 */
export function consumeRenameEdits<T>(
  state: { token: number; byPath: Record<string, T> },
  path: string,
): { token: number; byPath: Record<string, T> } {
  if (!(path in state.byPath)) return state;
  const byPath: Record<string, T> = {};
  for (const [key, value] of Object.entries(state.byPath)) {
    if (key !== path) byPath[key] = value;
  }
  return { token: state.token, byPath };
}

// ---------------------------------------------------------------------------
// 6. Where the field goes.
// ---------------------------------------------------------------------------

/** How wide the rename field is, for clamping it into the editor pane. */
export const RENAME_FIELD_WIDTH = 240;

/**
 * Where to put the rename field, in coordinates relative to the editor frame.
 *
 * Beside `usagesLogic.placeMenu` and clamped the same way rather than flipped,
 * for the same reason: the editor pane can be a couple of hundred pixels tall
 * and there is often no side with room. Every clamp has a floor, because a pane
 * narrower than the field makes the natural arithmetic negative and would place
 * the field off the left edge.
 *
 * The field is placed **above** nothing and offset slightly below the caret's
 * line, so it does not sit on top of the identifier the user is renaming — they
 * are reading it while they type.
 */
export function placeRenameField(
  x: number,
  y: number,
  bounds: MenuBounds | null,
  width: number = RENAME_FIELD_WIDTH,
): Placement {
  if (!bounds) return { left: 4, top: 4, maxHeight: 0 };
  const left = Math.max(4, Math.min(x - bounds.left, bounds.width - width - 4));
  const top = Math.max(4, Math.min(y - bounds.top + 4, Math.max(4, bounds.height - 36)));
  // `maxHeight` is part of the shared `Placement` shape and means nothing for a
  // single-line input; zero rather than a number a caller might apply as a style.
  return { left, top, maxHeight: 0 };
}

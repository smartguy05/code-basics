/**
 * The search palette's decisions, with no React and no DOM in sight.
 *
 * Everything here is a function of its arguments: which shortcut a keydown is,
 * where the selection moves to, how a label is cut into highlighted runs, which
 * line the editor may safely be asked to reveal. The palette component around
 * it stays a rendering shell, which is the only way any of this gets tested —
 * the vitest suite runs in a node environment with no jsdom, so nothing in this
 * file may touch a `KeyboardEvent`, an element or a CodeMirror view.
 *
 * The ranking is *not* here, and neither is the `Foo:123` line suffix. Both are
 * `cb-core`'s (`crates/core/src/symbols/search.rs`), which parses the suffix off
 * before it searches and returns the line on the hit. A second implementation of
 * either would be a second opinion, and two opinions about where to jump
 * eventually disagree.
 */

/**
 * `SearchScope` and `HitKind` are **not** declared here.
 *
 * They cross the IPC boundary, so `ipc/types.ts` is their single home and this
 * file imports them. They used to be declared in both places: two structurally
 * identical string unions, which `tsc` cannot tell apart and never will, so a
 * variant added on one side would type-check against the other while meaning
 * something different at run time. Re-exported so the components that already
 * import them from here keep working, and so there is still exactly one
 * declaration to change.
 *
 * This is a **type-only** import and is erased at compile time, so it pulls no
 * runtime module in and the vitest suite still loads this file without the IPC
 * layer or `@tauri-apps/api` behind it. Everything below stays structurally
 * typed (`groupHits` and friends are generic over `{ kind }`) for the same
 * reason — the values never come from an import.
 */
import type { HitKind, SearchScope } from "../ipc/types";

export type { HitKind, SearchScope };

/**
 * The parts of a keydown a shortcut is allowed to depend on.
 *
 * Deliberately not the DOM `KeyboardEvent`: a structural type of five fields is
 * what makes the whole keybinding table testable without a browser, and a real
 * event satisfies it as-is at the call site.
 */
export interface ShortcutEvent {
  key: string;
  ctrlKey: boolean;
  shiftKey: boolean;
  altKey: boolean;
  metaKey: boolean;
}

/**
 * How long after one Shift a second one still counts as a double-Shift.
 *
 * Long enough to be reachable deliberately, short enough that two unrelated
 * Shifts while typing capitals do not open the palette in someone's face.
 */
export const SHIFT_WINDOW_MS = 300;

/**
 * Which palette a keydown asks for, or null for every other key in the app.
 *
 * The whole keybinding table, in one place, as one expression of it:
 *
 * | keys | scope |
 * |---|---|
 * | Shift Shift (within {@link SHIFT_WINDOW_MS}) | `all` |
 * | Ctrl+N | `symbols` |
 * | Ctrl+Shift+N | `files` |
 * | Ctrl+Shift+A | `actions` |
 *
 * **Ctrl+F is deliberately not taken.** It already belongs to the output
 * console's find bar (`OutputConsole.tsx`), and a global palette binding would
 * shadow it — taking a working feature away from the user to give them a second
 * route to one they can already reach three other ways. The same reasoning
 * keeps Ctrl+A free: it is select-all in whatever has focus.
 *
 * Shift is checked before the letter, so Ctrl+Shift+N cannot fall through to
 * Ctrl+N. Written the other way round the *files* binding is unreachable, and
 * unreachable in a way that looks like it works — it opens a palette, just the
 * wrong one.
 *
 * `lastShiftAt` is when the previous bare Shift keydown happened, or null. The
 * "nothing between" half of double-Shift is the caller's: it clears the
 * timestamp whenever any other key is pressed, because this function is only
 * shown one event and cannot know what came before it. `now` is a parameter so
 * the window is testable without faking the clock.
 *
 * A Shift held together with Ctrl, Alt or Meta is never a bare Shift press —
 * that is the user reaching for some other chord and being fast about it, and
 * opening the palette over the top of it would be this file guessing.
 */
export function recogniseShortcut(
  event: ShortcutEvent,
  lastShiftAt: number | null,
  now: number = Date.now(),
): SearchScope | null {
  if (event.key === "Shift") {
    if (event.ctrlKey || event.altKey || event.metaKey) return null;
    if (lastShiftAt === null) return null;
    const since = now - lastShiftAt;
    // A negative gap means the clock moved backwards under us; that is not a
    // double-Shift, and treating it as one would fire the palette at random.
    return since >= 0 && since <= SHIFT_WINDOW_MS ? "all" : null;
  }

  // Meta is not accepted as a stand-in for Ctrl. The table above is the one
  // this app documents and the one it is tested against; inventing a second
  // modifier for a platform nothing here has been run on would be a claim
  // rather than a feature.
  if (!event.ctrlKey || event.altKey || event.metaKey) return null;

  // With Shift held the browser reports the shifted character, so `N` and `n`
  // are the same binding.
  const letter = event.key.toLowerCase();
  if (event.shiftKey) {
    if (letter === "n") return "files";
    if (letter === "a") return "actions";
    return null;
  }
  return letter === "n" ? "symbols" : null;
}

/** One heading's worth of results. */
export interface Section<T> {
  kind: HitKind;
  title: string;
  hits: T[];
}

/** The headings, in the order they are drawn. */
const SECTIONS: { kind: HitKind; title: string }[] = [
  { kind: "file", title: "Files" },
  { kind: "symbol", title: "Symbols" },
  { kind: "action", title: "Actions" },
];

/**
 * Group a ranked result list under its headings.
 *
 * The section order is fixed — Files, Symbols, Actions — so that the headings
 * do not jump around as the query changes, and a section with nothing in it is
 * left out entirely rather than drawn empty: a heading with no rows under it
 * reads as "there are none of these", which is true, but it costs a line of a
 * list whose whole job is to fit on the screen.
 *
 * Within a section the incoming order is preserved exactly. The backend ranked
 * these against each other with a total order it went to some trouble to make
 * stable (`Ranked::cmp` in `search.rs`); re-sorting them here by name or by
 * anything else would throw that ranking away and put the best match somewhere
 * other than the top.
 *
 * Generic over the hit so this file needs no import from `ipc/types` — a
 * `SearchHit` satisfies `{ kind }` and comes back out with its own type intact.
 */
export function groupHits<T extends { kind: HitKind }>(hits: T[]): Section<T>[] {
  return SECTIONS.map(({ kind, title }) => ({
    kind,
    title,
    hits: hits.filter((hit) => hit.kind === kind),
  })).filter((section) => section.hits.length > 0);
}

/**
 * Where the arrow keys move the selection, wrapping at both ends.
 *
 * Wrapping rather than stopping because the list is short and the fastest way to
 * the last row is Up from the first. The modulo is written twice on purpose:
 * JavaScript's `%` keeps the sign of the left operand, so `-1 % 3` is `-1`, and
 * a negative selection index would render nothing while looking like a hang.
 *
 * An empty list answers 0. It has no valid index at all, and 0 is the one value
 * that stays harmless when the list refills a keystroke later — `-1` and `NaN`
 * both survive into the render and index into nothing. This also normalises a
 * selection left over from a longer list, which is the ordinary case: the query
 * changes, the results shrink, and the old index is past the end.
 */
export function nextIndex(current: number, delta: number, total: number): number {
  if (total <= 0) return 0;
  return (((current + delta) % total) + total) % total;
}

/**
 * Which configuration ids an action hit may name and still be worth offering.
 *
 * The palette ranks over *every* `RunConfig` the workspace has, but the only
 * consumer of an action hit is the Run tab, and that tab's list is application
 * configurations only (`RunView`'s `appConfigs`). A test configuration
 * therefore ranks, renders, and then does nothing at all when it is chosen:
 * the palette closes, the tab switches, and no selection changes — a wrong
 * answer wearing the costume of a working one, which is exactly what this
 * codebase refuses.
 *
 * So actionability is decided here, from the same field the Run tab filters on,
 * and {@link dropUnactionable} applies it before anything is drawn. The narrow
 * alternative — routing a test configuration to the Tests tab instead — is the
 * more useful feature and is not this function's to build: it needs a second
 * pending-request slot in `App.tsx` and a consumer in `TestsView`, neither of
 * which the palette can reach from here.
 *
 * Typed structurally over `{ id, kind }` so this file still imports nothing
 * from `ipc/types`; a `RunConfig` satisfies it as it stands.
 */
export function actionableIds(
  configs: readonly { id: string; kind: string }[],
): Set<string> {
  const ids = new Set<string>();
  for (const config of configs) {
    if (config.kind === "app") ids.add(config.id);
  }
  return ids;
}

/**
 * Remove the action hits nothing downstream would act on, leaving the rest.
 *
 * File and symbol hits are never touched: they are opened by a different route
 * that serves any path the index holds. Only action rows are filtered, and only
 * against the set {@link actionableIds} computed — a hit with no `actionId` at
 * all goes too, since choosing it is already a no-op in the component.
 *
 * `actionable` is null while the workspace has not been read yet, and that case
 * drops every action row rather than showing them unchecked. It is the abstain
 * side of the governing rule: a row that is missing for the few milliseconds
 * before the workspace arrives costs the user a repeated keystroke, whereas a
 * row that is shown and then does nothing costs them their belief that Enter
 * works. The order the backend ranked the survivors in is preserved exactly.
 */
export function dropUnactionable<
  T extends { kind: HitKind; actionId?: string | null },
>(hits: readonly T[], actionable: ReadonlySet<string> | null): T[] {
  return hits.filter((hit) => {
    if (hit.kind !== "action") return true;
    if (actionable === null) return false;
    return hit.actionId != null && actionable.has(hit.actionId);
  });
}

/**
 * The identity of one search: the scope and the query text together.
 *
 * The palette holds one list of hits and reopens under whatever scope the
 * shortcut asked for, so "are these rows the answer to what is on screen?" is a
 * question it has to be able to ask. Comparing the query alone is not enough —
 * Esc out of an Actions search for "api" and back in with Ctrl+Shift+N and the
 * query is identical while the population is not.
 *
 * The parts are joined with a NUL, which no scope contains and no user types,
 * so no query text can be arranged to forge another scope's key.
 */
export function searchKey(scope: SearchScope, query: string): string {
  return `${scope}\u0000${query}`;
}

/** What the results area of the palette is currently able to say. */
export type ResultsState = "prompt" | "pending" | "empty" | "hits";

/**
 * Which of the four things the results area may say is true right now.
 *
 * The distinction that matters is `pending` versus `empty`. The hits in hand
 * were answered for some particular scope and query; until a reply for the
 * search *being displayed* arrives they are somebody else's answer, and both
 * ways of using them are wrong — drawing them shows rows the current scope
 * excludes, and drawing "No matches." instead announces a result no search has
 * produced. `pending` says neither, and the component draws no rows in it, so
 * Enter in that window cannot act on a row from the previous query either.
 *
 * `resultKey` is the key the hits in hand were fetched under, or null when
 * nothing has been fetched; `currentKey` is {@link searchKey} of what the user
 * is looking at. An empty or whitespace query is `prompt` before anything else
 * is considered — the component never searches for one, so any hits alongside
 * it are certainly stale.
 */
export function resultsState(
  query: string,
  resultKey: string | null,
  currentKey: string,
  hitCount: number,
): ResultsState {
  if (query.trim() === "") return "prompt";
  if (resultKey === null || resultKey !== currentKey) return "pending";
  return hitCount === 0 ? "empty" : "hits";
}

/** A run of a label, either matched by the query or not. */
export interface Span {
  text: string;
  hit: boolean;
}

/**
 * Cut a label into alternating matched and unmatched runs for rendering.
 *
 * `positions` are **character** indices, as `SearchHit::positions` documents —
 * they come from Rust's `char_indices`-based scorer, counting characters. So the
 * label is decomposed with `Array.from`, which iterates code points, and never
 * indexed or sliced as a raw string: a single emoji is two UTF-16 units, and
 * slicing by them would shift every span after the first non-BMP character and
 * can cut a surrogate pair in half, which renders as a replacement glyph. An
 * accented name is enough to make it visibly wrong; the tests assert the spans
 * rejoin into exactly the original label.
 *
 * Adjacent positions merge into one span. Emitting one element per matched
 * character would fill the DOM with single-character nodes on every keystroke,
 * and a fuzzy match is mostly consecutive runs.
 *
 * Positions outside the label, duplicated, out of order or not whole numbers are
 * dropped rather than thrown on. The label and the positions were computed by
 * one backend call and should always agree, but if a bug ever makes them
 * disagree the honest failure is a row rendered without highlighting — not a
 * palette that throws while the user is typing.
 */
export function highlightSpans(label: string, positions: number[]): Span[] {
  const chars = Array.from(label);
  if (chars.length === 0) return [];

  const matched = new Set<number>();
  for (const position of positions) {
    if (Number.isInteger(position) && position >= 0 && position < chars.length) {
      matched.add(position);
    }
  }
  if (matched.size === 0) return [{ text: label, hit: false }];

  const spans: Span[] = [];
  let start = 0;
  let hit = matched.has(0);
  for (let i = 1; i <= chars.length; i += 1) {
    const next = i < chars.length && matched.has(i);
    if (i === chars.length || next !== hit) {
      spans.push({ text: chars.slice(start, i).join(""), hit });
      start = i;
      hit = next;
    }
  }
  return spans;
}

/**
 * Clamp a hit's line onto a line the document actually has.
 *
 * A `SearchHit` names a line the index recorded, and the index is a snapshot: by
 * the time the row is clicked the file may have been edited, or the user may
 * have typed a `:5000` suffix of their own, which the backend passes through
 * because it has no way to know how long the file is. CodeMirror throws on
 * `doc.line()` out of range, and that throw lands inside the editor while the
 * palette is closing — an unrecoverable-looking crash for the trivial mistake of
 * a stale line number.
 *
 * So the answer is always a line that exists: the nearest end of the document,
 * which puts the user in the right file at the top or the bottom instead of
 * nowhere at all. A fractional line floors to the line it falls inside, and a
 * `NaN` line goes to the first line — the file is still the answer even when
 * the position is not.
 *
 * `NaN` is the only non-position this guards, and the wording above used to
 * claim more than that: "a line that is not a number at all". Executed, that is
 * false. The guard is `Number.isNaN(line)`, and `Number.isNaN(undefined)` is
 * `false`, so `undefined` falls through to `Math.floor(undefined)` — `NaN` —
 * and every comparison in the clamp is then false, so the function *returns*
 * `NaN`. (`"abc"` does the same; `null` and `"7"` do not, because `Math.floor`
 * coerces them to `0` and `7`.) The parameter is typed `number` and the sole
 * caller, `FileEditor`, has already narrowed `revealLine?: number | null`
 * against `null` before it reaches here, so nothing can present `undefined`
 * without defeating the type checker first. That is where the guarantee comes
 * from — not from this function, which is why the doc no longer says otherwise.
 */
export function lineToPos(totalLines: number, line: number): number {
  const last = Math.max(1, Math.floor(totalLines) || 1);
  if (Number.isNaN(line)) return 1;
  return Math.min(Math.max(Math.floor(line), 1), last);
}

/**
 * The sentence shown above the results, or `null` when there is nothing
 * honest to say.
 *
 * This is here rather than in the component because it is a claim about what
 * the backend can currently answer, and it was wrong once already. The note
 * used to read "Indexing the workspace — no results yet." for the whole first
 * build, which is the state a workspace spends its first 637 ms in warm and
 * its first nine seconds in cold. In that same window `search_everywhere`
 * substitutes an empty index and ranks run configurations normally
 * (`src-tauri/src/commands/symbols.rs`), so the palette was rendering action
 * rows underneath a banner saying there were none — the tool contradicting
 * itself on screen, which is worse than saying nothing.
 *
 * What each state may truthfully claim:
 *
 * * **building, nothing indexed yet** — configurations match; files and
 *   symbols do not yet. Name both halves, because a user who searched for a
 *   file and found only configurations needs to know the difference between
 *   "not there" and "not yet".
 * * **building over a usable index** (a rescan) — everything matches, but the
 *   answer may be behind the disk.
 * * **not building, never built** — the build failed or was cleared;
 *   configurations are all that can match, and that is now the *stable* state
 *   rather than a passing one, so it is worth a different sentence.
 * * **capped** — the counts, so the user can tell a missing symbol from an
 *   elided one. Never a bare "some results were dropped".
 * * **ready and complete** — nothing. A banner with no news trains people to
 *   stop reading banners.
 *
 * The parameter is structural rather than an import of `SymbolIndexStatus`,
 * for the same reason the hit helpers above are: this module deliberately
 * depends on nothing, so the vitest suite can load it without pulling in the
 * IPC layer. The test passes a real `SymbolIndexStatus`, which is what proves
 * the two shapes still agree.
 */
export interface IndexNoteStatus {
  ready: boolean;
  building: boolean;
  files: number;
  symbols: number;
  truncated: boolean;
}

export function indexNote(status: IndexNoteStatus | null): string | null {
  if (status === null) return null;

  if (status.building) {
    return status.ready
      ? "Indexing the workspace — results may be incomplete until it finishes."
      : "Indexing the workspace — run configurations match already; files and symbols do not yet.";
  }

  if (!status.ready) {
    return "The symbol index has not been built yet, so only run configurations can match.";
  }

  if (status.truncated) {
    return `The index hit its cap at ${status.files} files and ${status.symbols} symbols, so part of the workspace is not searched.`;
  }

  return null;
}

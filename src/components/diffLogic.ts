import type { FileDiff, Hunk } from "../ipc/types";

/**
 * Line endings normalised to `\n`.
 *
 * The diff view compares a baseline read from git (a blob, always `\n`) against
 * the working file read from disk (`\r\n` on Windows right after an
 * `autocrlf=true` checkout). `@codemirror/merge` diffs the two raw strings, so a
 * bare ending mismatch marks **every** line changed and paints the whole pane
 * green — a change git itself filters out. Both sides are brought to `\n` before
 * the comparison so only real content differences show. Lone `\r` (old Mac) is
 * folded too, so the normalisation is total rather than CRLF-only.
 */
export function normaliseEndings(source: string): string {
  return source.replace(/\r\n?/g, "\n");
}

/**
 * The working document with only `hunks` reverted to their baseline.
 *
 * Feeding this as the diff's left side (against the unchanged working copy on
 * the right) makes every region *outside* those hunks identical on both sides,
 * so the merge view highlights only the hunks named — the rest is unchanged and
 * folds away. This is what scopes an intent card's diff to the card's own
 * change instead of every change in the file, while keeping real line numbers.
 *
 * `working` must already be `\n`-normalised, since the hunk line content came
 * from git as `\n`; mixing the two would splice mismatched endings back in.
 * Hunks are applied bottom-up so an earlier splice cannot shift a later hunk's
 * position. A hunk whose position falls outside the document is skipped rather
 * than throwing — the diff can lag the file by a write.
 */
export function focusedBaseline(working: string, hunks: Hunk[]): string {
  const lines = working.split("\n");

  for (const hunk of [...hunks].sort((a, b) => b.newStart - a.newStart)) {
    const start = hunk.newStart - 1;
    if (start < 0 || start > lines.length) continue;

    // The baseline side of the hunk: everything the working copy did not add,
    // in order, which is exactly what those working lines replaced.
    const baselineLines = hunk.lines
      .filter((line) => line.origin !== "addition")
      .map((line) => line.content);

    lines.splice(start, hunk.newLines, ...baselineLines);
  }

  return lines.join("\n");
}

/** Every changed line index in a diff, for "select all". */
export function allChangedIndices(diff: FileDiff): number[] {
  return diff.hunks
    .flatMap((hunk) => hunk.lines)
    .filter((line) => line.origin !== "context")
    .map((line) => line.index);
}

/**
 * The diff reduced to the named hunks — an intent group's share of one file.
 *
 * Order follows the diff itself, and indices the diff does not have are
 * ignored: the group was computed from an earlier snapshot, and a stale index
 * must not throw the whole view away.
 */
export function onlyHunks(diff: FileDiff, hunks: number[]): FileDiff {
  const wanted = new Set(hunks);
  return { ...diff, hunks: diff.hunks.filter((_, index) => wanted.has(index)) };
}

/** Changed line indices belonging to one hunk. */
export function hunkIndices(diff: FileDiff, hunk: number): number[] {
  return (diff.hunks[hunk]?.lines ?? [])
    .filter((line) => line.origin !== "context")
    .map((line) => line.index);
}

/** What a hunk did, for colouring its mark on the strip. */
export type ChangeKind = "addition" | "deletion" | "modification";

/**
 * One change's position on the marker strip, as fractions of the document.
 *
 * Fractions rather than pixels so the strip needs no layout knowledge and the
 * arithmetic stays testable. A very small change can round to a hairline; the
 * strip gives its marks a CSS `min-height` rather than inflating the fraction,
 * because a mark that claims more of the file than the change occupies would
 * mislead about where the change actually is.
 */
export interface ChangeMark {
  top: number;
  height: number;
  kind: ChangeKind;
}

function kindOf(hunk: Hunk): ChangeKind | null {
  let added = false;
  let removed = false;

  for (const line of hunk.lines) {
    if (line.origin === "addition") added = true;
    else if (line.origin === "deletion") removed = true;
  }

  if (added && removed) return "modification";
  if (added) return "addition";
  if (removed) return "deletion";
  return null;
}

/**
 * Where every change sits in the working document, for the marker strip.
 *
 * `totalLines` is the editor's own line count rather than anything derived from
 * the diff: the strip runs the height of the document being scrolled, so the
 * marks have to be placed against that same scale.
 *
 * The diff and the document are fetched separately, so a file written between
 * the two calls can leave a hunk pointing past the end. Marks are clamped into
 * the strip rather than dropped — a mark in slightly the wrong place is still a
 * signal that something changed down there, whereas silently omitting it makes
 * an incomplete picture look complete.
 */
export function changeMarks(diff: FileDiff, totalLines: number): ChangeMark[] {
  if (totalLines <= 0) return [];

  const marks: ChangeMark[] = [];

  for (const hunk of diff.hunks) {
    const kind = kindOf(hunk);
    if (kind === null) continue;

    // A pure deletion has `newLines === 0` — it occupies no line in the working
    // copy — but it is still a change the reviewer needs to be able to find, so
    // it gets a single line's worth of mark at the point it was removed from.
    const span = Math.max(hunk.newLines, 1);
    const start = Math.max(hunk.newStart, 1);

    const top = Math.min(1, (start - 1) / totalLines);
    const height = Math.min(1 - top, span / totalLines);

    marks.push({ top, height, kind });
  }

  return marks;
}

/**
 * The line to jump to for the next (`1`) or previous (`-1`) change.
 *
 * Wraps at both ends, which is what Rider does and what stops the key reading
 * as broken once the last change is reached. `null` means there is nothing to
 * jump to at all.
 */
export function nextChangeLine(
  diff: FileDiff,
  fromLine: number,
  direction: 1 | -1,
): number | null {
  const starts = diff.hunks
    .filter((hunk) => kindOf(hunk) !== null)
    .map((hunk) => Math.max(hunk.newStart, 1))
    .sort((a, b) => a - b);

  const first = starts.at(0);
  const last = starts.at(-1);
  if (first === undefined || last === undefined) return null;

  return direction === 1
    ? (starts.find((line) => line > fromLine) ?? first)
    : (starts.filter((line) => line < fromLine).pop() ?? last);
}

/** A document with its redundant whitespace removed, and the way back. */
export interface Normalised {
  text: string;
  /**
   * `map[i]` is the offset in the original document that normalised character
   * `i` came from. One longer than `text`, so an exclusive end offset maps too.
   */
  map: number[];
}

/**
 * Strip whitespace that a reviewer asked to ignore.
 *
 * Leading and trailing whitespace on each line goes, internal runs collapse to
 * a single space, and carriage returns go — so a reindent, a reflow and a
 * line-ending change all stop being changes. **Newlines are kept**, because the
 * merge view builds its chunks from line boundaries and losing them would make
 * the two panes stop lining up.
 *
 * The map is the whole point: the diff is computed over the normalised text but
 * has to be *drawn* on the real document, so every offset needs a way back.
 */
export function normaliseWhitespace(source: string): Normalised {
  const out: string[] = [];
  const map: number[] = [];

  let atLineStart = true;
  let pendingSpace = false;

  for (let i = 0; i < source.length; i++) {
    const char = source[i] as string;

    if (char === "\n") {
      out.push("\n");
      map.push(i);
      atLineStart = true;
      // Whitespace held back at the end of a line is trailing: drop it.
      pendingSpace = false;
      continue;
    }

    if (char === " " || char === "\t" || char === "\r" || char === "\f" || char === "\v") {
      // Held rather than emitted, so a run at the end of a line can be
      // discarded once the newline arrives.
      if (!atLineStart) pendingSpace = true;
      continue;
    }

    if (pendingSpace) {
      out.push(" ");
      // The collapsed run stands for the character it precedes.
      map.push(i);
      pendingSpace = false;
    }

    out.push(char);
    map.push(i);
    atLineStart = false;
  }

  map.push(source.length);
  return { text: out.join(""), map };
}

/**
 * A normalised offset back in original coordinates.
 *
 * Clamped rather than trusted: the offsets come from a diff over the normalised
 * text so they should always be in range, but an out-of-range lookup would
 * yield `undefined`, and `undefined` reaching a CodeMirror range throws and
 * takes the editor down with it.
 */
export function mapOffset(map: number[], offset: number): number {
  const clamped = Math.min(Math.max(offset, 0), map.length - 1);
  return map[clamped] ?? 0;
}

/** What the horizontal scrollbar is being asked to represent. */
export interface ScrollMetrics {
  /** Width of the widest line, in pixels. */
  contentWidth: number;
  /** Width of the visible part of it. */
  viewportWidth: number;
  /** How far right the panes are currently scrolled. */
  scrollLeft: number;
  /** Width of the scrollbar's track. */
  trackWidth: number;
}

/** Where to draw the scrollbar's thumb, in track pixels. */
export interface ScrollThumb {
  left: number;
  width: number;
  /** False when the content fits and the bar should be hidden entirely. */
  scrollable: boolean;
}

/** Below this the thumb is too small to grab. */
const MIN_THUMB_WIDTH = 20;

/**
 * Size and place the horizontal scrollbar's thumb.
 *
 * The diff needs a scrollbar of its own because `@codemirror/merge` forces both
 * editors to full-document height, which puts their native horizontal
 * scrollbars thousands of pixels below the viewport — present, but unreachable.
 * See `DiffView`.
 *
 * Every division here is guarded: the bar is measured on mount, before layout
 * has given it a width, and a `NaN` reaching a style property silently drops
 * the rule rather than failing loudly.
 */
export function scrollThumb(metrics: ScrollMetrics): ScrollThumb {
  const { contentWidth, viewportWidth, scrollLeft, trackWidth } = metrics;
  const overflow = contentWidth - viewportWidth;

  if (!(overflow > 0) || !(trackWidth > 0) || !(contentWidth > 0)) {
    return { left: 0, width: trackWidth > 0 ? trackWidth : 0, scrollable: false };
  }

  const width = Math.max(
    MIN_THUMB_WIDTH,
    Math.min(trackWidth, (viewportWidth / contentWidth) * trackWidth),
  );
  const travel = Math.max(0, trackWidth - width);
  const progress = Math.min(1, Math.max(0, scrollLeft / overflow));

  return { left: travel * progress, width, scrollable: true };
}

/**
 * The scroll offset a click or drag at `position` along the track asks for.
 *
 * The inverse of `scrollThumb`, so grabbing the thumb and dropping it somewhere
 * lands where the thumb was drawn.
 */
export function scrollLeftForThumb(metrics: ScrollMetrics, thumbLeft: number): number {
  const { contentWidth, viewportWidth, trackWidth } = metrics;
  const overflow = contentWidth - viewportWidth;

  if (!(overflow > 0) || !(trackWidth > 0)) return 0;

  const { width } = scrollThumb(metrics);
  const travel = Math.max(0, trackWidth - width);
  if (travel === 0) return 0;

  const progress = Math.min(1, Math.max(0, thumbLeft / travel));
  return overflow * progress;
}

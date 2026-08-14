/**
 * The CodeMirror half of Find Usages / Go To Definition: decorations, a widget,
 * a mouse handler and a viewport notifier, and **no decisions at all**.
 *
 * Everything that could be got wrong about *what to say* lives in
 * `usagesLogic.ts`, where vitest can reach it. This file is deliberately the
 * part that cannot be tested here — the suite runs in a node environment with no
 * DOM, so an `EditorView`, a `WidgetType` and a `MouseEvent` are all
 * unreachable. It therefore contains no branch on `Availability`, no phrasing,
 * no count arithmetic and no grouping: it is handed a
 * {@link UsageRowView} that `usagesLogic.usageRowView` already produced and it
 * draws that object's `text`, `tooltip` and `tone` verbatim. The only condition
 * it evaluates is `action.kind === "dropdown"`, which is the logic module's own
 * answer to "is this row clickable", not a second opinion about it.
 *
 * It also renders **no dropdown and no picker**. A click reports itself upward
 * with the anchor and the row's bounding rectangle; React draws the menu in a
 * positioned host outside the editor. DOM injected inside CodeMirror fights the
 * editor's own event handling and gets torn out by the next viewport update, so
 * the overlay has to live where React's lifecycle owns it.
 *
 * ## Positions
 *
 * CodeMirror `Line.number` is **1-based** (`doc.line(1)` is the first line), and
 * an offset within a line is a UTF-16 code-unit count. The IPC boundary wants a
 * **1-based line** and a **0-based UTF-16 character** — so `line.number` and
 * `pos - line.from` are already in the wire's units and this file converts
 * nothing. That is worth stating because the surrounding convention is mixed on
 * purpose (see `Target.character` in `ipc/types.ts`): had CodeMirror been
 * 0-based here, every callback below would need a `+1`.
 *
 * ## Styling
 *
 * The class names below (`cb-usages-row`, `cb-usages-label`,
 * `cb-usages-<tone>`, `cb-usages-clickable`) are the whole styling contract, and
 * they are exported as constants so `styles.css` and the tests of neither can
 * drift apart silently. Only layout-critical, non-visual rules are set here, via
 * `EditorView.baseTheme`, which has lower precedence than any stylesheet — so a
 * rule in `styles.css` always wins and this file makes no colour or type
 * choices.
 */
import { StateEffect, StateField, type Extension } from "@codemirror/state";
import {
  Decoration,
  EditorView,
  ViewPlugin,
  WidgetType,
  type DecorationSet,
  type ViewUpdate,
} from "@codemirror/view";
import type { DeclarationAnchor } from "../ipc/types";
import { actionDetail, toneClass, type UsageRowView } from "./usagesLogic";

// ---------------------------------------------------------------------------
// Class names — the styling contract.
// ---------------------------------------------------------------------------

/** The block row drawn above a declaration. */
export const ROW_CLASS = "cb-usages-row";
/** The text inside the row; the click target when the row is actionable. */
export const LABEL_CLASS = "cb-usages-label";
/** Added to the label when clicking it opens the dropdown. */
export const CLICKABLE_CLASS = "cb-usages-clickable";
/**
 * `cb-usages-idle` … `cb-usages-reason`, re-exported so the whole styling
 * contract still reads in one place.
 *
 * It *lives* in `usagesLogic.ts`: it is pure, and this file is outside
 * `vite.config.ts`'s coverage glob, so a decision left here is unmeasured.
 */
export { toneClass };

// ---------------------------------------------------------------------------
// What the host sets, and what it gets told.
// ---------------------------------------------------------------------------

/**
 * One anchor and the row it should show, as `usagesLogic` decided it.
 *
 * The anchor travels alongside the view because the click payload needs it: the
 * dropdown is fetched (or read from cache) against `selectionLine`/`character`,
 * and the widget must not have to reconstruct either.
 */
export interface UsageRowSpec {
  anchor: DeclarationAnchor;
  view: UsageRowView;
}

/** A click on an actionable row, with everything needed to place a menu. */
export interface UsageRowClick {
  anchor: DeclarationAnchor;
  /** The very view that was on screen when it was clicked — not a re-derivation. */
  view: UsageRowView;
  /**
   * The label's rectangle in viewport coordinates.
   *
   * A `DOMRect` rather than a point so the host can hang the menu off the row's
   * edge and flip it when it would fall off screen. Read at click time: the
   * editor scrolls, so a rectangle captured earlier would be stale.
   */
  rect: DOMRect;
}

/** A middle-click asking for the definition of whatever is under the pointer. */
export interface GotoRequest {
  /** **1-based**, ready for `lspGotoDefinition` with no adjustment. */
  line: number;
  /** **0-based UTF-16 code units** into that line, likewise ready. */
  character: number;
  /** Where the pointer was, in viewport coordinates, for placing the picker. */
  x: number;
  y: number;
}

/** The 1-based, inclusive span of lines the user can currently see. */
export interface VisibleLines {
  firstVisibleLine: number;
  lastVisibleLine: number;
}

/**
 * What the extension reports upward. Nothing here fetches anything.
 *
 * These functions are captured once, when the extension is built, and the editor
 * is built once per file — so pass functions that read the latest React state
 * through a ref (the `handlers.current` pattern `FileEditor` already uses for
 * `onDirtyChange`) rather than fresh closures, which would be frozen at mount.
 */
export interface UsagesCallbacks {
  /** An actionable row was clicked. Draw the dropdown. */
  onRowClick: (click: UsageRowClick) => void;
  /** Middle-click. Ask for the definition, then jump or draw the picker. */
  onGoto: (request: GotoRequest) => void;
  /**
   * The visible line span changed, coalesced to at most one call per
   * {@link VIEWPORT_COALESCE_MS}.
   *
   * Feed it to `usagesLogic.visibleAnchors` to decide what to request. It is
   * called asynchronously — never from inside a CodeMirror update — so it is safe
   * to dispatch {@link setUsageRows} straight back from it.
   */
  onVisibleLinesChange: (visible: VisibleLines) => void;
}

/**
 * How long scroll activity is allowed to settle before the host is told.
 *
 * A single wheel gesture produces dozens of updates and each one must not become
 * a workspace-wide references query. Coalescing also moves the callback off the
 * update cycle, which is what makes dispatching from it legal.
 */
export const VIEWPORT_COALESCE_MS = 120;

/**
 * Replace every inline row.
 *
 * Whole-set replacement rather than per-anchor patching: the anchor list is
 * re-derived wholesale by `lspDeclarationAnchors`, and a diffing protocol here
 * would be a second model of the truth that could disagree with the first. Rows
 * whose line falls outside the document are dropped — an anchor list is a
 * snapshot of a buffer the user is free to have shortened since.
 */
export const setUsageRows = StateEffect.define<UsageRowSpec[]>();

// ---------------------------------------------------------------------------
// The widget.
// ---------------------------------------------------------------------------

/**
 * The first `WidgetType` in this repository, so its `eq` is written out
 * deliberately.
 *
 * CodeMirror keeps the **old** widget instance when `eq` returns true, and reuses
 * its DOM. That cuts both ways: comparing too little means a redraw that
 * discards the click target mid-gesture and makes the row flicker, while
 * comparing too little *in the other direction* means the surviving instance
 * keeps the old `spec` and hands the host a stale count when the row is clicked.
 * So everything that is either rendered or forwarded is compared — text,
 * tooltip, tone, actionability and the count inside it, plus the anchor's
 * identity and the position a request would be aimed at. None of these change on
 * a keystroke (they only arrive via {@link setUsageRows}), so the comparison
 * costs nothing in the common case.
 */
class UsageRowWidget extends WidgetType {
  constructor(
    readonly spec: UsageRowSpec,
    readonly onClick: (click: UsageRowClick) => void,
  ) {
    super();
  }

  eq(other: UsageRowWidget): boolean {
    const a = this.spec;
    const b = other.spec;
    return (
      a.anchor.id === b.anchor.id &&
      a.anchor.selectionLine === b.anchor.selectionLine &&
      a.anchor.character === b.anchor.character &&
      a.view.text === b.view.text &&
      a.view.tooltip === b.view.tooltip &&
      a.view.tone === b.view.tone &&
      a.view.total === b.view.total &&
      a.view.truncated === b.view.truncated &&
      a.view.action.kind === b.view.action.kind &&
      actionDetail(a.view) === actionDetail(b.view)
    );
  }

  toDOM(): HTMLElement {
    const { view } = this.spec;
    const row = document.createElement("div");
    row.className = ROW_CLASS;
    // Not part of the editable document: without this the browser will happily
    // put a caret inside the row and CodeMirror will try to map it to a
    // document position that does not exist.
    row.contentEditable = "false";

    // The row is the largest target in the pane — a full-width line above every
    // declaration, right where the pointer already is — and a middle-click that
    // lands on it never reaches `middleClickGoto`. CodeMirror's own input handler
    // drops the event first: `eventBelongsToEditor` walks up from `event.target`
    // and bails as soon as it finds a widget whose `ignoreEvent()` is true, which
    // is this widget's (deliberately, see below). So the `preventDefault` that
    // stops the Windows autoscroll cursor has to be here as well as there, on
    // mousedown, before the browser acts on it. Nothing is forwarded: the row is
    // not a document position and guessing one from it would send a goto request
    // at whatever happened to be nearest.
    row.addEventListener("mousedown", (event) => {
      if (event.button === 1) event.preventDefault();
    });

    const label = document.createElement("span");
    label.className = `${LABEL_CLASS} ${toneClass(view.tone)}`;
    label.textContent = view.text;
    if (view.tooltip !== null) label.title = view.tooltip;

    if (view.action.kind === "dropdown") {
      label.classList.add(CLICKABLE_CLASS);
      label.setAttribute("role", "button");
      // On mousedown, not on click: the default action of pressing a button
      // inside an editor is to move the selection and steal focus, and by the
      // time `click` fires that has already happened.
      label.addEventListener("mousedown", (event) => {
        if (event.button !== 0) return;
        event.preventDefault();
      });
      label.addEventListener("click", (event) => {
        if (event.button !== 0) return;
        event.preventDefault();
        event.stopPropagation();
        this.onClick({
          anchor: this.spec.anchor,
          view: this.spec.view,
          rect: label.getBoundingClientRect(),
        });
      });
    } else {
      label.setAttribute("aria-disabled", "true");
    }

    row.appendChild(label);
    return row;
  }

  /**
   * Keep the editor out of events inside the row.
   *
   * This is the default, and it is spelled out because the row's whole job is to
   * be clicked: were it false, CodeMirror would also interpret the click as a
   * selection gesture.
   */
  ignoreEvent(): boolean {
    return true;
  }
}

// ---------------------------------------------------------------------------
// The decoration field.
// ---------------------------------------------------------------------------

/**
 * The rows, as block widgets above their declaration lines.
 *
 * Follows the `StateEffect` + `StateField<DecorationSet>` shape already used by
 * `DiffView`, with two differences: the decorations are `Decoration.widget`
 * (`block: true`, `side: -1`, i.e. its own line above the anchor rather than a
 * class on the anchor's line), and the set is built sorted, because a block
 * widget set that is not in document order is rejected by `RangeSet`.
 *
 * On a document change the existing rows are **mapped**, not dropped: the widget
 * follows its declaration as the text above it grows. Deciding that a row's
 * count has gone stale is the host's job and it has the version-stamped cache key
 * (`usagesLogic.usageCacheKey`) to do it with; throwing the rows away here would
 * make every keystroke blank every row.
 */
function rowField(onClick: (click: UsageRowClick) => void): StateField<DecorationSet> {
  return StateField.define<DecorationSet>({
    create: () => Decoration.none,
    update(value, tr) {
      for (const effect of tr.effects) {
        if (!effect.is(setUsageRows)) continue;

        const marks = [];
        for (const spec of effect.value) {
          if (spec.anchor.line < 1 || spec.anchor.line > tr.state.doc.lines) continue;
          const line = tr.state.doc.line(spec.anchor.line);
          marks.push(
            Decoration.widget({
              widget: new UsageRowWidget(spec, onClick),
              block: true,
              side: -1,
            }).range(line.from),
          );
        }
        return Decoration.set(marks, true);
      }
      return tr.docChanged ? value.map(tr.changes) : value;
    },
    provide: (field) => EditorView.decorations.from(field),
  });
}

// ---------------------------------------------------------------------------
// Viewport reporting.
// ---------------------------------------------------------------------------

/**
 * The 1-based, inclusive line span currently on screen, or `null` for none.
 *
 * `visibleRanges` is what CodeMirror has actually rendered, which is the right
 * question — `viewport` can be wider than the visible area. Exported so the host
 * can ask once immediately after mounting, before any scroll has happened.
 */
export function visibleLineRange(view: EditorView): VisibleLines | null {
  const ranges = view.visibleRanges;
  if (ranges.length === 0) return null;
  const doc = view.state.doc;
  let first = Infinity;
  let last = -Infinity;
  for (const range of ranges) {
    first = Math.min(first, doc.lineAt(range.from).number);
    last = Math.max(last, doc.lineAt(range.to).number);
  }
  if (!Number.isFinite(first) || !Number.isFinite(last)) return null;
  return { firstVisibleLine: first, lastVisibleLine: last };
}

/**
 * Tell the host, at most once per {@link VIEWPORT_COALESCE_MS}, which lines are
 * visible.
 *
 * The timer does two jobs. It stops one scroll gesture from becoming dozens of
 * references queries, and it moves the callback out of the update cycle — a
 * callback that dispatched {@link setUsageRows} synchronously from inside
 * `update` would be dispatching during an update, which CodeMirror forbids.
 *
 * It fires once on construction too: the first paint is a viewport change from
 * nothing, and nothing else would report it.
 */
function viewportReporter(onChange: (visible: VisibleLines) => void) {
  return ViewPlugin.fromClass(
    class {
      private timer: ReturnType<typeof setTimeout> | null = null;

      constructor(readonly view: EditorView) {
        this.schedule();
      }

      update(update: ViewUpdate) {
        if (update.viewportChanged || update.docChanged || update.geometryChanged) {
          this.schedule();
        }
      }

      destroy() {
        if (this.timer !== null) clearTimeout(this.timer);
        this.timer = null;
      }

      private schedule() {
        if (this.timer !== null) return;
        this.timer = setTimeout(() => {
          this.timer = null;
          const visible = visibleLineRange(this.view);
          if (visible) onChange(visible);
        }, VIEWPORT_COALESCE_MS);
      }
    },
  );
}

// ---------------------------------------------------------------------------
// Middle-click.
// ---------------------------------------------------------------------------

/**
 * Middle-click reports a document position upward; it does not navigate.
 *
 * `domEventHandlers` rather than a listener on the host div, so it composes into
 * the extension array and sits inside CodeMirror's own handling instead of racing
 * it. Two things it has to get right, both already learned in this app:
 *
 * * `preventDefault()` must happen on **mousedown**, before anything else and
 *   whether or not a position can be resolved. On Windows a middle mousedown
 *   that reaches the browser starts the autoscroll cursor, and cancelling the
 *   later `auxclick` is too late (the precedent, with its comment, is the file
 *   tab close handler in `RunView.tsx`).
 * * `posAtCoords` can return `null` — a click past the last line, or on the
 *   gutter. That is not a position and must not be rounded to one, or a stray
 *   click in the margin sends a goto request aimed at whatever happens to be
 *   nearest.
 */
function middleClickGoto(onGoto: (request: GotoRequest) => void): Extension {
  return EditorView.domEventHandlers({
    mousedown(event, view) {
      if (event.button !== 1) return false;
      event.preventDefault();

      const pos = view.posAtCoords({ x: event.clientX, y: event.clientY });
      if (pos == null) return true;

      const line = view.state.doc.lineAt(pos);
      onGoto({
        // `Line.number` is already 1-based and `pos - line.from` is already a
        // 0-based UTF-16 offset: exactly the wire's two conventions.
        line: line.number,
        character: pos - line.from,
        x: event.clientX,
        y: event.clientY,
      });
      return true;
    },
  });
}

// ---------------------------------------------------------------------------
// The factory.
// ---------------------------------------------------------------------------

/**
 * Layout-only rules, at base precedence so `styles.css` overrides all of them.
 *
 * Everything here is about the row not interfering with the text: no colour, no
 * font, no size. The visual design belongs in the stylesheet.
 */
const usagesBaseTheme = EditorView.baseTheme({
  [`.${ROW_CLASS}`]: { userSelect: "none" },
  [`.${LABEL_CLASS}`]: { cursor: "default" },
  [`.${CLICKABLE_CLASS}`]: { cursor: "pointer" },
});

/**
 * The whole feature as one extension array: the rows, the middle-click handler
 * and the viewport reporter.
 *
 * Spread it into `FileEditor`'s `extensions` array. Then drive it with
 * `view.dispatch({ effects: setUsageRows.of(rows) })` whenever the anchors or
 * their counts change — which includes setting a row to its `idle` or `pending`
 * view, because a row is drawn as soon as its anchor is known and long before
 * there is a count.
 */
export function usagesExtension(callbacks: UsagesCallbacks): Extension[] {
  return [
    rowField(callbacks.onRowClick),
    middleClickGoto(callbacks.onGoto),
    viewportReporter(callbacks.onVisibleLinesChange),
    usagesBaseTheme,
  ];
}

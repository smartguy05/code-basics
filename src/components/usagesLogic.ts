/**
 * Every decision the usages UI makes, with no React, no DOM and no CodeMirror.
 *
 * This module is the whole testable surface of the Find Usages / Go To Definition
 * feature. The vitest suite runs in a node environment with no jsdom, so a
 * `WidgetType`, an `EditorView` or a `MouseEvent` cannot be reached from a test —
 * which is precisely why nothing that *decides* anything may live in the
 * components. `FileEditor.tsx` and the dropdown around it are rendering shells
 * that call the functions below and draw what they return.
 *
 * ## The one rule this file exists to enforce
 *
 * `cb-core` went to considerable trouble not to answer a question it could not
 * answer: `Availability` has **six** variants so that "no server is configured",
 * "the server is starting", "it is loading its projects", "it died", "it does not
 * provide this capability" and "there genuinely are no usages" can never collapse
 * into each other, and `UsageResult.total` is `number | null` so that a count
 * which might still change cannot be rendered at all. All of that honesty is
 * spent the instant a widget draws `0 usages` over a server that has not
 * finished starting.
 *
 * So: **a count is produced here only for `outcome === "ready"`**, and every
 * other outcome yields words instead of a number. {@link usageRowView} is the
 * only function that turns a result into row text, and it is written so that a
 * component *cannot* render a number from a non-ready answer — the number is not
 * in what it returns.
 *
 * ## Positions
 *
 * Nothing here converts a position. At the IPC boundary `line` is 1-based and
 * `character` is 0-based UTF-16 code units, in both directions; the ±1 on lines
 * belongs at the CodeMirror seam in `FileEditor.tsx`, once. The one offset this
 * file does touch is {@link Highlight}, whose `start`/`end` are UTF-16 offsets
 * into `snippet` — the same units `String.prototype.slice` counts in, so
 * {@link snippetParts} is a slice and not a conversion. Contrast
 * `SearchHit.positions`, which are code *point* indices and need `Array.from`;
 * the two are not interchangeable.
 *
 * Type-only imports, so this module pulls in no runtime dependency and the test
 * file needs no environment behind it.
 */
import type {
  Availability,
  DeclarationAnchor,
  DefinitionResult,
  Highlight,
  Target,
  Usage,
  UsageResult,
} from "../ipc/types";

// ---------------------------------------------------------------------------
// 1 + 2. The inline row.
// ---------------------------------------------------------------------------

/**
 * Where one anchor's usage request has got to.
 *
 * `idle` is a first-class state rather than an absent result, because the row is
 * drawn as soon as the anchor is known and *before* anything has been asked. A
 * component that modelled "not asked" as `undefined` would reach for `??` and
 * land on whichever phrasing came first, which is how a never-asked row starts
 * claiming a number.
 */
export type UsageRequestState =
  | { status: "idle" }
  | { status: "pending" }
  | { status: "answered"; result: UsageResult };

/**
 * What clicking the row does.
 *
 * A discriminated union rather than a boolean, so the component cannot pair
 * "clickable" with the wrong text: the count a dropdown would show travels
 * *inside* the `dropdown` variant, and an `inert` row has no count to draw at
 * all. It still carries its `reason`, because an inert row must explain itself
 * on hover — silence there is what makes a feature look broken.
 */
export type UsageRowAction =
  | { kind: "dropdown"; total: number }
  | { kind: "inert"; reason: string | null };

/**
 * How a row should read, which is also a style hook.
 *
 * `empty` and `reason` are deliberately separate: an empty *answer* is a fact
 * about the code, and a reason is a fact about the tooling, and they should not
 * look alike on screen.
 *
 * `reason` therefore covers two things: a row with no answer at all, and a row
 * whose answer the backend qualified (see {@link UsageRowView.provisional}) — both
 * are statements about the tooling, and both want the dotted rule that says the
 * `title` is worth reading. `empty` is reserved for a zero the server stood
 * behind. The five tones are the stylesheet's contract (`.cb-usages-*`), so a
 * sixth here is a rule that does not exist there.
 */
export type UsageTone = "idle" | "waiting" | "count" | "empty" | "reason";

/** Everything the inline row draws. */
export interface UsageRowView {
  /** The row's own text. Never contains a number unless {@link total} is set. */
  text: string;
  /** The longer explanation, for a `title`. `null` when the text says it all. */
  tooltip: string | null;
  action: UsageRowAction;
  /**
   * The true count, or `null` when there is no count that may be shown.
   *
   * `0` is a real answer and is not `null`; branch on `=== null`, never on
   * falsiness. When {@link truncated} is set this is still the full count and is
   * larger than the number of rows the dropdown can list.
   */
  total: number | null;
  /** Whether the dropdown's rows are fewer than {@link total}. Say so there. */
  truncated: boolean;
  tone: UsageTone;
  /**
   * True when this row shows an answer the **backend itself qualified**: a `ready`
   * result that also carried a `message`.
   *
   * `true` exactly when `outcome === "ready" && message !== null`, so
   * {@link total} is a lower bound rather than a count and {@link text} says so.
   * Exposed as a flag rather than left to the component to re-derive from
   * `result.message`, because the component does not see the result — and the one
   * time this decision was left implicit, a caveated zero rendered as "No usages"
   * over a method with one usage.
   */
  provisional: boolean;
}

/**
 * One sentence per `Availability`, and the tone that goes with it.
 *
 * Exported because the goto path needs the same words for the same states, and
 * two phrasings of "the server is still loading" would eventually disagree about
 * whether that means there are no usages.
 *
 * `unsupported`'s wording is the one to be careful with. "This server cannot
 * answer" and "there are none" are opposite claims, and the whole capability gate
 * in `lsp/client.rs` exists so the UI can tell them apart — so the text says what
 * the server cannot do and never uses the word "no".
 */
export function availabilityPhrase(outcome: Availability): {
  text: string;
  tone: UsageTone;
} {
  switch (outcome) {
    case "ready":
      // Reached whenever a `ready` result carries no usable `total` — a backend
      // contradicting its own contract, which {@link usageRowView} is written to
      // survive rather than to trust. So this is a real row and not a placeholder:
      // it must not say "Ready" (a row with no answer stating that the answer is
      // in) and must not take the `count` tone, which the stylesheet reserves for
      // the one tone carrying an answer. It was documented as never shown; it is
      // shown on exactly that path.
      return { text: "Usages unknown", tone: "reason" };
    case "starting":
      return { text: "Language server starting…", tone: "waiting" };
    case "loading":
      return { text: "Language server loading…", tone: "waiting" };
    case "notConfigured":
      return { text: "No language server", tone: "reason" };
    case "failed":
      return { text: "Language server failed", tone: "reason" };
    case "unsupported":
      return { text: "This server cannot answer", tone: "reason" };
  }
}

/**
 * The fallback explanation for an outcome whose `message` was `null`.
 *
 * The backend almost always sends one and it is better than anything written
 * here, so this is only ever a floor. It exists because an inert row with an
 * empty tooltip is indistinguishable from a bug.
 */
function fallbackReason(outcome: Availability): string {
  switch (outcome) {
    case "ready":
      return "The language server answered.";
    case "starting":
      return "The language server is still starting; there is no answer yet.";
    case "loading":
      return "The language server is still loading this workspace; there is no answer yet.";
    case "notConfigured":
      return "No language server is configured for this file's language.";
    case "failed":
      return "The language server failed, so there is no answer.";
    case "unsupported":
      return "This language server does not provide find-usages, so the number of usages is unknown.";
  }
}

/**
 * How a count reads, in one place.
 *
 * The inline row and the dropdown's heading say the same fact, and a second
 * pluralisation written beside the first diverges at zero first: "0 usages" is
 * exactly the phrasing {@link usageRowView} refuses, because "there are none" is
 * a sentence about the code and a bare zero reads like a failure to look.
 *
 * `provisional` is set when the backend qualified its own answer (a `ready`
 * result carrying a `message`, i.e. `ReadyWithCaveat`). Then `total` is a **lower
 * bound**, and the words change to match:
 *
 * * a positive count becomes "at least N usages" — the number is still useful and
 *   is still the largest thing known, it is simply not a total;
 * * zero becomes "Usages unknown" and drops the number entirely. "at least 0" is
 *   true of every possible answer, so it carries nothing and still reads as
 *   "none" — which is the wrong answer observed in the running app, where this
 *   row said "No usages" about a method with one usage at `Walker.cs:138`.
 */
export function usageCountLabel(total: number, provisional: boolean = false): string {
  if (provisional) {
    if (total === 0) return "Usages unknown";
    return total === 1 ? "at least 1 usage" : `at least ${total} usages`;
  }
  return total === 0 ? "No usages" : total === 1 ? "1 usage" : `${total} usages`;
}

/**
 * An `invoke` rejection, as the honest `UsageResult` it is.
 *
 * The IPC call itself failing is not one of the six `Availability` states the
 * backend reports, but it is unambiguously a failure to answer — so it becomes
 * `failed` carrying the error text and {@link usageRowView} renders it exactly
 * like a server that died. The alternative is a row stuck on "Finding usages…"
 * for ever, which reads as a hang.
 *
 * Here rather than in the component because inventing an `Availability` is a
 * decision about the six-way contract, and this is the only place in the frontend
 * that makes one.
 */
export function failedUsageResult(message: string): UsageResult {
  return { outcome: "failed", total: null, usages: [], truncated: false, message, server: null };
}

/**
 * What the inline row above a declaration says, and whether it does anything.
 *
 * The single place a `UsageResult` becomes text. A count is produced **only** for
 * `outcome === "ready"`; `total` is read defensively (`typeof … === "number"`)
 * rather than trusted, so even a backend contradicting its own contract cannot
 * put a number on screen under a "starting" server.
 *
 * A `ready` answer that carries a `message` is a **different row** from a plain
 * `ready` answer, not the same row with a tooltip added. The backend sends that
 * message when it promoted a server at its 90 s readiness ceiling and says "a
 * count may be low", so the number is a floor: the text becomes
 * {@link usageCountLabel}'s provisional wording, {@link provisional} is set, and
 * the tone is `reason` rather than `count`/`empty` — a claim about the tooling,
 * not about the code. The row stays clickable, because the dropdown is where the
 * message is read at length.
 *
 * The four `ready` shapes — with and without a message, at zero and above — all
 * produce different `text`, which is what a test can hold onto. Before that was
 * true, the row above `TryGetElements` read "No usages" about a method with one
 * usage, and the message explaining why was thrown away.
 */
export function usageRowView(state: UsageRequestState): UsageRowView {
  if (state.status === "idle") {
    return {
      text: "Usages",
      tooltip: null,
      action: { kind: "inert", reason: null },
      total: null,
      truncated: false,
      tone: "idle",
      provisional: false,
    };
  }
  if (state.status === "pending") {
    return {
      text: "Finding usages…",
      tooltip: null,
      action: { kind: "inert", reason: null },
      total: null,
      truncated: false,
      tone: "waiting",
      provisional: false,
    };
  }

  const { outcome, message, truncated } = state.result;
  const total = state.result.total;
  if (outcome === "ready" && typeof total === "number") {
    const provisional = message !== null;
    return {
      text: usageCountLabel(total, provisional),
      tooltip: message,
      action: { kind: "dropdown", total },
      total,
      truncated,
      tone: provisional ? "reason" : total === 0 ? "empty" : "count",
      provisional,
    };
  }

  const phrase = availabilityPhrase(outcome);
  const reason = message ?? fallbackReason(outcome);
  return {
    text: phrase.text,
    tooltip: reason,
    action: { kind: "inert", reason },
    total: null,
    truncated: false,
    tone: phrase.tone,
    provisional: false,
  };
}

// ---------------------------------------------------------------------------
// 3. Grouping usages for the dropdown.
// ---------------------------------------------------------------------------

/** One row of the dropdown. */
export interface UsageRow {
  usage: Usage;
  /**
   * False when `usage.path` is `null` — a `source-generated:` or metadata
   * document that exists, is counted, and cannot be opened. Render it
   * unclickable; dropping it would contradict `UsageResult.total`.
   */
  openable: boolean;
}

/** The dropdown's rows for one document. */
export interface UsageGroup {
  /** The heading: the workspace-relative path, or the raw URI. */
  label: string;
  /** The path every row in the group shares, or `null` for a pathless document. */
  path: string | null;
  /** Whether the group's document can be opened at all. */
  openable: boolean;
  rows: UsageRow[];
}

/**
 * Collect usages into per-document groups, **preserving the backend's order**.
 *
 * `lsp/results.rs` already sorts by path, line and character and already dedups,
 * so re-sorting here would be a second opinion about ordering and would sooner or
 * later disagree with the count. Groups appear in first-seen order and rows keep
 * the order they arrived in.
 *
 * Grouping is keyed on the *path* for a real file and on the raw URI for a
 * pathless one, never on the label alone: a metadata document whose label happens
 * to spell an existing relative path would otherwise be folded into that file's
 * group and inherit its openability.
 */
export function groupUsages(usages: Usage[]): UsageGroup[] {
  const groups: UsageGroup[] = [];
  const byKey = new Map<string, UsageGroup>();
  for (const usage of usages) {
    const openable = usage.path !== null;
    const key = openable ? `p\0${usage.path}` : `u\0${usage.label}`;
    let group = byKey.get(key);
    if (!group) {
      group = { label: usage.label, path: usage.path, openable, rows: [] };
      byKey.set(key, group);
      groups.push(group);
    }
    group.rows.push({ usage, openable });
  }
  return groups;
}

// ---------------------------------------------------------------------------
// 4. What middle-click does.
// ---------------------------------------------------------------------------

/** One section of the goto picker. */
export interface DefinitionGroup {
  /** `Declarations` / `Implementations` / `Type definitions`. */
  label: string;
  targets: Target[];
  /** True when this group has no targets. Kept in the picker regardless. */
  empty: boolean;
  /**
   * A note about *this* group specifically, or `null`.
   *
   * Always `null` today, and that is a decision rather than an omission. The
   * backend's `DefinitionResult.message` is English prose that names the group it
   * concerns ("No implementations: …"), and there is one message for three lists.
   * Copying it under every empty group would put an implementations sentence
   * under Type definitions, and parsing the prose to find out which group it
   * means is exactly the guess this subsystem refuses to make. So the message is
   * shown once, on {@link DefinitionAction}, and the field stays for a future
   * backend that names the group in a machine-readable way.
   */
  note: string | null;
}

/**
 * What a middle-click should do, as data the component cannot misread.
 *
 * The product rule, fixed by the user: exactly one target across all three groups
 * jumps; more than one opens a picker; none shows a note. **Never a silent pick**
 * — which is why this is a union and not a `Target | null` with a list beside it.
 */
export type DefinitionAction =
  | { kind: "jump"; target: Target }
  | {
      kind: "pick";
      groups: DefinitionGroup[];
      message: string | null;
      /**
       * The outcome the lists came from, which a picker must show.
       *
       * A list can be non-empty *and* provisional — a loading server can answer
       * `definition` while it is still resolving implementations — and a picker
       * that drew that identically to a settled answer would state "no
       * implementations" about a question nobody could ask yet. See
       * {@link partialAnswerNote}.
       */
      outcome: Availability;
    }
  | { kind: "none"; message: string; outcome: Availability };

/**
 * What an empty group in the picker may claim.
 *
 * `DefinitionResult` carries **one** `message` for three lists and names the
 * group it concerns in English prose, so an empty group standing beside a message
 * is not evidence of emptiness — it may be the group that was refused. Saying
 * "None." there is the "unsupported reads as there are none" failure the whole
 * capability gate exists to prevent, so the words are only licensed when there is
 * nothing to have been refused.
 */
export function emptyGroupNote(message: string | null): string {
  return message === null ? "None." : "Not reported — see the note above.";
}

/**
 * The warning a picker needs when its lists came from a server that is not
 * settled, or `null` for a `ready` answer.
 *
 * Uses {@link availabilityPhrase}'s words rather than its own, so the picker and
 * the inline row can never disagree about what the server is doing.
 */
export function partialAnswerNote(outcome: Availability, message: string | null): string | null {
  if (outcome !== "ready") {
    return `${availabilityPhrase(outcome).text} This list may be incomplete.`;
  }
  // `ready` is not the same as settled. A `ready` result carrying a message came
  // from a server promoted at the readiness ceiling, and its lists are as much a
  // lower bound as its counts are — the same assumption `usageRowView` was fixed
  // for. The message itself is rendered separately and at length; this is the one
  // line that says what it means for the list.
  return message === null ? null : "This list may be incomplete.";
}

const GROUP_LABELS = ["Declarations", "Implementations", "Type definitions"] as const;

/**
 * Turn a `DefinitionResult` into the one action it licenses.
 *
 * Three subtleties, all of them from `lsp/model.rs`'s own documentation:
 *
 * * **There is one `outcome` for three lists**, so an empty group on a `ready`
 *   answer does not by itself mean "there are none" — a server advertising
 *   `definitionProvider` but not `implementationProvider` answers the first and
 *   refuses the second while staying `ready`. Hence the picker always shows all
 *   three groups and always surfaces `message`.
 * * **A single target that cannot be opened is not a jump.** A lone
 *   `metadata:///System.String` is a true answer about where the symbol lives and
 *   a useless place to send the editor, so it goes to the picker where it can be
 *   read.
 * * **Zero targets is not automatically "no definition found".** For any outcome
 *   other than `ready` the honest note is the outcome's own words, because
 *   nobody could ask the question.
 * * **A qualified answer is not a jump either.** A `ready` result carrying a
 *   `message` came from a server promoted at the readiness ceiling, and "exactly
 *   one target" is then partly a statement about what could not be asked: Roslyn
 *   with no project loaded resolves `definition` from single-file context and
 *   answers `implementation` with nothing. Jumping would present a symbol with
 *   five implementations as having one place to go *and* discard the sentence
 *   saying so, since only the picker renders it.
 */
export function definitionAction(result: DefinitionResult): DefinitionAction {
  const lists: [Target[], Target[], Target[]] = [
    result.declarations,
    result.implementations,
    result.typeDefinitions,
  ];
  const all = lists.flat();
  // Count *destinations*, not rows. A static method is its own implementation, so
  // `definition` and `implementation` legitimately answer with the same location —
  // observed in the running app, where `Collections.TryGetElements` opened a picker
  // offering one place to go, listed twice. Deduplicating here and nowhere else
  // keeps this a decision about where to go: the groups below are built from the
  // original lists, because a reader does want to see that a symbol is both a
  // declaration and an implementation.
  const destinations = new Map<string, Target>();
  for (const candidate of all) {
    // An unopenable target is keyed by its label, not its null path: two
    // `source-generated:` documents are two different places we cannot go to,
    // and keying on the path alone would merge them into one.
    const key =
      candidate.path === null
        ? `label ${candidate.label} ${candidate.line}`
        : `path ${candidate.path} ${candidate.line}`;
    if (!destinations.has(key)) destinations.set(key, candidate);
  }
  const [only] = destinations.values();

  if (destinations.size === 1 && only && only.path !== null && result.message === null) {
    return { kind: "jump", target: only };
  }
  if (all.length > 0) {
    return {
      kind: "pick",
      groups: GROUP_LABELS.map((label, i) => {
        const targets = lists[i] ?? [];
        return { label, targets, empty: targets.length === 0, note: null };
      }),
      message: result.message,
      outcome: result.outcome,
    };
  }
  if (result.outcome === "ready") {
    return {
      kind: "none",
      message: result.message ?? "No definition found.",
      outcome: result.outcome,
    };
  }
  return {
    kind: "none",
    message: result.message ?? fallbackReason(result.outcome),
    outcome: result.outcome,
  };
}

// ---------------------------------------------------------------------------
// 5. Which anchors are worth asking about.
// ---------------------------------------------------------------------------

/**
 * How far outside the viewport an anchor is still worth a request.
 *
 * A references query is the most expensive thing this feature does — a whole
 * workspace search per anchor — so the editor asks only about what the user can
 * see. The margin buys the row being already filled in when a small scroll
 * brings it on screen, without prefetching a 4,000-line file's every method.
 */
export const ANCHOR_MARGIN_LINES = 20;

/**
 * The anchors near the viewport, in the order they arrived.
 *
 * All three line numbers are 1-based, matching `DeclarationAnchor.line` and the
 * editor gutter. The bounds are inclusive at both ends: an anchor sitting exactly
 * on the first or last visible line is visible, and rounding it out of the set
 * would leave the topmost method in a file with a permanently empty row.
 */
export function visibleAnchors(
  anchors: DeclarationAnchor[],
  firstVisibleLine: number,
  lastVisibleLine: number,
  margin: number = ANCHOR_MARGIN_LINES,
): DeclarationAnchor[] {
  const from = firstVisibleLine - margin;
  const to = lastVisibleLine + margin;
  return anchors.filter((a) => a.line >= from && a.line <= to);
}

// ---------------------------------------------------------------------------
// 5b. Whether to ask for the anchors again.
// ---------------------------------------------------------------------------

/** How long to wait before asking a still-starting server for anchors again. */
export const ANCHOR_RETRY_MS = 2000;

/**
 * How many times, which has to outlast the backend's own patience.
 *
 * `lsp/client.rs::READINESS_CEILING` is 90 s and `session.rs` keeps answering
 * `loading` until then, so a chain that ends sooner stops while the backend is
 * still promising a different answer — and the corner badge is left reading
 * "loading…" with nothing polling behind it, which is a claim about the present
 * tense that has become false. 60 × 2 s = 120 s, comfortably past it.
 */
export const ANCHOR_RETRY_LIMIT = 60;

/**
 * The backend's readiness ceiling, in milliseconds, mirrored here for one test.
 *
 * Not read by any code path — it exists so the relationship above is asserted
 * rather than described. If `READINESS_CEILING` in `crates/core/src/lsp/client.rs`
 * moves, that test is what notices.
 */
export const READINESS_CEILING_MS = 90_000;

/**
 * Whether an anchors answer is worth asking for again.
 *
 * `starting` and `loading` are the only two outcomes that become a different
 * answer on their own — Roslyn takes tens of seconds to load a solution — so they
 * are retried and everything else is final. Retrying `failed` or `notConfigured`
 * would be a poll for a server that is not going to appear.
 *
 * Here rather than in `FileEditor` because it is a five-way judgement about the
 * six-variant contract, and the component is where vitest cannot see one.
 * `attempt` is the number of retries already made, so the first call passes 0.
 */
export function shouldRetryAnchors(outcome: Availability, attempt: number): boolean {
  const retryable = outcome === "starting" || outcome === "loading";
  return retryable && attempt < ANCHOR_RETRY_LIMIT;
}

// ---------------------------------------------------------------------------
// 6. The cache key.
// ---------------------------------------------------------------------------

/**
 * The key a usage count is cached under: one file, one anchor, one document
 * version.
 *
 * The document version is part of the key because a count is only true of the
 * text it was computed over — an edit anywhere can add or remove a call site, and
 * a stale number is the wrong-answer failure this whole subsystem is built
 * against. Discard on version change; do **not** try to decide which edits could
 * not have mattered.
 *
 * The viewport is deliberately *not* part of the key. Scrolling changes nothing
 * about the answer, so scrolling away and back must hit the cache rather than
 * spend another workspace-wide references query.
 *
 * Joined with `\0` — written as the two-character escape, never as a literal NUL
 * byte in the source, which makes the file binary to ripgrep and invisible to code
 * search. A *printable* separator would not do: Windows workspace paths contain
 * spaces routinely and `lsp/results.rs` builds an anchor id from the **raw** symbol
 * name, so a C# id reads `Order.TryGet(ClrObject, int) : bool@12:8`. Under a space
 * separator `("a", "b c")` and `("a b", "c")` are the same key, which is one
 * method's count shown against another. NUL cannot occur in either part.
 */
export function usageCacheKey(path: string, anchorId: string, docVersion: number): string {
  return `${docVersion}\0${path}\0${anchorId}`;
}

// ---------------------------------------------------------------------------
// 6a2. What the answer store keeps, and what it goes back for.
// ---------------------------------------------------------------------------

/**
 * How many times one anchor's count is re-asked after an unsettled answer.
 *
 * Bounded for the reason {@link ANCHOR_RETRY_LIMIT} is bounded, and much smaller
 * because these retries are driven by what the user does — each scroll that moves
 * an anchor into view re-asks — rather than by a timer. A server that is not
 * coming back must not be asked once per scroll event for the life of the tab.
 */
export const UNSETTLED_RETRY_LIMIT = 5;

/** One anchor's answer, with how many unsettled ones preceded it. */
interface UsageAnswer {
  result: UsageResult;
  /** Requests that have been answered under this key, settled or not. */
  attempts: number;
}

/**
 * Every count this document version has, is waiting for, or gave up on.
 *
 * Here rather than as three refs inside `FileEditor` because the *caching rule* is
 * a decision and was the wrong one: every answer was filed under the version key
 * whatever its outcome, `requestVisible` skips any key already answered, and the
 * only thing that clears the map is an edit. So a `loading`, `starting` or
 * `failed` answer became permanent — a server that died and was restarted
 * mid-session (`session.rs` allows one restart a minute) left every row on screen
 * reading "Language server loading…" for the life of the tab while the new process
 * answered perfectly, recoverable only by typing a character. The anchors path
 * already refused to make exactly that claim; the counts path did not.
 *
 * Mutated in place. It is a cache belonging to one mounted editor, and a
 * copy-on-write store would be a new object per answer on the path that already
 * runs four requests at a time.
 */
export interface UsageAnswers {
  answers: Map<string, UsageAnswer>;
  /** Keys with a request out right now. */
  inFlight: Set<string>;
}

export function newUsageAnswers(): UsageAnswers {
  return { answers: new Map(), inFlight: new Set() };
}

/**
 * Whether this outcome is this version's final answer.
 *
 * `ready` is an answer. `notConfigured` and `unsupported` are settled facts about
 * the machine — no server is installed, or this one does not do references — and
 * re-asking is a poll for something that is not going to appear. The other three
 * are transient by definition: `starting` and `loading` become a different answer
 * on their own, and `failed` becomes one when the session restarts the server.
 */
function isSettled(outcome: Availability): boolean {
  return outcome === "ready" || outcome === "notConfigured" || outcome === "unsupported";
}

/**
 * File an answer, whatever it is.
 *
 * The result is always kept — a row that cannot say why it has no number is
 * indistinguishable from a bug — and only {@link shouldAskUsages} decides whether
 * to go back for a better one.
 */
export function recordUsageAnswer(store: UsageAnswers, key: string, result: UsageResult): void {
  const attempts = (store.answers.get(key)?.attempts ?? 0) + 1;
  store.answers.set(key, { result, attempts });
}

/** What the row above this anchor should be drawn from. */
export function usageStateFor(store: UsageAnswers, key: string): UsageRequestState {
  // In flight beats a previous answer: a retry that is out is the present tense,
  // and last time's reason is not.
  if (store.inFlight.has(key)) return { status: "pending" };
  const held = store.answers.get(key);
  return held ? { status: "answered", result: held.result } : { status: "idle" };
}

/**
 * Whether this anchor's count is worth asking for (again).
 *
 * No while a request is out, no once a settled answer is in, and no once the
 * retries for an unsettled one are spent.
 */
export function shouldAskUsages(store: UsageAnswers, key: string): boolean {
  if (store.inFlight.has(key)) return false;
  const held = store.answers.get(key);
  if (!held) return true;
  return !isSettled(held.result.outcome) && held.attempts < UNSETTLED_RETRY_LIMIT;
}

/**
 * Forget everything, for a new document version or a closing tab.
 *
 * The retry counts go too: a new version is a new question, and carrying a spent
 * budget across an edit would leave a row that can never be asked again.
 */
export function clearUsageAnswers(store: UsageAnswers): void {
  store.answers.clear();
  store.inFlight.clear();
}

// ---------------------------------------------------------------------------
// 6b. What the request queue keeps.
// ---------------------------------------------------------------------------

/** One queued references query: the anchor to ask about, and what it is for. */
export interface UsageJob {
  anchor: DeclarationAnchor;
  /** The {@link usageCacheKey} the answer will be filed under. */
  key: string;
  /** The document version that key is stamped with. */
  version: number;
}

/**
 * Drop the queued jobs that are no longer worth issuing, and say which.
 *
 * A references query is a workspace-wide search answered serially, so a FIFO queue
 * that is never pruned puts the rows the user is *looking at* behind questions
 * about methods they scrolled past minutes ago — and behind questions about a
 * document version that no longer exists, whose answers are discarded on arrival
 * anyway. Both are pure waste of the four concurrency slots, and the visible
 * symptom is a row that says "Finding usages…" for tens of seconds.
 *
 * The dropped keys are returned rather than merely forgotten: the caller holds
 * them in an in-flight set, and leaving one there would make that anchor
 * permanently unaskable for the rest of the document version — a row nothing will
 * ever answer.
 */
export function retainUsageJobs(
  jobs: UsageJob[],
  visibleAnchorIds: Set<string>,
  version: number,
): { keep: UsageJob[]; dropped: string[] } {
  const keep: UsageJob[] = [];
  const dropped: string[] = [];
  for (const job of jobs) {
    if (job.version === version && visibleAnchorIds.has(job.anchor.id)) keep.push(job);
    else dropped.push(job.key);
  }
  return { keep, dropped };
}

// ---------------------------------------------------------------------------
// 6c. Placing the overlay, counting its rows, and the strings around it.
// ---------------------------------------------------------------------------

/** Widest the usages dropdown is allowed to be, for clamping it into the pane. */
export const MENU_WIDTH = 560;

/** Where a dropdown goes, in coordinates relative to the positioned wrapper. */
export interface Placement {
  left: number;
  top: number;
  /** So a long list scrolls inside the pane instead of running off the window. */
  maxHeight: number;
}

/** The positioned wrapper's rectangle, or `null` before it has one. */
export interface MenuBounds {
  left: number;
  top: number;
  width: number;
  height: number;
}

/**
 * Turn a viewport point into a position inside the wrapper, kept in the pane.
 *
 * Clamped rather than flipped: the editor pane can be a couple of hundred pixels
 * tall, so there is often no side with room, and a menu that scrolls inside the
 * pane is more useful than one that flips onto equally little space. Every clamp
 * has a floor, because a pane narrower than the menu makes the natural arithmetic
 * negative and would place the menu off the left edge.
 */
export function placeMenu(
  x: number,
  y: number,
  bounds: MenuBounds | null,
  menuWidth: number = MENU_WIDTH,
): Placement {
  if (!bounds) return { left: 4, top: 4, maxHeight: 240 };
  const left = Math.max(4, Math.min(x - bounds.left, bounds.width - menuWidth - 4));
  const top = Math.max(4, Math.min(y - bounds.top + 4, Math.max(4, bounds.height - 80)));
  return { left, top, maxHeight: Math.max(80, bounds.height - top - 8) };
}

/**
 * How many rows the dropdown is actually showing, for the truncation line.
 *
 * The number the "showing the first N of M" sentence depends on, so it is measured
 * where a test can measure it too — the same reduce written inside the component is
 * a second opinion about a count whose whole job is to disagree with `total`.
 */
export function countUsageRows(groups: UsageGroup[]): number {
  return groups.reduce((sum, group) => sum + group.rows.length, 0);
}

/**
 * Why a listed location does nothing when it is clicked.
 *
 * Shown on hover over an unopenable row. It says where the location is and
 * deliberately never suggests the location is not real: it is counted in `total`,
 * and dropping it would contradict the number above it.
 */
export const INERT_LOCATION_REASON =
  "This location is outside the workspace or in a generated document, so it cannot be opened.";

// ---------------------------------------------------------------------------
// 6d. The two decisions the CodeMirror layer asks this module for.
// ---------------------------------------------------------------------------

/** `cb-usages-idle` … `cb-usages-reason`, from {@link UsageRowView.tone}. */
export function toneClass(tone: UsageTone): string {
  return `cb-usages-${tone}`;
}

/**
 * The part of a row's `action` that varies within a `kind`, as one comparable
 * string.
 *
 * Half of `UsageRowWidget.eq`, and therefore load-bearing: CodeMirror keeps the
 * **old** widget instance when `eq` returns true, so a comparison that misses the
 * count inside a `dropdown` action leaves a surviving widget handing the host a
 * stale number when its row is clicked. It lives here rather than beside the widget
 * because it is pure and `usagesExtension.ts` is outside the coverage glob — the
 * `nodeTargets.ts` mistake, avoided.
 */
export function actionDetail(view: UsageRowView): string {
  return view.action.kind === "dropdown"
    ? `d ${view.action.total}`
    : `i ${view.action.reason ?? ""}`;
}

// ---------------------------------------------------------------------------
// 7. Highlight slicing.
// ---------------------------------------------------------------------------

/** A snippet cut around its highlight, ready to render as three spans. */
export interface SnippetParts {
  before: string;
  match: string;
  after: string;
}

/**
 * Cut `snippet` into the text before the match, the match, and the text after.
 *
 * `Highlight.start`/`.end` are **UTF-16 code-unit** offsets, which is exactly
 * what `String.prototype.slice` counts in, so this is a slice and not a
 * conversion. Reading them as code points (`Array.from`) would shift the match
 * after any astral character and could cut a surrogate pair in half; the Rust
 * side holds the span in bytes and `lsp::positions::byte_to_utf16` is the only
 * thing that converts.
 *
 * Everything out of range **clamps** rather than throwing — the same lesson as
 * `searchLogic.lineToPos`. A snippet is trimmed context and an offset can
 * legitimately fall outside it, and a thrown exception inside a render is a blank
 * dropdown rather than a slightly wrong underline. A span that is inverted or not
 * a number yields an empty match: no underline is honest, and an underline over
 * the wrong characters is a claim.
 */
export function snippetParts(snippet: string, highlight: Highlight | null): SnippetParts {
  const whole: SnippetParts = { before: snippet, match: "", after: "" };
  if (!highlight) return whole;
  const { start, end } = highlight;
  if (!Number.isFinite(start) || !Number.isFinite(end)) return whole;

  const from = Math.min(Math.max(Math.trunc(start), 0), snippet.length);
  const to = Math.min(Math.max(Math.trunc(end), from), snippet.length);
  return {
    before: snippet.slice(0, from),
    match: snippet.slice(from, to),
    after: snippet.slice(to),
  };
}

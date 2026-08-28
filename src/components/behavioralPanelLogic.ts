import type {
  BehavioralDelta,
  BehavioralScorecard,
  BodyDelta,
  CardBehavior,
  CaseTransition,
  FileChange,
  RunConfig,
  TestDelta,
} from "../ipc/types";

/**
 * The runtime counterpart to the intent view's static colouring: whether one
 * behavioral outcome moved the code toward working, away from it, or somewhere
 * that carries no verdict on its own.
 *
 * A wrong colour is worse than a neutral one here too — only a transition that
 * plainly regressed (`regressed`/`stillFailing`) or plainly fixed (`fixed`) is
 * coloured; `added`/`removed`/`unchanged` stay neutral, because a case that
 * appeared or vanished is a fact to read, not a pass or a fail.
 */
export type Tone = "positive" | "warning" | "neutral";

/** The tone one test transition carries on its own. */
export function transitionTone(transition: CaseTransition): Tone {
  switch (transition) {
    case "regressed":
    case "stillFailing":
      return "warning";
    case "fixed":
      return "positive";
    default:
      return "neutral";
  }
}

/**
 * The tone a change in HTTP status code carries.
 *
 * A `2xx` is the only "good" band, so leaving it is a warning and entering it
 * is positive; within the same band a higher code (e.g. 500 vs 400) reads as
 * worse and a lower one as better; an unchanged code is neutral. Shared by the
 * per-card badge and the unattributed delta line so the two never disagree.
 */
export function httpStatusTone(before: number, after: number): Tone {
  const wasOk = before >= 200 && before < 300;
  const nowOk = after >= 200 && after < 300;
  if (wasOk && !nowOk) return "warning";
  if (!wasOk && nowOk) return "positive";
  if (after > before) return "warning";
  if (after < before) return "positive";
  return "neutral";
}

/**
 * The config the before/after action replays: the first test config, else the
 * first config of any kind, else nothing (the action is disabled). Extracted so
 * the choice is tested rather than decided inline in the view.
 */
export function pickBehavioralConfig(configs: RunConfig[]): RunConfig | null {
  return configs.find((c) => c.kind === "test") ?? configs[0] ?? null;
}

/**
 * Which `.http` files the before/after run should replay.
 *
 * `auto` lets the backend discover them itself; `explicit` names a specific set
 * the user toggled in the Evidence picker. The two are kept apart so the view
 * can offer a checklist of candidates without losing the default "just find
 * them" behaviour.
 */
export type HttpFileSelection =
  | { mode: "auto" }
  | { mode: "explicit"; files: string[] };

/**
 * The wire value for `behavioral_diff`'s `http_files` argument.
 *
 * `null` means "discover" — and that is what both `auto` and an *empty* explicit
 * list resolve to, because the backend treats `Some(empty)` identically to
 * `None` (see `cb_core::behavioral`). Otherwise the explicit list is normalised:
 * trimmed, blanks dropped, deduplicated in first-seen order. There is
 * deliberately no value that means "run no HTTP at all" — the wire argument
 * cannot express it, so this never invents one.
 */
export function resolveHttpFiles(selection: HttpFileSelection): string[] | null {
  if (selection.mode === "auto") return null;
  const seen = new Set<string>();
  const files: string[] = [];
  for (const raw of selection.files) {
    const path = raw.trim();
    if (path === "" || seen.has(path)) continue;
    seen.add(path);
    files.push(path);
  }
  return files.length > 0 ? files : null;
}

/**
 * The changed files that look like HTTP request collections — the candidates
 * the Evidence picker offers to replay explicitly.
 *
 * Limited to changed files (what `git status` reports) on purpose: unchanged
 * `.http` files elsewhere in the tree are not offered, since full discovery is
 * the backend's `auto` path, not this list.
 */
export function httpFileCandidates(files: FileChange[]): string[] {
  return files
    .map((file) => file.path)
    .filter((path) => {
      const lower = path.toLowerCase();
      return lower.endsWith(".http") || lower.endsWith(".rest");
    });
}

/** A short badge summarising one card's behavioral deltas, with a longer tooltip. */
export interface BehavioralBadge {
  label: string;
  tone: Tone;
  title: string;
}

/**
 * Summarise the observable difference one intent card produced.
 *
 * The label is a compact pill (e.g. `2 regressed · 1 fixed`); the title is the
 * spelled-out tooltip. Tone follows the abstain rule the whole feature keeps: a
 * card that regressed *anything* — a test or an HTTP status that got worse —
 * reads as a warning even when it also fixed something, because a regression is
 * the finding a reviewer must not miss. A card that only improves things is
 * positive; everything else (console noise, bodies, appeared/removed cases) is
 * neutral rather than guessed at.
 */
export function behavioralBadge(card: CardBehavior): BehavioralBadge {
  let fixed = 0;
  let regressed = 0;
  let stillFailing = 0;
  let otherTests = 0;
  let statusWorse = 0;
  let statusBetter = 0;
  let httpOther = 0;
  let consoleChanges = 0;

  for (const delta of card.deltas) {
    if (delta.kind === "test") {
      switch (delta.transition) {
        case "fixed":
          fixed += 1;
          break;
        case "regressed":
          regressed += 1;
          break;
        case "stillFailing":
          stillFailing += 1;
          break;
        default:
          otherTests += 1;
      }
    } else if (delta.kind === "http") {
      if (delta.status) {
        const [before, after] = delta.status;
        switch (httpStatusTone(before, after)) {
          case "warning":
            statusWorse += 1;
            break;
          case "positive":
            statusBetter += 1;
            break;
          default:
            httpOther += 1;
        }
      } else {
        httpOther += 1;
      }
    } else {
      consoleChanges += 1;
    }
  }

  const parts: string[] = [];
  if (regressed > 0) parts.push(`${regressed} regressed`);
  if (stillFailing > 0) parts.push(`${stillFailing} still failing`);
  if (fixed > 0) parts.push(`${fixed} fixed`);
  if (statusWorse > 0) parts.push(`${statusWorse} status worse`);
  if (statusBetter > 0) parts.push(`${statusBetter} status better`);
  const responses = httpOther;
  if (responses > 0) parts.push(`${responses} response${responses === 1 ? "" : "s"}`);
  if (consoleChanges > 0) parts.push("console");
  if (otherTests > 0) parts.push(`${otherTests} case${otherTests === 1 ? "" : "s"}`);

  const label = parts.length > 0 ? parts.join(" · ") : "no change";

  const hasRegression = regressed > 0 || stillFailing > 0 || statusWorse > 0;
  const hasImprovement = fixed > 0 || statusBetter > 0;
  const tone: Tone = hasRegression
    ? "warning"
    : hasImprovement
      ? "positive"
      : "neutral";

  return { label, tone, title: behavioralTitle(card.deltas, label) };
}

/** The spelled-out tooltip: each delta named, so hovering explains the pill. */
function behavioralTitle(deltas: BehavioralDelta[], label: string): string {
  if (deltas.length === 0) return "No observable difference for this card.";

  const lines = deltas.map((delta) => {
    if (delta.kind === "test") {
      return `${delta.transition}: ${delta.fullName}`;
    }
    if (delta.kind === "http") {
      const status = delta.status
        ? ` (${delta.status[0]} → ${delta.status[1]})`
        : "";
      return `${delta.name}${status}`;
    }
    return `console: ${delta.addedLines.length} added, ${delta.removedLines.length} removed`;
  });

  return `${label}\n${lines.join("\n")}`;
}

/**
 * The one-line runtime scorecard, twin of `intentPanelLogic.scorecardLine`.
 *
 * A reading like `3 outcomes compared · 2 deltas · 1 attributed · 1
 * unattributed · 1 abstained`. Reads sensibly at zero, so it can show even when
 * a run produced no observable difference at all.
 */
export function behavioralScoreLine(sc: BehavioralScorecard): string {
  return [
    `${sc.outcomesCompared} outcome${sc.outcomesCompared === 1 ? "" : "s"} compared`,
    `${sc.deltas} delta${sc.deltas === 1 ? "" : "s"}`,
    `${sc.attributedDeltas} attributed`,
    `${sc.unattributedDeltas} unattributed`,
    `${sc.abstained} abstained`,
  ].join(" · ");
}

/**
 * A one-line description of one unattributed delta, for the overall section.
 * These never pin to a card, so they are listed plainly with their own tone.
 */
export interface DeltaLine {
  text: string;
  tone: Tone;
}

/** Render one behavioral delta as a labelled line with a tone. */
export function deltaLine(delta: BehavioralDelta): DeltaLine {
  if (delta.kind === "test") {
    return {
      text: `${delta.transition}: ${delta.fullName}`,
      tone: transitionTone(delta.transition),
    };
  }
  if (delta.kind === "http") {
    if (delta.status) {
      const [before, after] = delta.status;
      return {
        text: `${delta.name}: ${before} → ${after}`,
        tone: httpStatusTone(before, after),
      };
    }
    return { text: `${delta.name}: response changed`, tone: "neutral" };
  }
  return {
    text: `console: ${delta.addedLines.length} added, ${delta.removedLines.length} removed`,
    tone: "neutral",
  };
}

/**
 * How many lines of one side of one delta are shown before the rest is folded
 * into a `+N more` row.
 *
 * A cap that silently truncated would be the very bug this evidence exists to
 * avoid, so the withheld count is always stated — the same habit as the
 * inspector's `Elided`.
 */
export const EVIDENCE_LINE_CAP = 20;

/**
 * One line of the evidence *under* a delta's summary row.
 *
 * `kind` is what the line is, not how it looks: `added`/`removed` are the two
 * sides of a text diff (rendered monospace and coloured), `note` is everything
 * that describes the comparison rather than being part of it — the masking
 * note, the `+N more` remainder, a status or header change.
 */
export interface DetailRow {
  text: string;
  tone: Tone;
  kind: "added" | "removed" | "note";
}

/** `null` renders as `absent`: a side that was not there is a fact, not a blank. */
function orAbsent(value: string | null | undefined): string {
  return value ?? "absent";
}

/**
 * The `-`/`+` rows for one pair of line lists, capped per side.
 *
 * Removals come first so a row reads the way a diff does. Each side is capped
 * independently, and a single trailing `+N more` states the total withheld
 * across both, because the reader cares how much they are not seeing, not which
 * half it came from.
 */
function diffRows(removed: string[], added: string[]): DetailRow[] {
  const rows: DetailRow[] = [];
  for (const line of removed.slice(0, EVIDENCE_LINE_CAP)) {
    rows.push({ text: `- ${line}`, tone: "neutral", kind: "removed" });
  }
  for (const line of added.slice(0, EVIDENCE_LINE_CAP)) {
    rows.push({ text: `+ ${line}`, tone: "neutral", kind: "added" });
  }
  const withheld =
    Math.max(0, removed.length - EVIDENCE_LINE_CAP) +
    Math.max(0, added.length - EVIDENCE_LINE_CAP);
  if (withheld > 0) {
    rows.push({ text: `+${withheld} more`, tone: "neutral", kind: "note" });
  }
  return rows;
}

/** The masking note for a body diff, only when masking actually happened. */
function bodyRows(body: BodyDelta): DetailRow[] {
  const rows = diffRows(body.removedLines, body.addedLines);
  if (body.normalized) {
    rows.push({
      text: "timestamps and ids were masked before comparing",
      tone: "neutral",
      kind: "note",
    });
  }
  return rows;
}

/**
 * The evidence behind one delta — the lines, headers and outcomes the summary
 * row only counted.
 *
 * Every field the backend carries is rendered here; nothing is dropped, because
 * dropping it is what made a finished run read as "console: 2 added, 2 removed"
 * and nothing else. A delta with no recorded detail says so rather than
 * rendering an empty list, so an expanded row is never blank.
 */
export function deltaDetail(delta: BehavioralDelta): DetailRow[] {
  if (delta.kind === "console") {
    const rows = diffRows(delta.removedLines, delta.addedLines);
    if (delta.normalized) {
      rows.push({
        text: "timestamps, ids, durations and both run roots were masked before comparing",
        tone: "neutral",
        kind: "note",
      });
    }
    return rows;
  }

  if (delta.kind === "http") {
    const rows: DetailRow[] = [];
    if (delta.status) {
      const [before, after] = delta.status;
      rows.push({
        text: `status ${before} → ${after}`,
        tone: httpStatusTone(before, after),
        kind: "note",
      });
    }
    for (const header of delta.headerChanges) {
      rows.push({
        text: `${header.name}: ${orAbsent(header.before)} → ${orAbsent(header.after)}`,
        tone: "neutral",
        kind: "note",
      });
    }
    if (delta.body) rows.push(...bodyRows(delta.body));
    if (rows.length === 0) {
      rows.push({
        text: "the response differed, but no status, header or body detail was recorded",
        tone: "neutral",
        kind: "note",
      });
    }
    return rows;
  }

  return [
    {
      text: `${orAbsent(delta.base)} → ${orAbsent(delta.work)}`,
      tone: transitionTone(delta.transition),
      kind: "note",
    },
  ];
}

/**
 * How much the run trusts one delta, or `null` when the delta carries no
 * confidence of its own.
 *
 * A test delta genuinely has none — its confidence is assigned during
 * attribution (capped at medium, a single run per side), so printing one here
 * would invent a number the backend never produced.
 */
export function deltaConfidenceNote(delta: BehavioralDelta): string | null {
  if (delta.kind === "test") return null;
  return `${delta.confidence} confidence`;
}

/**
 * Why one delta was pinned to no intent card.
 *
 * "0 attributed" reads as a failure when it is usually a *rule*: HTTP and test
 * deltas can never attribute (`behavioral/attribute.rs::candidate_paths` returns
 * no candidate files for HTTP by design, and `compare.rs` leaves every case's
 * `files_hint` empty), and a console delta attributes only when its changed
 * lines name the files of exactly one card. The console wording is deliberately
 * true of both the zero-owner and the ambiguous case, since the two are not
 * distinguishable from the report alone.
 */
export function unattributedReason(delta: BehavioralDelta): string {
  if (delta.kind === "test") {
    return "a test case is not mapped to a source file, so a test delta is never pinned to a card";
  }
  if (delta.kind === "http") {
    return "an .http request's handler is not derivable, so HTTP deltas are never pinned to a card";
  }
  return "no single intent card's files were named in these lines";
}

/**
 * The test cases whose outcome actually moved, under the pass/fail summary line.
 *
 * `TestDelta.cases` already omits `Unchanged`, so this is exactly what changed —
 * and an empty list is stated rather than rendered as an empty section, because
 * "186 passed → 186 passed" with nothing beneath it leaves the reader unsure
 * whether the rows are missing or there were none.
 */
export function testCaseRows(delta: TestDelta): DetailRow[] {
  if (delta.cases.length === 0) {
    return [{ text: "no test case changed outcome", tone: "neutral", kind: "note" }];
  }
  return delta.cases.map((c) => ({
    text: `${c.transition}: ${c.fullName} (${orAbsent(c.base)} → ${orAbsent(c.work)})`,
    tone: transitionTone(c.transition),
    kind: "note" as const,
  }));
}

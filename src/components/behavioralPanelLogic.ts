import type {
  BehavioralDelta,
  BehavioralScorecard,
  CardBehavior,
  CaseTransition,
  RunConfig,
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

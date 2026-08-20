import type {
  BehavioralDelta,
  BehavioralReport,
  RulesReport,
  RunConfig,
} from "../ipc/types";
import { behavioralScoreLine, pickBehavioralConfig } from "./behavioralPanelLogic";

/**
 * Decisions behind the "Verify claims" and "Verify rules" review actions.
 *
 * All pure, so the parts that matter — what evidence text an agent is handed,
 * whether an action is even runnable, and the guard shown before a rules review
 * — are pinned by tests rather than discovered in a live agent run. The
 * component that calls these only wires them to buttons.
 */

/**
 * A one-line, plain-text description of one behavioral delta, for the evidence
 * block. Deliberately terse and stable — it is read by an agent, not rendered,
 * so it carries no tone or markup, only the fact.
 */
function describeDelta(delta: BehavioralDelta): string {
  if (delta.kind === "test") {
    const from = delta.base ?? "absent";
    const to = delta.work ?? "absent";
    return `test ${delta.transition}: ${delta.fullName} (${from} → ${to})`;
  }
  if (delta.kind === "http") {
    const status = delta.status
      ? `${delta.status[0]} → ${delta.status[1]}`
      : "response changed";
    return `http ${delta.name}: ${status}`;
  }
  return `console: ${delta.addedLines.length} added, ${delta.removedLines.length} removed`;
}

/**
 * Render a {@link BehavioralReport} as a deterministic, human-readable evidence
 * block suitable to prepend to the verify-claims prompt (via
 * `startReview(..., context)`).
 *
 * The agent is meant to read this as **the evidence** and the diff as the
 * claims, so this is the observable before/after — the scorecard counts, the
 * test-summary shift, the deltas grouped as the backend attributed them, the
 * unattributed remainder, and every warning the run raised.
 *
 * Ordering follows the report's own arrays (which the backend already sorts),
 * so two calls on the same report produce byte-identical text. Empty sections
 * are omitted rather than printed as headers with nothing under them, and a
 * report with no deltas at all says so plainly instead of looking truncated.
 * Never throws: every field access is over the typed shape and guarded.
 */
export function behavioralReportToPromptContext(report: BehavioralReport): string {
  const lines: string[] = ["Behavioral before/after evidence", ""];

  lines.push(behavioralScoreLine(report.scorecard));

  if (report.tests) {
    const b = report.tests.summaryBefore;
    const a = report.tests.summaryAfter;
    lines.push(
      `Tests: before ${b.passed} passed / ${b.failed} failed / ${b.skipped} skipped ` +
        `(${b.total} total), after ${a.passed} passed / ${a.failed} failed / ` +
        `${a.skipped} skipped (${a.total} total)`,
    );
  }

  if (report.attributions.length > 0) {
    lines.push("", "Attributed deltas (by intent card):");
    for (const card of report.attributions) {
      lines.push(`- card ${card.groupId} (${card.confidence} confidence):`);
      if (card.deltas.length === 0) {
        lines.push("  - (no deltas)");
      } else {
        for (const delta of card.deltas) lines.push(`  - ${describeDelta(delta)}`);
      }
    }
  }

  if (report.unattributed.length > 0) {
    lines.push("", "Unattributed deltas (pinned to no card):");
    for (const delta of report.unattributed) lines.push(`- ${describeDelta(delta)}`);
  }

  if (report.warnings.length > 0) {
    lines.push("", "Warnings (evidence that could not be gathered):");
    for (const warning of report.warnings) lines.push(`- ${warning}`);
  }

  if (report.attributions.length === 0 && report.unattributed.length === 0) {
    lines.push("", "No observable before/after differences were detected.");
  }

  return lines.join("\n");
}

/** Whether the verify-claims action can run, the config it would replay, and why. */
export interface VerifyClaimsAction {
  enabled: boolean;
  /** The config whose outcomes are compared, or `null` when none resolves. */
  config: RunConfig | null;
  /** The button's tooltip — the reason it is disabled, or what it will do. */
  hint: string;
}

/**
 * Decide whether "Verify claims" is runnable and against which config.
 *
 * Reuses {@link pickBehavioralConfig} so the evidence gathered here replays the
 * same run the before/after panel does. With no config there is nothing to
 * replay, so the action is disabled with a reason rather than silently doing
 * nothing.
 */
export function verifyClaimsAction(configs: RunConfig[]): VerifyClaimsAction {
  const config = pickBehavioralConfig(configs);
  if (!config) {
    return {
      enabled: false,
      config: null,
      hint: "No run configuration is available to gather before/after evidence.",
    };
  }
  return {
    enabled: true,
    config,
    hint: `Same run as "Run before/after" ("${config.name}"), then a read-only agent checks whether the diff's changes are borne out by the results`,
  };
}

/**
 * The guard shown before running the verify-rules review, or `null` when there
 * is nothing to say.
 *
 * A rules review against an empty rules directory can only report "no rules",
 * so we say so up front and point at the fix. When some rules loaded but a file
 * would not read, the review is still worth running but the reader must know it
 * is working from an incomplete set — that is a note, not a block. The
 * no-rules case wins over the warning note: it is the more fundamental problem.
 */
export function rulesRunHint(report: RulesReport): string | null {
  if (report.rules.length === 0) {
    return "No business rules yet — run Extract Business Rules first.";
  }
  if (report.warnings.length > 0) {
    const n = report.warnings.length;
    return `${n} rule file${n === 1 ? "" : "s"} could not be read and will be skipped.`;
  }
  return null;
}

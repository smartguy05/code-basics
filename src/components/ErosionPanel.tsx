import { useState } from "react";
import { groupByCategory } from "./erosionLogic";
import type { ErosionFlag, ErosionReport } from "../ipc/types";

/**
 * The Changes tab's erosion view: a rules-based, no-model list of changes that
 * quietly weaken the codebase — deleted assertions, skipped tests, widened
 * catches, introduced panics, stubs left in production paths, removed
 * safeguards and logs.
 *
 * Every decision (how flags are grouped and ordered, the count on the toggle)
 * lives in `erosionLogic.ts`; this is a rendering shell. Clicking a flag opens
 * its file and highlights the offending line in the diff pane.
 */
export interface ErosionPanelProps {
  /** `null` before the first scan has returned. */
  report: ErosionReport | null;
  selectedPath: string | null;
  onOpenFlag: (flag: ErosionFlag) => void;
}

const ORIGIN_MARK: Record<ErosionFlag["origin"], string> = {
  addition: "+",
  deletion: "−",
  context: " ",
};

export function ErosionPanel({ report, selectedPath, onOpenFlag }: ErosionPanelProps) {
  // Which category sections are collapsed, by category name. Empty = all open.
  const [collapsed, setCollapsed] = useState<Set<string>>(new Set());
  const toggle = (category: string) =>
    setCollapsed((previous) => {
      const next = new Set(previous);
      if (next.has(category)) next.delete(category);
      else next.add(category);
      return next;
    });

  if (!report) {
    return (
      <div className="muted" style={{ padding: 8, fontSize: 12 }}>
        Scanning for erosion…
      </div>
    );
  }

  const sections = groupByCategory(report.flags);

  return (
    <>
      {report.warnings.map((warning) => (
        <div key={warning} className="warning" style={{ fontSize: 11 }}>
          {warning}
        </div>
      ))}

      {sections.length === 0 && (
        <div className="muted" style={{ padding: 8, fontSize: 12 }}>
          Nothing weakening detected in these changes.
        </div>
      )}

      {sections.map((section) => {
        const isCollapsed = collapsed.has(section.category);
        return (
        <div key={section.category}>
          <div
            className="group-label dropdown-section"
            style={{ display: "flex", alignItems: "center", gap: 4 }}
            onClick={() => toggle(section.category)}
          >
            <span className="twisty">{isCollapsed ? "▸" : "▾"}</span>
            <span style={{ flex: 1 }}>{section.label}</span>
            <span className="badge">{section.flags.length}</span>
          </div>

          {!isCollapsed && section.flags.map((flag) => (
            <button
              key={`${flag.path}:${flag.index}:${flag.ruleId}`}
              className={`row ${flag.path === selectedPath ? "selected" : ""}`}
              onClick={() => onOpenFlag(flag)}
              title={`${flag.message}\n${flag.path}:${flag.line} — click to show it in the diff`}
              style={{ display: "block", textAlign: "left" }}
            >
              <div style={{ display: "flex", gap: 4, alignItems: "baseline" }}>
                <span className={`status ${flag.origin}`}>{ORIGIN_MARK[flag.origin]}</span>
                <code style={{ flex: 1, overflow: "hidden", textOverflow: "ellipsis" }}>
                  {flag.content}
                </code>
              </div>
              <div className="faint" style={{ fontSize: 11 }}>
                {flag.path}:{flag.line}
              </div>
            </button>
          ))}
        </div>
        );
      })}
    </>
  );
}

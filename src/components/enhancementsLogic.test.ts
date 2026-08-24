import { describe, expect, it } from "vitest";
import {
  actionFor,
  actionTitle,
  confirmAddMessage,
  confirmRerunMessage,
  emptyMessage,
  emptyPromptsMessage,
  promptClickAction,
  relativeTime,
  runBadge,
  statusBadge,
} from "./enhancementsLogic";
import type { EnhancementInfo, PromptInfo, PromptRuns } from "../ipc/types";

const info = (installed: boolean): EnhancementInfo => ({
  id: "memory",
  title: "Memory Files",
  installed,
});

const prompt = (over: Partial<PromptInfo> = {}): PromptInfo => ({
  id: "setup",
  title: "Setup",
  once: false,
  body: "do it",
  ...over,
});

describe("actionFor", () => {
  it("adds when not installed, removes when installed", () => {
    expect(actionFor(info(false))).toBe("add");
    expect(actionFor(info(true))).toBe("remove");
  });
});

describe("statusBadge", () => {
  it("marks installed rows and nothing else", () => {
    expect(statusBadge(info(true))).toBe("added");
    expect(statusBadge(info(false))).toBeNull();
  });
});

describe("actionTitle", () => {
  it("names both target files and the direction of the click", () => {
    expect(actionTitle(info(false))).toContain("Add");
    expect(actionTitle(info(false))).toContain("CLAUDE.md and AGENTS.md");
    expect(actionTitle(info(true))).toContain("Remove");
  });
});

describe("emptyMessage", () => {
  it("only speaks when there is nothing to list", () => {
    expect(emptyMessage(0)).toContain(".md file");
    expect(emptyMessage(3)).toBeNull();
  });
});

describe("emptyPromptsMessage", () => {
  it("points at the prompts folder only when empty", () => {
    expect(emptyPromptsMessage(0)).toContain("prompts folder");
    expect(emptyPromptsMessage(2)).toBeNull();
  });
});

describe("confirmAddMessage", () => {
  it("names the template and both target files", () => {
    const msg = confirmAddMessage("Memory Files");
    expect(msg).toContain("Memory Files");
    expect(msg).toContain("CLAUDE.md and AGENTS.md");
  });
});

describe("promptClickAction", () => {
  const runs: PromptRuns = { setup: { lastRunAtMs: 1000 } };

  it("runs straight away when not run-once, whatever the record says", () => {
    expect(promptClickAction(prompt({ once: false }), runs)).toBe("run");
  });

  it("runs a run-once prompt that has no record yet", () => {
    expect(promptClickAction(prompt({ once: true }), {})).toBe("run");
  });

  it("confirms before re-running an already-run run-once prompt", () => {
    expect(promptClickAction(prompt({ once: true }), runs)).toBe("confirm-rerun");
  });
});

describe("runBadge", () => {
  const now = 10_000_000;

  it("badges only a run-once prompt that has a record", () => {
    expect(runBadge(prompt({ once: false }), { setup: { lastRunAtMs: 0 } }, now)).toBeNull();
    expect(runBadge(prompt({ once: true }), {}, now)).toBeNull();
    expect(runBadge(prompt({ once: true }), { setup: { lastRunAtMs: now } }, now)).toContain(
      "ran",
    );
  });
});

describe("confirmRerunMessage", () => {
  it("names the prompt", () => {
    expect(confirmRerunMessage("Build Graph")).toContain("Build Graph");
  });
});

describe("relativeTime", () => {
  const now = 1_000_000_000_000;
  it("scales the unit to the age and never goes to the future", () => {
    expect(relativeTime(now, now)).toBe("just now");
    expect(relativeTime(now + 5000, now)).toBe("just now"); // clock skew
    expect(relativeTime(now - 5 * 60_000, now)).toBe("5m ago");
    expect(relativeTime(now - 3 * 3_600_000, now)).toBe("3h ago");
    expect(relativeTime(now - 2 * 86_400_000, now)).toBe("2d ago");
    expect(relativeTime(now - 21 * 86_400_000, now)).toBe("3w ago");
  });
});

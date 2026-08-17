import { describe, expect, it } from "vitest";
import {
  actionFor,
  actionTitle,
  confirmAddMessage,
  copyFeedback,
  emptyMessage,
  emptyPromptsMessage,
  statusBadge,
} from "./enhancementsLogic";
import type { EnhancementInfo } from "../ipc/types";

const info = (installed: boolean): EnhancementInfo => ({
  id: "memory",
  title: "Memory Files",
  installed,
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

describe("copyFeedback", () => {
  it("names what was copied", () => {
    expect(copyFeedback("Code Review")).toBe('Copied "Code Review"');
  });
});

describe("confirmAddMessage", () => {
  it("names the template and both target files", () => {
    const msg = confirmAddMessage("Memory Files");
    expect(msg).toContain("Memory Files");
    expect(msg).toContain("CLAUDE.md and AGENTS.md");
  });
});

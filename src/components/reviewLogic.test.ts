import { describe, expect, it } from "vitest";
import {
  defaultAgentId,
  defaultModel,
  defaultPromptId,
  modelsFor,
  reviewStatus,
} from "./reviewLogic";
import type { PromptInfo, ReviewAgentInfo } from "../ipc/types";

const prompt = (id: string, title: string): PromptInfo => ({ id, title, body: "" });
const agent = (id: string, models: string[]): ReviewAgentInfo => ({ id, label: id, models });

describe("defaultAgentId", () => {
  it("takes the first agent (preference order)", () => {
    expect(defaultAgentId([agent("claude-code", ["opus"]), agent("codex", [])])).toBe("claude-code");
  });

  it("is undefined when no agent is installed", () => {
    expect(defaultAgentId([])).toBeUndefined();
  });
});

describe("modelsFor", () => {
  const agents = [agent("claude-code", ["opus", "sonnet"]), agent("codex", [])];

  it("returns the selected agent's models", () => {
    expect(modelsFor(agents, "claude-code")).toEqual(["opus", "sonnet"]);
  });

  it("returns empty for an agent with no models", () => {
    expect(modelsFor(agents, "codex")).toEqual([]);
  });

  it("returns empty for an unknown or undefined agent", () => {
    expect(modelsFor(agents, "cursor")).toEqual([]);
    expect(modelsFor(agents, undefined)).toEqual([]);
  });
});

describe("defaultModel", () => {
  it("prefers opus when offered", () => {
    expect(defaultModel(["haiku", "sonnet", "opus"])).toBe("opus");
  });

  it("falls back to the first when opus is absent", () => {
    expect(defaultModel(["sonnet", "haiku"])).toBe("sonnet");
  });

  it("is undefined for an empty list", () => {
    expect(defaultModel([])).toBeUndefined();
  });
});

describe("defaultPromptId", () => {
  it("prefers the exact code-review prompt", () => {
    const prompts = [prompt("write-tests", "Write tests"), prompt("code-review", "Code review")];
    expect(defaultPromptId(prompts)).toBe("code-review");
  });

  it("otherwise prefers a prompt whose id or title mentions review", () => {
    const prompts = [prompt("setup-docs", "Setup docs"), prompt("adversarial", "Adversarial Review")];
    expect(defaultPromptId(prompts)).toBe("adversarial");
  });

  it("falls back to the first prompt when none mention review", () => {
    const prompts = [prompt("setup-docs", "Setup docs"), prompt("write-tests", "Write tests")];
    expect(defaultPromptId(prompts)).toBe("setup-docs");
  });

  it("is undefined when there are no prompts", () => {
    expect(defaultPromptId([])).toBeUndefined();
  });
});

describe("reviewStatus", () => {
  it("reports idle before anything runs", () => {
    expect(reviewStatus("idle", null)).toBe("Idle");
  });

  it("reports running while in flight", () => {
    expect(reviewStatus("running", null)).toBe("Reviewing…");
  });

  it("reports a clean finish", () => {
    expect(reviewStatus("done", { type: "exited", code: 0, success: true, durationMs: 10, cancelled: false })).toBe(
      "Done",
    );
  });

  it("reports a cancellation distinctly from a failure", () => {
    expect(
      reviewStatus("done", { type: "exited", code: null, success: false, durationMs: 5, cancelled: true }),
    ).toBe("Cancelled");
  });

  it("reports a non-zero exit as a failure with its code", () => {
    const status = reviewStatus("done", {
      type: "exited",
      code: 2,
      success: false,
      durationMs: 5,
      cancelled: false,
    });
    expect(status).toContain("2");
    expect(status.toLowerCase()).toContain("exit");
  });

  it("reports a spawn failure", () => {
    expect(reviewStatus("done", { type: "failed", message: "program not found" })).toContain(
      "program not found",
    );
  });
});

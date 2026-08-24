import { describe, expect, it } from "vitest";
import {
  defaultAgentId,
  defaultModel,
  defaultPromptId,
  loadAgentPrefs,
  modelsFor,
  preferredAgentId,
  preferredModel,
  preferredPromptId,
  reviewStatus,
  saveAgentPrefs,
  type AgentPrefs,
} from "./reviewLogic";
import type { PromptInfo, ReviewAgentInfo } from "../ipc/types";

const prompt = (id: string, title: string): PromptInfo => ({ id, title, once: false, body: "" });
const agent = (id: string, models: string[]): ReviewAgentInfo => ({ id, label: id, models });

/** A minimal in-memory Storage stand-in for the persistence tests. */
function fakeStorage(seed?: string) {
  let value: string | null = seed ?? null;
  return {
    getItem: () => value,
    setItem: (_k: string, v: string) => {
      value = v;
    },
    read: () => value,
  };
}

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

describe("agent prefs persistence", () => {
  const agents = [agent("claude-code", ["opus", "sonnet"]), agent("codex", [])];
  const prompts = [prompt("code-review", "Code review"), prompt("setup", "Setup")];

  it("round-trips a saved selection", () => {
    const store = fakeStorage();
    const prefs: AgentPrefs = { agentId: "codex", model: "sonnet", promptId: "setup" };
    saveAgentPrefs(store, prefs);
    expect(loadAgentPrefs(store)).toEqual(prefs);
  });

  it("reads missing or garbage storage as empty", () => {
    expect(loadAgentPrefs(fakeStorage())).toEqual({});
    expect(loadAgentPrefs(fakeStorage("not json"))).toEqual({});
    expect(loadAgentPrefs(fakeStorage("[1,2,3]"))).toEqual({});
    // Wrong-typed fields are dropped, not trusted.
    expect(loadAgentPrefs(fakeStorage('{"agentId":42}'))).toEqual({
      agentId: undefined,
      model: undefined,
      promptId: undefined,
    });
  });

  it("prefers a remembered agent that is still installed, else the default", () => {
    expect(preferredAgentId({ agentId: "codex" }, agents)).toBe("codex");
    // Remembered agent gone (uninstalled) → default leads.
    expect(preferredAgentId({ agentId: "cursor" }, agents)).toBe("claude-code");
    expect(preferredAgentId({}, agents)).toBe("claude-code");
  });

  it("prefers a remembered model the agent still offers, else its default", () => {
    expect(preferredModel({ model: "sonnet" }, agents, "claude-code")).toBe("sonnet");
    // Remembered model not offered by this agent → default (opus).
    expect(preferredModel({ model: "o3" }, agents, "claude-code")).toBe("opus");
    // Agent with no models → undefined regardless of what was remembered.
    expect(preferredModel({ model: "sonnet" }, agents, "codex")).toBeUndefined();
  });

  it("lets an explicit initial prompt win, then a remembered one, then the default", () => {
    // Run Agent entry: initialPromptId is honoured over the remembered prompt.
    expect(preferredPromptId("setup", { promptId: "code-review" }, prompts)).toBe("setup");
    // Review entry (no initial): remembered prompt if it still exists.
    expect(preferredPromptId(undefined, { promptId: "setup" }, prompts)).toBe("setup");
    // Remembered prompt gone → canonical default.
    expect(preferredPromptId(undefined, { promptId: "vanished" }, prompts)).toBe("code-review");
    // An initial id that no longer exists is ignored, falling through.
    expect(preferredPromptId("vanished", { promptId: "setup" }, prompts)).toBe("setup");
  });
});

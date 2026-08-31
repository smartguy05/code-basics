import { describe, it, expect } from "vitest";
import {
  needsSetup,
  isDismissed,
  setDismissed,
  shouldPrompt,
  dismissKey,
} from "./setupPromptLogic";
import type { ProviderStatus } from "../ipc/types";

function provider(over: Partial<ProviderStatus>): ProviderStatus {
  return {
    provider: "claudeCode",
    detected: true,
    capture: null,
    sessions: 0,
    ...over,
  };
}

// A minimal in-memory Storage for the node test environment.
function memStorage(seed: Record<string, string> = {}): Storage {
  const map = new Map(Object.entries(seed));
  return {
    getItem: (k) => map.get(k) ?? null,
    setItem: (k, v) => void map.set(k, String(v)),
    removeItem: (k) => void map.delete(k),
    clear: () => map.clear(),
    key: (i) => [...map.keys()][i] ?? null,
    get length() {
      return map.size;
    },
  } as Storage;
}

describe("needsSetup", () => {
  it("prompts when the gate is missing, even if intent is on", () => {
    expect(needsSetup([provider({ capture: "project" })], null)).toBe(true);
  });

  it("prompts when a detected agent has no capture, even if the gate is on", () => {
    expect(needsSetup([provider({ capture: null })], "project")).toBe(true);
  });

  it("does not prompt when both are installed at project scope", () => {
    expect(needsSetup([provider({ capture: "project" })], "project")).toBe(false);
  });

  it("still prompts when capture is only at user (global) scope", () => {
    // A developer with the hooks installed globally must still be prompted to
    // set up the per-project, team-shared intent capture for this repo.
    expect(needsSetup([provider({ capture: "user" })], "project")).toBe(true);
    expect(needsSetup([provider({ capture: "user" })], "user")).toBe(true);
  });

  it("prompts for a gate-less repo with no agent detected", () => {
    expect(needsSetup([provider({ detected: false })], null)).toBe(true);
  });

  it("does not prompt when no agent is detected but the gate is installed", () => {
    expect(needsSetup([provider({ detected: false })], "project")).toBe(false);
  });

  it("ignores capture on an undetected agent", () => {
    // capture set but not detected ⇒ still counts as intent-not-installed.
    expect(needsSetup([provider({ detected: false, capture: "project" })], "project")).toBe(
      false,
    );
    expect(needsSetup([provider({ detected: false, capture: "project" })], null)).toBe(true);
  });
});

describe("dismissal", () => {
  const root = "C:/repo";

  it("round-trips a per-workspace dismissal", () => {
    const s = memStorage();
    expect(isDismissed(s, root)).toBe(false);
    setDismissed(s, root);
    expect(isDismissed(s, root)).toBe(true);
    expect(s.getItem(dismissKey(root))).toBe("1");
  });

  it("is scoped per workspace", () => {
    const s = memStorage();
    setDismissed(s, root);
    expect(isDismissed(s, "D:/other")).toBe(false);
  });
});

describe("shouldPrompt", () => {
  const needs = [provider({ capture: null })];
  const root = "C:/repo";

  it("is true when setup is needed and not dismissed", () => {
    expect(shouldPrompt(needs, null, memStorage(), root)).toBe(true);
  });

  it("is false once dismissed", () => {
    const s = memStorage();
    setDismissed(s, root);
    expect(shouldPrompt(needs, null, s, root)).toBe(false);
  });

  it("is false when nothing needs setup", () => {
    expect(shouldPrompt([provider({ capture: "project" })], "project", memStorage(), root)).toBe(
      false,
    );
  });
});

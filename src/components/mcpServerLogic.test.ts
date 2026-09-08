import { describe, expect, it } from "vitest";
import {
  MCP_PROVIDERS,
  confirmLabel,
  defaultScope,
  planOutcome,
  providerLabel,
  scopeAvailable,
  scopeOptions,
  statusText,
  writtenNote,
} from "./mcpServerLogic";
import type { InstallPlan } from "../ipc/types";

const plan = (writes: number): InstallPlan => ({
  provider: "claudeCode",
  scope: "project",
  writes: Array.from({ length: writes }, (_, i) => ({
    path: `file-${i}.json`,
    content: "{}",
    mergesExisting: true,
  })),
  caveats: [],
});

describe("scopeOptions", () => {
  it("offers both scopes for Claude Code", () => {
    expect(scopeOptions("claudeCode").filter((o) => o.available).map((o) => o.scope)).toEqual([
      "project",
      "user",
    ]);
  });

  it("shows Codex's project scope as unavailable rather than hiding it", () => {
    // Codex reads MCP servers only from $CODEX_HOME/config.toml. Hiding the row
    // leaves someone hunting for an option that does not exist; showing it with
    // the reason is the backend's own refusal, said before it is reached.
    const project = scopeOptions("codex").find((o) => o.scope === "project");
    expect(project?.available).toBe(false);
    expect(project?.detail).toContain("no per-repository");
  });

  it("always gives every scope something to read", () => {
    for (const provider of MCP_PROVIDERS) {
      for (const option of scopeOptions(provider)) {
        expect(option.detail.length).toBeGreaterThan(0);
        expect(option.label.length).toBeGreaterThan(0);
      }
    }
  });

  it("names the file each scope writes, so the grant is visible before it is made", () => {
    const claude = scopeOptions("claudeCode");
    expect(claude.find((o) => o.scope === "project")?.detail).toContain(".mcp.json");
    expect(claude.find((o) => o.scope === "user")?.detail).toContain("~/.claude.json");
    expect(scopeOptions("codex").find((o) => o.scope === "user")?.detail).toContain(
      "config.toml",
    );
  });
});

describe("defaultScope", () => {
  it("preselects the narrower grant for Claude Code", () => {
    // Per repository, not everywhere: this switch gives an agent read access to
    // a database, and the default must be the smaller of the two.
    expect(defaultScope("claudeCode")).toBe("project");
  });

  it("never preselects a scope the provider refuses", () => {
    for (const provider of MCP_PROVIDERS) {
      expect(scopeAvailable(provider, defaultScope(provider))).toBe(true);
    }
  });
});

describe("statusText", () => {
  it("says plainly when nothing is installed", () => {
    expect(statusText("claudeCode", null)).toBe("Not installed for Claude Code.");
  });

  it("keeps the two grants apart", () => {
    // "Installed" alone would hide which grant somebody actually made.
    const project = statusText("codex", "project");
    const user = statusText("codex", "user");
    expect(project).not.toBe(user);
    expect(project).toContain("this repository");
    expect(user).toContain("every repository");
  });

  it("names the provider it is talking about", () => {
    for (const provider of MCP_PROVIDERS) {
      expect(statusText(provider, "user")).toContain(providerLabel(provider));
    }
  });
});

describe("confirmLabel", () => {
  it("says which direction the button goes", () => {
    expect(confirmLabel("install")).not.toBe(confirmLabel("remove"));
    expect(confirmLabel("remove").toLowerCase()).toContain("remove");
  });
});

describe("planOutcome", () => {
  it("renders a zero-write removal as a sentence, not a confirmation", () => {
    // The rule this module exists for: there is nothing of ours in that file, so
    // there is nothing to confirm. A disabled Remove button is indistinguishable
    // from one that is busy or broken; this is an answer.
    const outcome = planOutcome(plan(0), "remove");
    expect(outcome.kind).toBe("nothing");
    if (outcome.kind === "nothing") expect(outcome.message).toContain("Nothing to remove");
  });

  it("confirms a removal that would actually rewrite the file", () => {
    const outcome = planOutcome(plan(1), "remove");
    expect(outcome.kind).toBe("confirm");
  });

  it("confirms an install", () => {
    expect(planOutcome(plan(1), "install").kind).toBe("confirm");
  });

  it("reports a zero-write install as the anomaly it would be", () => {
    // An install always writes the entry, so this cannot happen — and if it
    // does, saying "installed" would be a wrong answer shown confidently.
    const outcome = planOutcome(plan(0), "install");
    expect(outcome.kind).toBe("nothing");
    if (outcome.kind === "nothing") expect(outcome.message).not.toContain("Nothing to remove");
  });
});

describe("writtenNote", () => {
  // The bug this exists for: `confirm()` cleared the preview and updated the
  // status line, and that was the whole of the feedback. A preview vanishing is
  // indistinguishable from a cancel, so a successful write read as "nothing
  // happened" — which is the one outcome a user must never have to guess at,
  // because the alternative is installing twice.

  it("says what was written and where, for an install", () => {
    const note = writtenNote("install", "claudeCode", "project", [".mcp.json"]);
    expect(note).toContain("Installed");
    expect(note).toContain(".mcp.json");
  });

  it("names every file it wrote, not just the first", () => {
    const note = writtenNote("install", "claudeCode", "user", ["a.json", "b.json"]);
    expect(note).toContain("a.json");
    expect(note).toContain("b.json");
  });

  it("says removed rather than installed for a removal", () => {
    const note = writtenNote("remove", "codex", "user", ["config.toml"]);
    expect(note).toContain("Removed");
    expect(note).not.toContain("Installed");
  });

  it("tells a project install that the agent still has to approve it", () => {
    // Correct-but-inert is the failure mode people waste time on: the file is
    // right and the server does not load until Claude Code prompts.
    const note = writtenNote("install", "claudeCode", "project", [".mcp.json"]);
    expect(note.toLowerCase()).toContain("approve");
  });

  it("does not claim an approval step for a user-scope install", () => {
    const note = writtenNote("install", "claudeCode", "user", ["claude.json"]);
    expect(note.toLowerCase()).not.toContain("approve");
  });

  it("does not claim an approval step for a removal", () => {
    const note = writtenNote("remove", "claudeCode", "project", [".mcp.json"]);
    expect(note.toLowerCase()).not.toContain("approve");
  });

  it("says a restart is needed, because a running agent has already read the file", () => {
    const note = writtenNote("install", "codex", "user", ["config.toml"]);
    expect(note.toLowerCase()).toMatch(/restart|running/);
  });
});

import { describe, expect, it } from "vitest";

import type { ReviewAgentInfo } from "../ipc/types";
import type { ShortcutEvent } from "./searchLogic";
import {
  ASK_TITLE_MAX,
  askProgram,
  canAsk,
  launchBlockedReason,
  recogniseAskShortcut,
  shouldAbstainForFocus,
  terminalTitle,
} from "./askLogic";
import type { FocusedSurface } from "./askLogic";

/** A keydown with nothing held; each test overrides only what it is about. */
function key(over: Partial<ShortcutEvent> = {}): ShortcutEvent {
  return {
    key: "/",
    ctrlKey: false,
    shiftKey: false,
    altKey: false,
    metaKey: false,
    ...over,
  };
}

/**
 * A real `ReviewAgentInfo`, not a lookalike. `launchBlockedReason` is typed
 * structurally so `askLogic.ts` imports nothing from the IPC layer; passing the
 * genuine wire type here is what proves the two shapes still agree.
 */
function agent(id: string, label: string): ReviewAgentInfo {
  return { id, label, models: [] };
}

const CLAUDE = agent("claude-code", "Claude Code");
const CODEX = agent("codex", "Codex");

/**
 * A focused element as `AskPanel` describes it: the tag, whether it is
 * contenteditable, and which of the surface selectors it sits inside.
 */
function focus(over: Partial<FocusedSurface> = {}): FocusedSurface {
  return { tagName: "div", contentEditable: false, ancestors: [], ...over };
}

describe("shouldAbstainForFocus", () => {
  it("abstains inside a CodeMirror editor — Ctrl+/ is its comment toggle", () => {
    expect(shouldAbstainForFocus(focus({ ancestors: [".cm-editor"] }))).toBe(true);
  });

  it("abstains inside a terminal — Ctrl+/ is Ctrl+_ , readline's undo", () => {
    // xterm focuses a hidden textarea inside `.xterm`; both facts on their own
    // are enough, because either could change without the other.
    expect(
      shouldAbstainForFocus(focus({ tagName: "textarea", ancestors: [".xterm"] })),
    ).toBe(true);
    expect(shouldAbstainForFocus(focus({ ancestors: [".xterm"] }))).toBe(true);
  });

  it("abstains in a plain input or textarea", () => {
    expect(shouldAbstainForFocus(focus({ tagName: "input" }))).toBe(true);
    expect(shouldAbstainForFocus(focus({ tagName: "textarea" }))).toBe(true);
    // The tag arrives from `Element.tagName`, which is upper case in HTML.
    expect(shouldAbstainForFocus(focus({ tagName: "INPUT" }))).toBe(true);
  });

  it("abstains in a contenteditable region", () => {
    expect(shouldAbstainForFocus(focus({ contentEditable: true }))).toBe(true);
  });

  it("does NOT abstain when nothing is focused — that is the shortcut's own case", () => {
    expect(shouldAbstainForFocus(null)).toBe(false);
  });

  it("does NOT abstain on an ordinary focused element", () => {
    expect(shouldAbstainForFocus(focus({ tagName: "button" }))).toBe(false);
    expect(shouldAbstainForFocus(focus({ tagName: "div" }))).toBe(false);
  });

  it("ignores an ancestor selector it was not asked about", () => {
    // The caller only ever reports the selectors this module named; an unknown
    // one must not abstain, or a future caller could silently disable Ctrl+/.
    expect(shouldAbstainForFocus(focus({ ancestors: [".launcher-overlay"] }))).toBe(false);
  });
});

describe("recogniseAskShortcut", () => {
  it("recognises Ctrl+/", () => {
    expect(recogniseAskShortcut(key({ ctrlKey: true }))).toBe(true);
  });

  it("recognises Cmd+/ for mac keyboards", () => {
    expect(recogniseAskShortcut(key({ metaKey: true }))).toBe(true);
  });

  it("ignores a bare slash — that is just typing", () => {
    expect(recogniseAskShortcut(key())).toBe(false);
  });

  it("ignores any other key with Ctrl held", () => {
    expect(recogniseAskShortcut(key({ key: "n", ctrlKey: true }))).toBe(false);
    expect(recogniseAskShortcut(key({ key: "?", ctrlKey: true }))).toBe(false);
  });

  it("abstains when Alt is held — that is a different chord", () => {
    expect(recogniseAskShortcut(key({ ctrlKey: true, altKey: true }))).toBe(false);
    expect(recogniseAskShortcut(key({ metaKey: true, altKey: true }))).toBe(false);
  });

  it("abstains when Shift is held", () => {
    expect(recogniseAskShortcut(key({ ctrlKey: true, shiftKey: true }))).toBe(false);
  });

  it("abstains when both Ctrl and Cmd are held", () => {
    expect(recogniseAskShortcut(key({ ctrlKey: true, metaKey: true }))).toBe(false);
  });

  it("abstains when neither Ctrl nor Cmd is held", () => {
    expect(recogniseAskShortcut(key({ shiftKey: true }))).toBe(false);
  });
});

describe("canAsk", () => {
  it("accepts a question with content", () => {
    expect(canAsk("why does the supervisor leak?")).toBe(true);
  });

  it("rejects an empty question", () => {
    expect(canAsk("")).toBe(false);
  });

  it("rejects whitespace only", () => {
    expect(canAsk("   \n\t  ")).toBe(false);
  });

  it("accepts a question that only needs trimming", () => {
    expect(canAsk("  hi  ")).toBe(true);
  });
});

describe("askProgram", () => {
  it("names the program each known agent id is spawned as", () => {
    expect(askProgram("claude-code")).toBe("claude");
    expect(askProgram("codex")).toBe("codex");
  });

  it("abstains on an id this build does not know", () => {
    // Notably the camelCase `ProviderId` spelling, which is a different union.
    expect(askProgram("claudeCode")).toBeNull();
    expect(askProgram("")).toBeNull();
  });
});

describe("launchBlockedReason", () => {
  it("returns null when the chosen agent is installed", () => {
    expect(launchBlockedReason([CLAUDE, CODEX], "claude-code")).toBeNull();
    expect(launchBlockedReason([CLAUDE, CODEX], "codex")).toBeNull();
  });

  it("reports an empty agent list before anything else", () => {
    const none = launchBlockedReason([], "claude-code");
    expect(none).not.toBeNull();
    expect(none).toContain("No coding agent");
    // Still the empty-list answer even with nothing chosen: that is the reason
    // there is nothing to choose.
    expect(launchBlockedReason([], undefined)).toBe(none);
  });

  it("reports a missing choice distinctly from a missing agent", () => {
    const unchosen = launchBlockedReason([CLAUDE, CODEX], undefined);
    const uninstalled = launchBlockedReason([CODEX], "claude-code");
    expect(unchosen).not.toBeNull();
    expect(unchosen).toContain("Choose an agent");
    expect(uninstalled).not.toBe(unchosen);
  });

  it("treats null and an empty id as no choice", () => {
    const unchosen = launchBlockedReason([CLAUDE], undefined);
    expect(launchBlockedReason([CLAUDE], null)).toBe(unchosen);
    expect(launchBlockedReason([CLAUDE], "   ")).toBe(unchosen);
  });

  it("names the program it looked for when a known agent is missing", () => {
    const claude = launchBlockedReason([CODEX], "claude-code");
    expect(claude).toContain("Claude Code");
    expect(claude).toContain("claude");
    expect(claude).toContain("PATH");

    const codex = launchBlockedReason([CLAUDE], "codex");
    expect(codex).toContain("Codex");
    expect(codex).toContain("codex");
    expect(codex).toContain("PATH");

    // Two different agents must not produce the same sentence.
    expect(claude).not.toBe(codex);
  });

  it("abstains rather than inventing a program for an unknown id", () => {
    const unknown = launchBlockedReason([CLAUDE], "claudeCode");
    expect(unknown).not.toBeNull();
    expect(unknown).toContain("claudeCode");
    // It must not claim to have looked for anything on PATH — it has no idea
    // what this id would be spawned as.
    expect(unknown).not.toContain("PATH");
    expect(unknown).not.toBe(launchBlockedReason([CODEX], "claude-code"));
  });
});

describe("terminalTitle", () => {
  it("uses a short question as-is", () => {
    expect(terminalTitle("why is the LSP slow?")).toBe("why is the LSP slow?");
  });

  it("collapses runs of whitespace and trims the ends", () => {
    expect(terminalTitle("  why   is\tthe\nLSP slow?  ")).toBe("why is the LSP slow?");
  });

  it("never returns empty", () => {
    expect(terminalTitle("")).toBe("Ask");
    expect(terminalTitle("   \n ")).toBe("Ask");
  });

  it("keeps a question of exactly the cap intact", () => {
    const exact = "a".repeat(ASK_TITLE_MAX);
    expect(terminalTitle(exact)).toBe(exact);
    expect(terminalTitle(exact)).not.toContain("…");
  });

  it("ellipsises a long question without exceeding the cap", () => {
    const long = "a".repeat(ASK_TITLE_MAX + 20);
    const title = terminalTitle(long);
    expect(Array.from(title)).toHaveLength(ASK_TITLE_MAX);
    expect(title.endsWith("…")).toBe(true);
  });

  it("does not leave a dangling space before the ellipsis", () => {
    const long = `${"a".repeat(ASK_TITLE_MAX - 2)} bbbbbbbbbb`;
    const title = terminalTitle(long);
    expect(title).toBe(`${"a".repeat(ASK_TITLE_MAX - 2)}…`);
  });

  it("cuts by code points, never by UTF-16 units", () => {
    const long = "🙂".repeat(ASK_TITLE_MAX + 5);
    const title = terminalTitle(long);
    const chars = Array.from(title);
    expect(chars).toHaveLength(ASK_TITLE_MAX);
    expect(chars.at(-1)).toBe("…");
    // A halved surrogate pair renders as U+FFFD; none may appear.
    expect(title).not.toContain("�");
    expect(chars.slice(0, -1).every((c) => c === "🙂")).toBe(true);
  });
});

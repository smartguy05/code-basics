import { describe, expect, it } from "vitest";
import type { StashEntry } from "../ipc/types";
import { stashSummary } from "./stashLogic";

function entry(over: Partial<StashEntry>): StashEntry {
  return {
    index: 0,
    id: "abc",
    message: "On main: refactor",
    branch: "main",
    time: 0,
    ...over,
  };
}

describe("stashSummary", () => {
  it("strips the branch prefix git wrote, leaving the human message", () => {
    expect(stashSummary(entry({ message: "On main: refactor the parser" }))).toBe(
      "refactor the parser",
    );
    expect(
      stashSummary(entry({ message: "WIP on feature: 0abc123 subject", branch: "feature" })),
    ).toBe("0abc123 subject");
  });

  it("shows the message verbatim when there is no recognised prefix", () => {
    expect(stashSummary(entry({ message: "hand edited", branch: null }))).toBe("hand edited");
  });

  it("falls back to the whole message when stripping would leave nothing", () => {
    expect(stashSummary(entry({ message: "On main: ", branch: "main" }))).toBe("On main: ");
  });
});

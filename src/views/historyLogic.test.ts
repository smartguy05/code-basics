import { describe, expect, it, vi } from "vitest";
import {
  bulkDeleteBranches,
  bulkDeleteMessage,
  formatTime,
  intentForLine,
  whyCaption,
  whyTooltip,
} from "./historyLogic";
import type { LineIntent } from "../ipc/types";

function lineIntent(over: Partial<LineIntent> & { line: number }): LineIntent {
  return { turnId: "t", confidence: "medium", ...over };
}

describe("intentForLine", () => {
  const intents = [
    lineIntent({ line: 2, label: "add retry" }),
    lineIntent({ line: 5, label: "fix null guard" }),
  ];

  it("returns the intent recorded for a line", () => {
    expect(intentForLine(intents, 2)?.label).toBe("add retry");
    expect(intentForLine(intents, 5)?.label).toBe("fix null guard");
  });

  it("returns null for a line with no recorded reason", () => {
    expect(intentForLine(intents, 3)).toBeNull();
  });

  it("returns null when there is no caret line", () => {
    expect(intentForLine(intents, null)).toBeNull();
  });
});

describe("whyTooltip", () => {
  it("is null when there is no intent for the line", () => {
    expect(whyTooltip(null)).toBeNull();
  });

  it("shows the label and that it was stated by the agent", () => {
    const text = whyTooltip(lineIntent({ line: 1, label: "add retry", labelSource: "declared" }));
    expect(text).toContain("add retry");
    expect(text).toMatch(/stated by the agent/i);
  });

  it("marks an inferred label as inferred", () => {
    const text = whyTooltip(lineIntent({ line: 1, label: "did a thing", labelSource: "inferred" }));
    expect(text).toMatch(/inferred/i);
  });

  it("includes the user prompt when one was captured", () => {
    const text = whyTooltip(
      lineIntent({ line: 1, label: "add retry", prompt: "add backoff, cap at 5" }),
    );
    expect(text).toContain("Prompt: add backoff, cap at 5");
  });

  it("omits the prompt line when none was captured", () => {
    const text = whyTooltip(lineIntent({ line: 1, label: "add retry" })) ?? "";
    expect(text).not.toMatch(/Prompt:/);
  });
});

describe("whyCaption", () => {
  it("is null when nothing resolved", () => {
    expect(whyCaption([])).toBeNull();
  });

  it("uses the singular for one line", () => {
    expect(whyCaption([lineIntent({ line: 1 })])).toBe("1 line carries a recorded reason");
  });

  it("uses the plural for several", () => {
    expect(whyCaption([lineIntent({ line: 1 }), lineIntent({ line: 2 })])).toContain("2 lines");
  });
});

describe("formatTime", () => {
  it("interprets the value as Unix seconds, not milliseconds", () => {
    expect(formatTime(1_700_000_000)).toBe(
      new Date(1_700_000_000_000).toLocaleString(),
    );
  });

  it("renders the epoch itself", () => {
    expect(formatTime(0)).toBe(new Date(0).toLocaleString());
  });

  it("renders a pre-epoch (negative) timestamp", () => {
    expect(formatTime(-86_400)).toBe(new Date(-86_400_000).toLocaleString());
  });
});

describe("bulkDeleteBranches", () => {
  const describe_ = (e: unknown) => (e instanceof Error ? e.message : String(e));

  it("deletes sequentially — never two in flight at once", async () => {
    let inFlight = 0;
    let maxInFlight = 0;
    const deleteBranch = async () => {
      inFlight += 1;
      maxInFlight = Math.max(maxInFlight, inFlight);
      // Yield so an overlapping call would be observed if they raced.
      await Promise.resolve();
      await Promise.resolve();
      inFlight -= 1;
    };
    await bulkDeleteBranches(["a", "b", "c"], deleteBranch, describe_);
    expect(maxInFlight).toBe(1);
  });

  it("attempts every branch in order and returns none when all succeed", async () => {
    const seen: string[] = [];
    const failed = await bulkDeleteBranches(
      ["a", "b", "c"],
      async (name) => {
        seen.push(name);
      },
      describe_,
    );
    expect(seen).toEqual(["a", "b", "c"]);
    expect(failed).toEqual([]);
  });

  it("records a failure and keeps going with the rest", async () => {
    const seen: string[] = [];
    const failed = await bulkDeleteBranches(
      ["a", "b", "c"],
      async (name) => {
        seen.push(name);
        if (name === "b") throw new Error("not fully merged");
      },
      describe_,
    );
    // "c" was still attempted after "b" threw.
    expect(seen).toEqual(["a", "b", "c"]);
    expect(failed).toEqual([{ name: "b", error: "not fully merged" }]);
  });

  it("does nothing for an empty list", async () => {
    const deleteBranch = vi.fn();
    const failed = await bulkDeleteBranches([], deleteBranch, describe_);
    expect(deleteBranch).not.toHaveBeenCalled();
    expect(failed).toEqual([]);
  });
});

describe("bulkDeleteMessage", () => {
  it("says nothing when every deletion succeeded", () => {
    expect(bulkDeleteMessage([], 3)).toBeNull();
  });

  it("reports partial success with per-branch reasons", () => {
    const msg = bulkDeleteMessage(
      [{ name: "Releases/S20", error: "not fully merged" }],
      3,
    );
    expect(msg).toBe(
      "Deleted 2 of 3 branches. 1 could not be deleted:\n" +
        "Releases/S20: not fully merged",
    );
  });

  it("reports total failure without claiming any were deleted", () => {
    const msg = bulkDeleteMessage(
      [
        { name: "a", error: "not fully merged" },
        { name: "b", error: "locked" },
      ],
      2,
    );
    expect(msg).toBe(
      "Could not delete any of the 2 branches:\na: not fully merged\nb: locked",
    );
  });

  it("uses singular wording for a single total failure", () => {
    const msg = bulkDeleteMessage([{ name: "a", error: "not fully merged" }], 1);
    expect(msg).toBe("Could not delete the branch:\na: not fully merged");
  });
});

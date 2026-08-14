import { describe, expect, it } from "vitest";
import { formatTime } from "./historyLogic";

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

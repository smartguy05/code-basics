/// <reference types="vite/client" />
import { describe, expect, it } from "vitest";

// Load the two source files' text at build time. This repo ships no
// @types/node, so we cannot import from node:fs — Vite's `?raw` query gives us
// the file contents as a string with no node builtins involved.
const sources = import.meta.glob(
  ["./views/InspectView.tsx", "./components/OutputConsole.tsx"],
  { query: "?raw", import: "default", eager: true },
) as Record<string, string>;

describe("reexport guards", () => {
  it("compatibility re-exports are dropped", () => {
    const inspect = sources["./views/InspectView.tsx"];
    const console = sources["./components/OutputConsole.tsx"];

    expect(inspect).toBeTruthy();
    expect(console).toBeTruthy();

    expect(inspect).not.toContain("export { preferApplicationProcess }");
    expect(console).not.toContain("export { stripAnsi }");
  });
});

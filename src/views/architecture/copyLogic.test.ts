import { describe, expect, it } from "vitest";
import { copyDiagramName } from "./copyLogic";

const existing = (...names: string[]) => names.map((name) => ({ name }));

describe("copyDiagramName", () => {
  it("appends the extension the store will file it under", () => {
    expect(copyDiagramName("project-map", [])).toEqual({
      ok: true,
      name: "project-map.md",
    });
  });

  it("leaves an extension that is already there, whatever its case", () => {
    // `store::file_name` matches the extension case-insensitively, so appending
    // here would produce `Map.MD.md` — a second file under a name the user did
    // not type, and one that would miss every collision check afterwards.
    expect(copyDiagramName("Map.MD", [])).toEqual({ ok: true, name: "Map.MD" });
    expect(copyDiagramName("map.md", [])).toEqual({ ok: true, name: "map.md" });
  });

  it("trims the padding a text field collects", () => {
    expect(copyDiagramName("  spaced  ", [])).toEqual({ ok: true, name: "spaced.md" });
  });

  it("refuses a name that is only whitespace", () => {
    expect(copyDiagramName("   ", []).ok).toBe(false);
    expect(copyDiagramName("", []).ok).toBe(false);
  });

  it("refuses a name a saved diagram already has", () => {
    const result = copyDiagramName("notes", existing("notes.md"));
    expect(result.ok).toBe(false);
    if (!result.ok) expect(result.reason).toContain("notes.md");
  });

  it("compares the name it would write, not the name that was typed", () => {
    // The user typed no extension; the collision is only visible after the
    // extension the backend will add has been added here too.
    expect(copyDiagramName("notes", existing("notes.md")).ok).toBe(false);
    expect(copyDiagramName("notes.md", existing("notes.md")).ok).toBe(false);
  });

  it("refuses a collision that differs only in case", () => {
    // Windows would silently overwrite; Linux would not. Refusing on both is
    // the abstain — the cost is picking another name, the cost of guessing is
    // somebody's diagram.
    const result = copyDiagramName("NOTES", existing("notes.md"));
    expect(result.ok).toBe(false);
  });

  it("allows a name nothing else has", () => {
    expect(copyDiagramName("other", existing("notes.md", "map.md"))).toEqual({
      ok: true,
      name: "other.md",
    });
  });

  it("says why, in a sentence that names the file", () => {
    const result = copyDiagramName("notes", existing("notes.md"));
    if (result.ok) throw new Error("expected a refusal");
    expect(result.reason).toMatch(/notes\.md/);
    expect(result.reason.length).toBeGreaterThan(20);
  });
});

import { describe, expect, it } from "vitest";

import {
  BUILTIN_DIAGRAMS,
  derivationLabel,
  diagramEntries,
  warningSummary,
} from "./architectureLogic";
import type { Derivation, DiagramDerivation, DiagramFile } from "../../ipc/types";

/** A `DiagramFile` with only the fields under test spelled out. */
function file(name: string, extra: Partial<DiagramFile> = {}): DiagramFile {
  return {
    name,
    path: `.code-basics/diagrams/${name}`,
    level: null,
    derivation: "user",
    generated: null,
    edited: false,
    warning: null,
    ...extra,
  };
}

/** Index into a result, failing the test loudly rather than reading `undefined`. */
function at<T>(items: T[], index: number): T {
  const item = items[index];
  if (item === undefined) throw new Error(`no entry at ${index} of ${items.length}`);
  return item;
}

describe("diagramEntries", () => {
  it("puts both built-ins first, project map before component map", () => {
    const entries = diagramEntries([]);
    expect(entries.map((entry) => entry.label)).toEqual([
      "Project map",
      "Component map",
    ]);
    expect(entries.map((entry) => entry.builtin)).toEqual(["project", "component"]);
  });

  it("exposes the two built-ins as a table the caller can read directly", () => {
    expect(BUILTIN_DIAGRAMS.map((entry) => entry.builtin)).toEqual([
      "project",
      "component",
    ]);
    // The two are different pictures, not zoom levels, so each carries its own
    // sentence saying what it shows.
    for (const entry of BUILTIN_DIAGRAMS) {
      expect(entry.description.length).toBeGreaterThan(0);
    }
    const descriptions = BUILTIN_DIAGRAMS.map((entry) => entry.description);
    expect(new Set(descriptions).size).toBe(descriptions.length);
  });

  it("appends saved diagrams after the built-ins", () => {
    const entries = diagramEntries([file("runtime.md")]);
    expect(entries).toHaveLength(3);
    expect(at(entries, 2).source).toBe("saved");
    expect(at(entries, 2).label).toBe("runtime");
    expect(at(entries, 2).file?.name).toBe("runtime.md");
  });

  it("orders saved diagrams deterministically regardless of listing order", () => {
    const forwards = diagramEntries([file("a.md"), file("b.md"), file("c.md")]);
    const backwards = diagramEntries([file("c.md"), file("a.md"), file("b.md")]);
    expect(backwards.map((entry) => entry.id)).toEqual(
      forwards.map((entry) => entry.id),
    );
    expect(forwards.slice(2).map((entry) => entry.label)).toEqual(["a", "b", "c"]);
  });

  it("distinguishes a built-in from a saved diagram by more than its label", () => {
    // A saved diagram called "Project map" must not be confusable with the
    // derived one: same label, different id, different source, different
    // payload.
    const entries = diagramEntries([file("Project map.md")]);
    const builtin = at(entries, 0);
    const saved = at(entries, 2);
    expect(saved.label).toBe("Project map");
    expect(builtin.label).toBe("Project map");
    expect(saved.id).not.toBe(builtin.id);
    expect(saved.source).toBe("saved");
    expect(builtin.source).toBe("builtin");
    expect(builtin.file).toBeNull();
    expect(saved.builtin).toBeNull();
  });

  it("gives every entry a unique id", () => {
    const entries = diagramEntries([file("a.md"), file("b.md")]);
    expect(new Set(entries.map((entry) => entry.id)).size).toBe(entries.length);
  });

  it("keeps the file's own name when it does not end in .md", () => {
    const entries = diagramEntries([file("notes.mmd")]);
    expect(at(entries, 2).label).toBe("notes.mmd");
  });

  it("accepts anything with a name, not only a DiagramFile", () => {
    const entries = diagramEntries([{ name: "plain.md" }]);
    expect(at(entries, 2).label).toBe("plain");
    expect(at(entries, 2).file).toEqual({ name: "plain.md" });
  });
});

describe("derivationLabel", () => {
  it("reads the bare-string variant", () => {
    expect(derivationLabel("user")).toBe("User-authored");
    expect(derivationLabel("derived" as DiagramDerivation)).toBe("Derived");
  });

  it("reads the derived variant that carries a scanner version", () => {
    const derivation: Derivation = { derived: { scanner: 1 } };
    expect(derivationLabel(derivation)).toBe("Derived");
  });

  it("names the agent that inferred it", () => {
    expect(derivationLabel({ inferred: { agent: "claude" } })).toBe(
      "Inferred by claude",
    );
  });

  it("says an agent inferred it even when the agent is not named", () => {
    expect(derivationLabel({ inferred: { agent: "" } })).toBe("Inferred by an agent");
    expect(derivationLabel({ inferred: { agent: "   " } })).toBe(
      "Inferred by an agent",
    );
  });

  it("adds the edit to the origin rather than replacing it", () => {
    expect(derivationLabel({ inferred: { agent: "codex" } }, true)).toBe(
      "Inferred by codex, edited",
    );
    expect(derivationLabel("derived" as DiagramDerivation, true)).toBe(
      "Derived, edited",
    );
    expect(derivationLabel("user", true)).toBe("User-authored, edited");
  });

  it("abstains on a shape it does not recognise", () => {
    expect(derivationLabel(null as never)).toBe("Origin unknown");
    expect(derivationLabel("scribbled" as never)).toBe("Origin unknown");
    expect(derivationLabel({ conjured: { by: "nobody" } } as never)).toBe(
      "Origin unknown",
    );
    expect(derivationLabel({ inferred: { agent: 7 } } as never)).toBe(
      "Inferred by an agent",
    );
  });
});

describe("warningSummary", () => {
  it("says nothing at all when there is nothing to say", () => {
    expect(warningSummary([])).toBeNull();
    expect(warningSummary(null)).toBeNull();
    expect(warningSummary(["", "   "])).toBeNull();
  });

  it("counts one warning in the singular", () => {
    expect(warningSummary(["a reference resolved to nothing"])).toBe(
      "1 thing this diagram could not draw",
    );
  });

  it("counts several in the plural", () => {
    expect(warningSummary(["a", "b", "c"])).toBe(
      "3 things this diagram could not draw",
    );
  });

  it("does not count blank entries", () => {
    expect(warningSummary(["a", "", "  ", "b"])).toBe(
      "2 things this diagram could not draw",
    );
  });
});

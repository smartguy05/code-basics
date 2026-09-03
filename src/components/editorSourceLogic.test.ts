import { describe, expect, it } from "vitest";
import {
  DIFF_MODE_LABELS,
  EMPTY_SECRETS,
  diffFile,
  secretsFile,
  secretsProjects,
  sourceEnablesLsp,
  sourceEntersNavStack,
  sourceLanguageHint,
  workspaceFile,
  type EditorSource,
} from "./editorSourceLogic";
import type { ComparisonMode, Project } from "../ipc/types";

/** Every comparison mode, so a new one cannot slip past these tests unlabelled. */
const MODES: ComparisonMode[] = ["workingToHead", "workingToIndex", "indexToHead"];

/** A minimal Project stub — only the fields secretsProjects reads matter. */
function proj(over: Partial<Project> & Pick<Project, "id" | "ecosystem">): Project {
  return {
    name: over.id,
    manifestPath: `${over.id}.csproj`,
    dir: ".",
    kind: "library",
    frameworks: [],
    configurations: [],
    isTestProject: false,
    testRunner: null,
    ...over,
  } as Project;
}

describe("workspaceFile", () => {
  it("uses the path as identity and the base name as the label", () => {
    const f = workspaceFile("src/App.tsx");
    expect(f.id).toBe("src/App.tsx");
    expect(f.name).toBe("App.tsx");
    expect(f.source).toEqual({ kind: "workspace", path: "src/App.tsx" });
  });

  it("takes the last segment from either separator", () => {
    expect(workspaceFile("a\\b\\Program.cs").name).toBe("Program.cs");
    expect(workspaceFile("a/b/Program.cs").name).toBe("Program.cs");
  });

  it("falls back to the whole path when there is no separator", () => {
    expect(workspaceFile("README").name).toBe("README");
  });
});

describe("secretsFile", () => {
  it("namespaces the identity so it cannot collide with a workspace path", () => {
    const f = secretsFile("src/MyApi/MyApi.csproj");
    expect(f.id).toBe("secrets:src/MyApi/MyApi.csproj");
    expect(f.id.startsWith("secrets:")).toBe(true);
    expect(f.source).toEqual({ kind: "secrets", project: "src/MyApi/MyApi.csproj" });
  });

  it("labels the tab secrets.json", () => {
    expect(secretsFile("src/MyApi/MyApi.csproj").name).toBe("secrets.json");
  });

  it("gives a distinct identity per project and a stable one per project", () => {
    expect(secretsFile("a.csproj").id).not.toBe(secretsFile("b.csproj").id);
    expect(secretsFile("a.csproj").id).toBe(secretsFile("a.csproj").id);
  });
});

describe("diffFile", () => {
  it("namespaces the identity so it cannot collide with a workspace path", () => {
    const f = diffFile("src/App.tsx", "workingToHead");
    expect(f.id).toBe("diff:workingToHead:src/App.tsx");
    expect(f.source).toEqual({ kind: "diff", path: "src/App.tsx", mode: "workingToHead" });
  });

  it("puts the mode in the identity, so two baselines are two tabs", () => {
    const staged = diffFile("src/App.tsx", "indexToHead");
    const unstaged = diffFile("src/App.tsx", "workingToIndex");
    expect(staged.id).not.toBe(unstaged.id);
  });

  it("is stable for the same path and mode, so reopening finds the same tab", () => {
    expect(diffFile("a.cs", "indexToHead").id).toBe(diffFile("a.cs", "indexToHead").id);
  });

  it("labels the tab with the file name and the mode, so the two are told apart", () => {
    expect(diffFile("src/a/App.tsx", "indexToHead").name).toBe("App.tsx (Staged)");
    expect(diffFile("src\\a\\App.tsx", "workingToIndex").name).toBe("App.tsx (Unstaged)");
    expect(diffFile("README", "workingToHead").name).toBe("README (Diff)");
  });

  it("gives every mode a distinct id and a distinct label for one path", () => {
    const ids = MODES.map((m) => diffFile("a.cs", m).id);
    const names = MODES.map((m) => diffFile("a.cs", m).name);
    expect(new Set(ids).size).toBe(MODES.length);
    expect(new Set(names).size).toBe(MODES.length);
  });

  it("labels every mode, so no mode falls through to an empty suffix", () => {
    for (const mode of MODES) {
      expect(DIFF_MODE_LABELS[mode].trim()).not.toBe("");
    }
  });
});

describe("the three id schemes", () => {
  it("cannot collide, even for the same underlying string", () => {
    const same = "src/MyApi/MyApi.csproj";
    const ids = [
      workspaceFile(same).id,
      secretsFile(same).id,
      ...MODES.map((m) => diffFile(same, m).id),
    ];
    expect(new Set(ids).size).toBe(ids.length);
  });

  it("keeps a workspace path bare and namespaces the other two", () => {
    expect(workspaceFile("diff.ts").id).toBe("diff.ts");
    expect(secretsFile("a.csproj").id.startsWith("secrets:")).toBe(true);
    expect(diffFile("a.cs", "workingToHead").id.startsWith("diff:")).toBe(true);
  });
});

describe("sourceEnablesLsp", () => {
  it("is true only for a workspace file", () => {
    expect(sourceEnablesLsp({ kind: "workspace", path: "a.cs" })).toBe(true);
    expect(sourceEnablesLsp({ kind: "secrets", project: "a.csproj" })).toBe(false);
  });

  it("is false for a diff — its lines are two revisions, not the file's text", () => {
    for (const mode of MODES) {
      expect(sourceEnablesLsp({ kind: "diff", path: "a.cs", mode })).toBe(false);
    }
  });
});

describe("sourceEntersNavStack", () => {
  it("is true only for a workspace file, the one source the stack can reopen", () => {
    expect(sourceEntersNavStack({ kind: "workspace", path: "a.cs" })).toBe(true);
    expect(sourceEntersNavStack({ kind: "secrets", project: "a.csproj" })).toBe(false);
    expect(sourceEntersNavStack({ kind: "diff", path: "a.cs", mode: "indexToHead" })).toBe(false);
  });
});

describe("sourceLanguageHint", () => {
  it("is the path for a workspace file, so highlighting keys off its extension", () => {
    const source: EditorSource = { kind: "workspace", path: "src/App.tsx" };
    expect(sourceLanguageHint(source)).toBe("src/App.tsx");
  });

  it("is a .json name for secrets, so the tab gets JSON highlighting", () => {
    const source: EditorSource = { kind: "secrets", project: "a.csproj" };
    expect(sourceLanguageHint(source)).toBe("secrets.json");
  });

  it("is the compared file's path for a diff, so each side highlights as that file", () => {
    const source: EditorSource = { kind: "diff", path: "src/App.tsx", mode: "workingToHead" };
    expect(sourceLanguageHint(source)).toBe("src/App.tsx");
  });
});

describe("secretsProjects", () => {
  it("returns every readable .NET project, so a picker can list them all", () => {
    const projects = [
      proj({ id: "Api", ecosystem: "dotnet" }),
      proj({ id: "Worker", ecosystem: "dotnet" }),
      proj({ id: "web", ecosystem: "node" }),
    ];
    expect(secretsProjects(projects).map((p) => p.id)).toEqual(["Api", "Worker"]);
  });

  it("excludes non-.NET projects", () => {
    const projects = [proj({ id: "web", ecosystem: "node" }), proj({ id: "cli", ecosystem: "cargo" })];
    expect(secretsProjects(projects)).toEqual([]);
  });

  it("excludes an unreadable .NET project (its manifest never parsed)", () => {
    const projects = [
      proj({ id: "Ok", ecosystem: "dotnet" }),
      proj({ id: "Broken", ecosystem: "dotnet", unreadable: "could not parse" }),
    ];
    expect(secretsProjects(projects).map((p) => p.id)).toEqual(["Ok"]);
  });

  it("preserves scan order", () => {
    const projects = [
      proj({ id: "B", ecosystem: "dotnet" }),
      proj({ id: "A", ecosystem: "dotnet" }),
    ];
    expect(secretsProjects(projects).map((p) => p.id)).toEqual(["B", "A"]);
  });
});

describe("EMPTY_SECRETS", () => {
  it("is an empty JSON object the write command accepts", () => {
    expect(JSON.parse(EMPTY_SECRETS)).toEqual({});
  });
});

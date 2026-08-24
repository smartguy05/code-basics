import { describe, expect, it } from "vitest";
import {
  EMPTY_SECRETS,
  secretsFile,
  sourceEnablesLsp,
  sourceLanguageHint,
  workspaceFile,
  type EditorSource,
} from "./editorSourceLogic";

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

describe("sourceEnablesLsp", () => {
  it("is true only for a workspace file", () => {
    expect(sourceEnablesLsp({ kind: "workspace", path: "a.cs" })).toBe(true);
    expect(sourceEnablesLsp({ kind: "secrets", project: "a.csproj" })).toBe(false);
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
});

describe("EMPTY_SECRETS", () => {
  it("is an empty JSON object the write command accepts", () => {
    expect(JSON.parse(EMPTY_SECRETS)).toEqual({});
  });
});

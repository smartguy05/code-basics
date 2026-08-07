import { describe, expect, it } from "vitest";
import { envToText, projectTarget, textToEnv } from "./configLogic";
import type { Project } from "../ipc/types";

function project(overrides: Partial<Project>): Project {
  return {
    id: "p",
    name: "p",
    manifestPath: "C:\\ws\\src\\App\\App.csproj",
    dir: "C:\\ws\\src\\App",
    ecosystem: "dotnet",
    kind: "executable",
    frameworks: [],
    configurations: [],
    isTestProject: false,
    testRunner: null,
    ...overrides,
  };
}

describe("envToText", () => {
  it("writes one KEY=value line per entry", () => {
    expect(envToText({ A: "1", B: "2" })).toBe("A=1\nB=2");
  });

  it("returns an empty string for undefined or empty env", () => {
    expect(envToText(undefined)).toBe("");
    expect(envToText({})).toBe("");
  });
});

describe("textToEnv", () => {
  it("splits on the first '=' so values keep theirs", () => {
    const env = textToEnv("ConnectionStrings__Db=Server=x;Pwd=y=");
    expect(env).toEqual({ ConnectionStrings__Db: "Server=x;Pwd=y=" });
  });

  it("skips blank lines and comments", () => {
    expect(textToEnv("\n# a comment\n  \nA=1\n   # indented comment\n")).toEqual({
      A: "1",
    });
  });

  it("trims whitespace around the key and the value", () => {
    expect(textToEnv("  KEY  =  value  ")).toEqual({ KEY: "value" });
  });

  it("skips lines with no '=' and lines starting with '='", () => {
    expect(textToEnv("JUST_A_KEY\n=novalue\nA=1")).toEqual({ A: "1" });
  });

  it("keeps an empty value", () => {
    expect(textToEnv("EMPTY=")).toEqual({ EMPTY: "" });
  });

  it("lets a later duplicate key win", () => {
    expect(textToEnv("A=1\nA=2")).toEqual({ A: "2" });
  });

  it("round-trips through envToText, including '=' in values", () => {
    const env = { A: "1", TOKEN: "a=b=c", EMPTY: "" };
    expect(textToEnv(envToText(env))).toEqual(env);
  });
});

describe("projectTarget", () => {
  it("uses the manifest file for dotnet projects", () => {
    expect(projectTarget(project({}), "C:\\ws")).toBe("src\\App\\App.csproj");
  });

  it("uses the directory for node projects", () => {
    const node = project({
      ecosystem: "node",
      dir: "C:\\ws\\web",
      manifestPath: "C:\\ws\\web\\package.json",
    });
    expect(projectTarget(node, "C:\\ws")).toBe("web");
  });

  it("uses the directory for any non-dotnet ecosystem", () => {
    const python = project({
      ecosystem: "python",
      dir: "C:\\ws\\svc",
      manifestPath: "C:\\ws\\svc\\pyproject.toml",
    });
    expect(projectTarget(python, "C:\\ws")).toBe("svc");
  });

  it("ignores trailing separators on the root", () => {
    expect(projectTarget(project({}), "C:\\ws\\\\")).toBe("src\\App\\App.csproj");
  });

  it("handles forward-slash paths", () => {
    const p = project({
      ecosystem: "node",
      dir: "/home/me/ws/web",
      manifestPath: "/home/me/ws/web/package.json",
    });
    expect(projectTarget(p, "/home/me/ws/")).toBe("web");
  });

  it("returns the absolute path when it is outside the root", () => {
    const outside = project({ manifestPath: "D:\\other\\App.csproj" });
    expect(projectTarget(outside, "C:\\ws")).toBe("D:\\other\\App.csproj");
  });

  it("returns an empty string when the project is the root itself", () => {
    const atRoot = project({ ecosystem: "node", dir: "C:\\ws" });
    expect(projectTarget(atRoot, "C:\\ws")).toBe("");
  });
});

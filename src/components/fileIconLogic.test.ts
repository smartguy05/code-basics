import { describe, expect, it } from "vitest";
import { GENERIC_FILE, extensionOf, iconFor } from "./fileIconLogic";

describe("iconFor: directories", () => {
  it("shows a closed folder when collapsed and an open one when expanded", () => {
    expect(iconFor("src", true, false)).toBe("folder");
    expect(iconFor("src", true, true)).toBe("folder-open");
  });

  it("ignores a directory's name entirely — a folder called Cargo.toml is a folder", () => {
    expect(iconFor("Cargo.toml", true, false)).toBe("folder");
  });

  it("ignores the expanded flag for a file, which cannot be expanded", () => {
    expect(iconFor("main.rs", false, true)).toBe("rust");
  });
});

describe("iconFor: extensions", () => {
  it("recognises the ecosystems this app opens", () => {
    expect(iconFor("Program.cs", false, false)).toBe("csharp");
    expect(iconFor("App.csproj", false, false)).toBe("csharp");
    expect(iconFor("CodeBasics.sln", false, false)).toBe("visualstudio");
    expect(iconFor("lib.rs", false, false)).toBe("rust");
    expect(iconFor("rustfmt.toml", false, false)).toBe("toml");
    expect(iconFor("api.ts", false, false)).toBe("typescript");
    expect(iconFor("App.tsx", false, false)).toBe("react");
    expect(iconFor("main.jsx", false, false)).toBe("react");
    expect(iconFor("vite.config.js", false, false)).toBe("javascript");
    expect(iconFor("settings.json", false, false)).toBe("json");
    expect(iconFor("README.md", false, false)).toBe("markdown");
    expect(iconFor("build.yaml", false, false)).toBe("yaml");
    expect(iconFor("build.yml", false, false)).toBe("yaml");
    expect(iconFor("schema.sql", false, false)).toBe("database");
    expect(iconFor("index.html", false, false)).toBe("html");
    expect(iconFor("styles.css", false, false)).toBe("css");
    expect(iconFor("app.config.xml", false, false)).toBe("xml");
    expect(iconFor("build.ps1", false, false)).toBe("powershell");
    expect(iconFor("setup.sh", false, false)).toBe("console");
    expect(iconFor("train.py", false, false)).toBe("python");
    expect(iconFor("Main.java", false, false)).toBe("java");
    expect(iconFor("main.go", false, false)).toBe("go");
    expect(iconFor("logo.png", false, false)).toBe("image");
    expect(iconFor("logo.svg", false, false)).toBe("svg");
    expect(iconFor("app.ico", false, false)).toBe("image");
    expect(iconFor("server.log", false, false)).toBe("log");
    expect(iconFor("deps.lock", false, false)).toBe("lock");
    expect(iconFor("orders.http", false, false)).toBe("http");
  });

  it("matches an extension case-insensitively", () => {
    expect(iconFor("Program.CS", false, false)).toBe("csharp");
    expect(iconFor("README.MD", false, false)).toBe("markdown");
    expect(iconFor("App.TSX", false, false)).toBe("react");
  });

  it("uses only the last extension of a multi-dot name", () => {
    expect(iconFor("vite.config.ts", false, false)).toBe("typescript");
    expect(iconFor("appsettings.Development.json", false, false)).toBe("json");
  });
});

describe("iconFor: exact names beat extensions", () => {
  it("reads a manifest as its ecosystem rather than its format", () => {
    expect(iconFor("Cargo.toml", false, false)).toBe("rust");
    expect(iconFor("package.json", false, false)).toBe("nodejs");
    expect(iconFor("tauri.conf.json", false, false)).toBe("json");
  });

  it("reads a lock file as a lock rather than as the format it happens to use", () => {
    expect(iconFor("pnpm-lock.yaml", false, false)).toBe("lock");
    expect(iconFor("package-lock.json", false, false)).toBe("lock");
    expect(iconFor("Cargo.lock", false, false)).toBe("rust");
  });

  it("matches an exact name case-insensitively, so Dockerfile and dockerfile agree", () => {
    expect(iconFor("Dockerfile", false, false)).toBe("docker");
    expect(iconFor("dockerfile", false, false)).toBe("docker");
    expect(iconFor("docker-compose.yml", false, false)).toBe("docker");
    expect(iconFor("CARGO.TOML", false, false)).toBe("rust");
  });

  it("treats a leading dot as part of the name, not as an extension", () => {
    expect(iconFor(".gitignore", false, false)).toBe("git");
    expect(iconFor(".gitattributes", false, false)).toBe("git");
  });
});

describe("iconFor: abstaining", () => {
  it("falls back to the generic document for an unknown extension", () => {
    expect(iconFor("data.qzx", false, false)).toBe(GENERIC_FILE);
    expect(iconFor("a.b.c", false, false)).toBe(GENERIC_FILE);
  });

  it("falls back for a name with no extension at all", () => {
    expect(iconFor("noext", false, false)).toBe(GENERIC_FILE);
    expect(iconFor("LICENSE", false, false)).toBe(GENERIC_FILE);
  });

  it("falls back for an unrecognised dotfile rather than reading its tail as an extension", () => {
    expect(iconFor(".env", false, false)).toBe(GENERIC_FILE);
    expect(iconFor(".ts", false, false)).toBe(GENERIC_FILE);
  });

  it("survives the degenerate names a listing can hand it", () => {
    expect(iconFor("", false, false)).toBe(GENERIC_FILE);
    expect(iconFor(".", false, false)).toBe(GENERIC_FILE);
    expect(iconFor("..", false, false)).toBe(GENERIC_FILE);
    expect(iconFor("trailing.", false, false)).toBe(GENERIC_FILE);
    expect(iconFor("", true, false)).toBe("folder");
  });
});

describe("extensionOf", () => {
  it("lowercases what it returns", () => {
    expect(extensionOf("Program.CS")).toBe("cs");
  });

  it("returns nothing for a name whose only dot leads it", () => {
    expect(extensionOf(".gitignore")).toBe("");
    expect(extensionOf(".")).toBe("");
  });

  it("returns nothing when there is no dot, or nothing after it", () => {
    expect(extensionOf("noext")).toBe("");
    expect(extensionOf("..")).toBe("");
    expect(extensionOf("trailing.")).toBe("");
  });
});

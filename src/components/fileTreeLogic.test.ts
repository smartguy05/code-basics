import { describe, expect, it } from "vitest";
import {
  baseName,
  createPath,
  isRenameWorthSending,
  joinPath,
  parentDir,
  renamePath,
  targetDir,
  validateName,
} from "./fileTreeLogic";

describe("targetDir", () => {
  it("creates inside a folder that was right-clicked", () => {
    expect(targetDir({ path: "src/views", isDir: true })).toBe("src/views");
  });

  it("creates beside a file that was right-clicked", () => {
    expect(targetDir({ path: "src/views/App.tsx", isDir: false })).toBe("src/views");
  });

  it("creates at the root when nothing was right-clicked", () => {
    expect(targetDir(null)).toBe("");
  });

  it("creates at the root beside a top-level file", () => {
    expect(targetDir({ path: "README.md", isDir: false })).toBe("");
  });
});

describe("parentDir and baseName", () => {
  it("splits a nested path", () => {
    expect(parentDir("a/b/c.ts")).toBe("a/b");
    expect(baseName("a/b/c.ts")).toBe("c.ts");
  });

  it("treats a single segment as living at the root", () => {
    expect(parentDir("c.ts")).toBe("");
    expect(baseName("c.ts")).toBe("c.ts");
  });
});

describe("joinPath", () => {
  it("does not put a leading slash on a root-level path", () => {
    expect(joinPath("", "a.ts")).toBe("a.ts");
  });

  it("joins a directory and a name", () => {
    expect(joinPath("src", "a.ts")).toBe("src/a.ts");
  });
});

describe("validateName", () => {
  it("accepts an ordinary name", () => {
    expect(validateName("App.tsx")).toBeNull();
  });

  it("accepts a name with spaces inside it", () => {
    expect(validateName("my notes.md")).toBeNull();
  });

  it("accepts a nested name, so the folders can be typed in one go", () => {
    expect(validateName("views/parts/App.tsx")).toBeNull();
  });

  it("refuses an empty or blank name", () => {
    expect(validateName("")).toBe("A name is required.");
    expect(validateName("   ")).toBe("A name is required.");
  });

  it("refuses an absolute path in either spelling", () => {
    expect(validateName("/etc/passwd")).toMatch(/absolute path/);
    expect(validateName("\\Windows\\win.ini")).toMatch(/absolute path/);
  });

  it("refuses characters Windows will not accept", () => {
    for (const name of ["a<b", "a>b", "a:b", 'a"b', "a|b", "a?b", "a*b"]) {
      expect(validateName(name)).toMatch(/cannot contain/);
    }
  });

  it("refuses a name that would leave the workspace", () => {
    expect(validateName("..")).toMatch(/\. or \.\./);
    expect(validateName("../secrets")).toMatch(/\. or \.\./);
    expect(validateName("src/../../x")).toMatch(/\. or \.\./);
  });

  it("refuses an empty folder segment and a trailing slash", () => {
    expect(validateName("a//b")).toMatch(/empty folder/);
    expect(validateName("a/")).toMatch(/empty folder/);
  });

  it("refuses a segment Windows would silently rename", () => {
    // `a ` and `a.` are stored as `a`, so the name read back is not the name
    // that was typed.
    expect(validateName("a /b.ts")).toMatch(/space or a dot/);
    expect(validateName("a./b.ts")).toMatch(/space or a dot/);
  });

  it("does not mistake an extension for a trailing dot", () => {
    expect(validateName("App.tsx")).toBeNull();
  });
});

describe("createPath", () => {
  it("resolves a name inside the target directory", () => {
    expect(createPath("src", "App.tsx")).toBe("src/App.tsx");
  });

  it("resolves a name at the root", () => {
    expect(createPath("", "App.tsx")).toBe("App.tsx");
  });

  it("trims what the user typed", () => {
    expect(createPath("src", "  App.tsx  ")).toBe("src/App.tsx");
  });

  it("accepts the Windows separator and stores the forward-slash form", () => {
    expect(createPath("src", "views\\App.tsx")).toBe("src/views/App.tsx");
  });

  it("abstains on a name it would not accept", () => {
    expect(createPath("src", "..")).toBeNull();
    expect(createPath("src", "")).toBeNull();
  });
});

describe("renamePath", () => {
  it("replaces the last segment and keeps the folder", () => {
    expect(renamePath("src/a.ts", "b.ts")).toBe("src/b.ts");
  });

  it("renames at the root", () => {
    expect(renamePath("a.ts", "b.ts")).toBe("b.ts");
  });

  it("moves the file when the new name has folders in it", () => {
    expect(renamePath("src/a.ts", "views/b.ts")).toBe("src/views/b.ts");
  });

  it("abstains on a name it would not accept", () => {
    expect(renamePath("src/a.ts", "../a.ts")).toBeNull();
  });
});

describe("isRenameWorthSending", () => {
  it("is false when the name has not changed", () => {
    expect(isRenameWorthSending("src/a.ts", "a.ts")).toBe(false);
    expect(isRenameWorthSending("src/a.ts", "  a.ts ")).toBe(false);
  });

  it("is false when the name is unusable", () => {
    expect(isRenameWorthSending("src/a.ts", "")).toBe(false);
  });

  it("is true for a real change", () => {
    expect(isRenameWorthSending("src/a.ts", "b.ts")).toBe(true);
  });
});

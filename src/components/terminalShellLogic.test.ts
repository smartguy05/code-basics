import { describe, expect, it } from "vitest";
import type { ShellInfo } from "../ipc/types";
import {
  DEFAULT_TERMINAL_SHELL,
  loadTerminalShell,
  missingShellNotice,
  readTerminalShell,
  resolvePreferredShell,
  saveTerminalShell,
  TERMINAL_SHELL_STORAGE_KEY,
} from "./terminalShellLogic";

const shell = (id: string, program: string, args: string[] = []): ShellInfo => ({
  id,
  label: id,
  program,
  args,
});

/** A storage stand-in; the module takes storage as a parameter, never the global. */
function fakeStorage(initial: Record<string, string> = {}) {
  const map = new Map(Object.entries(initial));
  return {
    map,
    getItem: (key: string) => map.get(key) ?? null,
    setItem: (key: string, value: string) => void map.set(key, value),
  };
}

describe("readTerminalShell", () => {
  it("defaults to the system shell when nothing is stored", () => {
    expect(readTerminalShell(null)).toEqual(DEFAULT_TERMINAL_SHELL);
    expect(DEFAULT_TERMINAL_SHELL.shellId).toBe(null);
  });

  it("defaults on blank, unparseable, non-object and array payloads", () => {
    expect(readTerminalShell("")).toEqual(DEFAULT_TERMINAL_SHELL);
    expect(readTerminalShell("   ")).toEqual(DEFAULT_TERMINAL_SHELL);
    expect(readTerminalShell("{not json")).toEqual(DEFAULT_TERMINAL_SHELL);
    expect(readTerminalShell("42")).toEqual(DEFAULT_TERMINAL_SHELL);
    expect(readTerminalShell("null")).toEqual(DEFAULT_TERMINAL_SHELL);
    expect(readTerminalShell('["pwsh"]')).toEqual(DEFAULT_TERMINAL_SHELL);
  });

  it("defaults on a version it does not understand rather than guessing the shape", () => {
    expect(readTerminalShell('{"version":2,"shellId":"pwsh"}')).toEqual(DEFAULT_TERMINAL_SHELL);
    expect(readTerminalShell('{"shellId":"pwsh"}')).toEqual(DEFAULT_TERMINAL_SHELL);
  });

  it("reads a stored id", () => {
    expect(readTerminalShell('{"version":1,"shellId":"pwsh"}')).toEqual({
      version: 1,
      shellId: "pwsh",
    });
  });

  it("treats a non-string or empty id as no preference", () => {
    expect(readTerminalShell('{"version":1,"shellId":""}')).toEqual(DEFAULT_TERMINAL_SHELL);
    expect(readTerminalShell('{"version":1,"shellId":7}')).toEqual(DEFAULT_TERMINAL_SHELL);
    expect(readTerminalShell('{"version":1}')).toEqual(DEFAULT_TERMINAL_SHELL);
  });

  it("never throws on a getItem that does", () => {
    const hostile: Pick<Storage, "getItem"> = {
      getItem: () => {
        throw new Error("storage unavailable");
      },
    };
    expect(loadTerminalShell(hostile)).toEqual(DEFAULT_TERMINAL_SHELL);
  });
});

describe("loadTerminalShell / saveTerminalShell", () => {
  it("round-trips through the versioned key", () => {
    const storage = fakeStorage();
    saveTerminalShell(storage, { version: 1, shellId: "cmd" });
    expect(storage.map.get(TERMINAL_SHELL_STORAGE_KEY)).toBe('{"version":1,"shellId":"cmd"}');
    expect(loadTerminalShell(storage)).toEqual({ version: 1, shellId: "cmd" });
  });

  it("swallows a failing write, because persistence is a convenience", () => {
    const hostile: Pick<Storage, "setItem"> = {
      setItem: () => {
        throw new Error("quota");
      },
    };
    expect(() => saveTerminalShell(hostile, DEFAULT_TERMINAL_SHELL)).not.toThrow();
  });
});

describe("resolvePreferredShell", () => {
  const detected = [shell("pwsh", "C:\\pwsh.exe"), shell("cmd", "C:\\cmd.exe", ["/K"])];

  it("returns the resolved program and args of the remembered shell", () => {
    expect(resolvePreferredShell({ version: 1, shellId: "cmd" }, detected)).toEqual({
      program: "C:\\cmd.exe",
      args: ["/K"],
    });
  });

  it("abstains when there is no preference, so the backend default decides", () => {
    expect(resolvePreferredShell(DEFAULT_TERMINAL_SHELL, detected)).toBe(null);
  });

  it("abstains while detection has not been read, rather than guessing off a stale list", () => {
    expect(resolvePreferredShell({ version: 1, shellId: "cmd" }, null)).toBe(null);
  });

  it("abstains when the remembered id names nothing detected", () => {
    expect(resolvePreferredShell({ version: 1, shellId: "fish" }, detected)).toBe(null);
    expect(resolvePreferredShell({ version: 1, shellId: "fish" }, [])).toBe(null);
  });

  it("does not fall back to the bare id as a program when the shell is gone", () => {
    // Spawning a remembered bare name whose file no longer exists would fail the
    // open instead of opening a terminal, which is worse than the default shell.
    const resolved = resolvePreferredShell({ version: 1, shellId: "fish" }, detected);
    expect(resolved).not.toEqual({ program: "fish", args: [] });
    expect(resolved).toBe(null);
  });

  it("copies the detected args so a later mutation cannot rewrite a live spawn", () => {
    const args: string[] = ["/K"];
    const resolved = resolvePreferredShell({ version: 1, shellId: "cmd" }, [
      shell("cmd", "C:\\cmd.exe", args),
    ]);
    args.push("evil");
    expect(resolved).toEqual({ program: "C:\\cmd.exe", args: ["/K"] });
  });
});

describe("missingShellNotice", () => {
  const detected = [shell("pwsh", "C:\\pwsh.exe")];

  it("says nothing when the remembered shell is here", () => {
    expect(missingShellNotice({ version: 1, shellId: "pwsh" }, detected)).toBe(null);
  });

  it("says nothing when there is no preference to be missing", () => {
    expect(missingShellNotice(DEFAULT_TERMINAL_SHELL, detected)).toBe(null);
    expect(missingShellNotice(DEFAULT_TERMINAL_SHELL, [])).toBe(null);
  });

  it("stays silent while detection is in flight, so an in-flight read is no alarm", () => {
    expect(missingShellNotice({ version: 1, shellId: "pwsh" }, null)).toBe(null);
    expect(missingShellNotice({ version: 1, shellId: "fish" }, null)).toBe(null);
  });

  it("names the missing shell once detection has answered, and says what happens instead", () => {
    const notice = missingShellNotice({ version: 1, shellId: "fish" }, detected);
    expect(notice).not.toBe(null);
    expect(notice).toContain("fish");
    expect(notice?.toLowerCase()).toContain("system default");
  });

  it("keeps the preference readable after the notice — nothing erases it", () => {
    const storage = fakeStorage();
    saveTerminalShell(storage, { version: 1, shellId: "fish" });
    expect(missingShellNotice(loadTerminalShell(storage), detected)).not.toBe(null);
    expect(loadTerminalShell(storage)).toEqual({ version: 1, shellId: "fish" });
  });
});

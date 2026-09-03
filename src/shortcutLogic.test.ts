import { describe, expect, it } from "vitest";
import { COMMANDS, chordEquals, conflictingCommand, effectiveBinding, formatChord, readShortcutOverrides, pickCommandTarget, commandSections } from "./shortcutLogic";

const command = (id: string) => COMMANDS.find((candidate) => candidate.id === id)!;

describe("custom shortcuts", () => {
  it("uses Ctrl+N for Search All and leaves Symbols unbound", () => {
    expect(formatChord(effectiveBinding(command("search.all"), {}))).toBe("Ctrl+N");
    expect(effectiveBinding(command("search.symbols"), {})).toBeNull();
  });

  it("normalises printable keys and rejects conflicts in the same context", () => {
    const binding = { key: "N", ctrl: true, shift: true, alt: false, meta: false };
    expect(chordEquals(binding, { ...binding, key: "n" })).toBe(true);
    expect(conflictingCommand(command("search.symbols"), binding, {})?.id).toBe("search.files");
  });

  it("binds the file tree's reveal to Alt+F1 and leaves collapse unbound", () => {
    expect(formatChord(effectiveBinding(command("tree.reveal"), {}))).toBe("Alt+F1");
    expect(effectiveBinding(command("tree.collapse"), {})).toBeNull();
    // Alt+F1 must be the tree's alone, or which command answers would depend on
    // registration order rather than on what the user asked for.
    const binding = effectiveBinding(command("tree.reveal"), {})!;
    expect(conflictingCommand(command("tree.reveal"), binding, {})).toBeNull();
  });

  it("keeps explicit unbound overrides and ignores malformed entries", () => {
    const read = readShortcutOverrides(JSON.stringify({ "search.all": null, broken: { key: 4 } }));
    expect(read).toEqual({ "search.all": null });
  });
});

describe("pickCommandTarget", () => {
  const shown = (disabled = false) => ({ rendered: true, disabled });
  const hidden = (disabled = false) => ({ rendered: false, disabled });

  it("says so when nothing carries the command", () => {
    expect(pickCommandTarget([])).toEqual({ kind: "none" });
  });

  it("takes the rendered one, not the first in document order", () => {
    // The bug this exists for: every open codebase mounts its own view, and a
    // background one is only `hidden` (display:none, still in the DOM). Taking
    // the first match fired Run and Changes commands — `changes.commit`
    // included — against whichever codebase happened to be listed first.
    expect(pickCommandTarget([hidden(), shown()])).toEqual({ kind: "target", index: 1 });
    expect(pickCommandTarget([shown(), hidden()])).toEqual({ kind: "target", index: 0 });
    expect(pickCommandTarget([hidden(), hidden(), shown()])).toEqual({ kind: "target", index: 2 });
  });

  it("refuses rather than falling back to an off-screen match", () => {
    // Acting on a codebase the user cannot see is worse than doing nothing —
    // and doing nothing is what the key did before anyone bound it.
    expect(pickCommandTarget([hidden(), hidden()])).toEqual({ kind: "hidden" });
  });

  it("keeps 'off screen' and 'refusing' apart", () => {
    // Two different facts: a hidden match is a wiring bug, a disabled one is
    // the app correctly saying no. Collapsing them hides the first.
    expect(pickCommandTarget([hidden()])).toEqual({ kind: "hidden" });
    expect(pickCommandTarget([shown(true)])).toEqual({ kind: "disabled", index: 0 });
  });

  it("does not look past a disabled visible control for an enabled hidden one", () => {
    // Reaching further would act on some surface other than the one on screen,
    // which is the whole bug in a different costume.
    expect(pickCommandTarget([shown(true), hidden(false)])).toEqual({ kind: "disabled", index: 0 });
  });

  it("ignores a hidden control's disabled state entirely", () => {
    expect(pickCommandTarget([hidden(true), shown(false)])).toEqual({ kind: "target", index: 1 });
  });
});

describe("default bindings", () => {
  it("binds Run to F5", () => {
    expect(formatChord(effectiveBinding(command("run.run"), {}))).toBe("F5");
  });

  it("leaves the other run controls unbound", () => {
    // A default is a claim on a key the user cannot easily see; only Run has
    // a convention strong enough to earn one.
    for (const id of ["run.stop", "run.restart", "run.build", "run.rebuild", "run.clean"]) {
      expect(effectiveBinding(command(id), {})).toBeNull();
    }
  });

  it("does not collide with anything else bound by default", () => {
    const f5 = effectiveBinding(command("run.run"), {})!;
    expect(conflictingCommand(command("run.run"), f5, {})).toBeNull();
  });
});

describe("commandSections", () => {
  it("puts the application's own commands first", () => {
    const sections = commandSections(COMMANDS);
    expect(sections[0]?.title).toBe("Application");
    expect(sections[0]?.plugin).toBeNull();
  });

  it("gives each plugin its own block, after the application", () => {
    const sections = commandSections(COMMANDS);
    const titles = sections.map((s) => s.title);
    expect(titles).toContain("SQL Console");
    expect(titles.indexOf("SQL Console")).toBeGreaterThan(0);
  });

  it("files every sql command under the plugin, not the application", () => {
    const sections = commandSections(COMMANDS);
    const app = sections.find((s) => s.plugin === null)!;
    expect(app.commands.some((c) => c.id.startsWith("sql."))).toBe(false);
    const sql = sections.find((s) => s.plugin === "sqlConsole")!;
    expect(sql.commands.every((c) => c.id.startsWith("sql."))).toBe(true);
  });

  it("loses no command to the split", () => {
    // The list is how a key is rebound; a command that fell out of every
    // block would be unreachable rather than merely misfiled.
    const total = commandSections(COMMANDS).reduce((sum, s) => sum + s.commands.length, 0);
    expect(total).toBe(COMMANDS.length);
  });

  it("drops a block with nothing in it rather than showing an empty heading", () => {
    // An empty heading reads as a feature that has no shortcuts, which is a
    // different and wrong claim. Simulates a search that matched nothing.
    expect(commandSections([])).toEqual([]);
  });

  it("orders plugins stably, so blocks do not move between releases", () => {
    const titles = commandSections(COMMANDS).slice(1).map((s) => s.plugin!);
    expect(titles).toEqual([...titles].sort());
  });
});

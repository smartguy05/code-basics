import { describe, expect, it } from "vitest";
import { COMMANDS, chordEquals, conflictingCommand, effectiveBinding, formatChord, readShortcutOverrides } from "./shortcutLogic";

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

  it("keeps explicit unbound overrides and ignores malformed entries", () => {
    const read = readShortcutOverrides(JSON.stringify({ "search.all": null, broken: { key: 4 } }));
    expect(read).toEqual({ "search.all": null });
  });
});

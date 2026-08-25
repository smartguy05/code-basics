import { describe, expect, it } from "vitest";
import type { Note } from "../ipc/types";
import {
  addNote,
  deleteNote,
  loadActiveId,
  makeNote,
  nextActiveAfterDelete,
  renameNote,
  resolveActiveId,
  saveActiveId,
  sendToAgentTitle,
  updateBody,
  UNTITLED,
} from "./notesLogic";

function seed(...titles: string[]): Note[] {
  return titles.map((title, i) => ({
    id: `note-${i + 1}`,
    title,
    body: "",
    createdAtMs: 100,
    updatedAtMs: 100,
  }));
}

describe("makeNote", () => {
  it("names a note from its sequence and stamps both times", () => {
    const n = makeNote(3, 555);
    expect(n).toEqual({
      id: "note-3",
      title: "Note 3",
      body: "",
      createdAtMs: 555,
      updatedAtMs: 555,
    });
  });
});

describe("addNote", () => {
  it("appends a fresh note and makes it active", () => {
    const { notes, activeId } = addNote(seed("A"), 2, 999);
    expect(notes).toHaveLength(2);
    expect(notes[1]!.id).toBe("note-2");
    expect(activeId).toBe("note-2");
  });
});

describe("renameNote", () => {
  it("renames and bumps updatedAt", () => {
    const out = renameNote(seed("A", "B"), "note-2", "  Deploy  ", 777);
    expect(out[1]!.title).toBe("Deploy");
    expect(out[1]!.updatedAtMs).toBe(777);
    expect(out[0]).toEqual(seed("A", "B")[0]);
  });
  it("falls back to Untitled for a blank title", () => {
    const out = renameNote(seed("A"), "note-1", "   ", 1);
    expect(out[0]!.title).toBe(UNTITLED);
  });
});

describe("updateBody", () => {
  it("replaces the body and bumps updatedAt of only the target", () => {
    const out = updateBody(seed("A", "B"), "note-1", "hello", 42);
    expect(out[0]!.body).toBe("hello");
    expect(out[0]!.updatedAtMs).toBe(42);
    expect(out[1]!.body).toBe("");
    expect(out[1]!.updatedAtMs).toBe(100);
  });
});

describe("deleteNote", () => {
  it("removes by id", () => {
    expect(deleteNote(seed("A", "B"), "note-1").map((n) => n.id)).toEqual(["note-2"]);
  });
});

describe("resolveActiveId", () => {
  it("returns undefined for an empty list", () => {
    expect(resolveActiveId([], "note-1")).toBeUndefined();
  });
  it("keeps a stored id that still exists", () => {
    expect(resolveActiveId(seed("A", "B"), "note-2")).toBe("note-2");
  });
  it("falls back to the first note when the stored id is gone or absent", () => {
    expect(resolveActiveId(seed("A", "B"), "note-9")).toBe("note-1");
    expect(resolveActiveId(seed("A", "B"), null)).toBe("note-1");
    expect(resolveActiveId(seed("A", "B"), undefined)).toBe("note-1");
  });
});

describe("nextActiveAfterDelete", () => {
  const notes = seed("A", "B", "C"); // note-1, note-2, note-3
  it("leaves the active tab alone when a different note is deleted", () => {
    expect(nextActiveAfterDelete(notes, "note-1", "note-3")).toBe("note-3");
  });
  it("slides to the note that takes the deleted index", () => {
    // Delete the active middle note → the note now at that index (C) leads.
    expect(nextActiveAfterDelete(notes, "note-2", "note-2")).toBe("note-3");
  });
  it("slides to the new last note when the active last note is deleted", () => {
    expect(nextActiveAfterDelete(notes, "note-3", "note-3")).toBe("note-2");
  });
  it("returns undefined when the last note is deleted", () => {
    expect(nextActiveAfterDelete(seed("A"), "note-1", "note-1")).toBeUndefined();
  });
});

describe("sendToAgentTitle", () => {
  it("labels the agent panel with the note title", () => {
    expect(sendToAgentTitle(seed("Deploy")[0]!)).toBe("Note: Deploy");
  });
});

describe("active-id persistence", () => {
  it("round-trips through a storage stub", () => {
    const store = new Map<string, string>();
    const storage = {
      getItem: (k: string) => store.get(k) ?? null,
      setItem: (k: string, v: string) => void store.set(k, v),
    };
    expect(loadActiveId(storage)).toBeUndefined();
    saveActiveId(storage, "note-5");
    expect(loadActiveId(storage)).toBe("note-5");
  });
  it("never throws when storage is unavailable", () => {
    const boom = {
      getItem: () => {
        throw new Error("nope");
      },
      setItem: () => {
        throw new Error("nope");
      },
    };
    expect(loadActiveId(boom)).toBeUndefined();
    expect(() => saveActiveId(boom, "x")).not.toThrow();
  });
});

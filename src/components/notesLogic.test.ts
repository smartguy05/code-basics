import { describe, expect, it } from "vitest";
import type { Note } from "../ipc/types";
import {
  addNote,
  deleteNote,
  flushDelay,
  loadActiveId,
  loadPillColor,
  makeNote,
  nextActiveAfterDelete,
  renameNote,
  resolveActiveId,
  saveActiveId,
  savePillColor,
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

describe("pill-color persistence", () => {
  const makeStore = () => {
    const store = new Map<string, string>();
    return {
      store,
      storage: {
        getItem: (k: string) => store.get(k) ?? null,
        setItem: (k: string, v: string) => void store.set(k, v),
        removeItem: (k: string) => void store.delete(k),
      },
    };
  };

  it("round-trips a colour through a storage stub", () => {
    const { storage } = makeStore();
    expect(loadPillColor(storage)).toBeUndefined();
    savePillColor(storage, "#7a4b00");
    expect(loadPillColor(storage)).toBe("#7a4b00");
  });

  it("clears back to the default when saved undefined", () => {
    const { storage } = makeStore();
    savePillColor(storage, "#123456");
    savePillColor(storage, undefined);
    expect(loadPillColor(storage)).toBeUndefined();
  });

  it("reads an empty stored value as no colour", () => {
    const { store, storage } = makeStore();
    store.set("cb.notes.pillColor", "");
    expect(loadPillColor(storage)).toBeUndefined();
  });

  it("never throws when storage is unavailable", () => {
    const boom = {
      getItem: () => {
        throw new Error("nope");
      },
      setItem: () => {
        throw new Error("nope");
      },
      removeItem: () => {
        throw new Error("nope");
      },
    };
    expect(loadPillColor(boom)).toBeUndefined();
    expect(() => savePillColor(boom, "#fff")).not.toThrow();
    expect(() => savePillColor(boom, undefined)).not.toThrow();
  });
});

describe("flushDelay", () => {
  const DEBOUNCE = 400;
  const MAX_WAIT = 1500;

  it("waits the full debounce when the pending write is young", () => {
    // First keystroke of a burst: nothing is overdue, so debounce normally.
    expect(flushDelay(1000, 1000, DEBOUNCE, MAX_WAIT)).toBe(DEBOUNCE);
    expect(flushDelay(1000, 1200, DEBOUNCE, MAX_WAIT)).toBe(DEBOUNCE);
  });

  it("flushes immediately once the pending write exceeds the max wait", () => {
    // Continuous typing past the cap must not defer the write forever.
    expect(flushDelay(1000, 1000 + MAX_WAIT, DEBOUNCE, MAX_WAIT)).toBe(0);
    expect(flushDelay(1000, 5000, DEBOUNCE, MAX_WAIT)).toBe(0);
  });

  it("caps the debounce so the write never lands after the max wait", () => {
    // 200 ms into the window: a full 400 ms debounce would overshoot 1500 ms
    // only near the end — here it clamps to the remaining budget.
    expect(flushDelay(1000, 1000 + (MAX_WAIT - 200), DEBOUNCE, MAX_WAIT)).toBe(200);
  });

  it("never returns a negative delay", () => {
    expect(flushDelay(1000, 10_000, DEBOUNCE, MAX_WAIT)).toBeGreaterThanOrEqual(0);
  });
});

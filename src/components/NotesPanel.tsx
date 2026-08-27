import { useEffect, useRef, useState } from "react";
import * as api from "../ipc/api";
import type { Note } from "../ipc/types";
import {
  clampPanelPosition,
  clampPanelSize,
  createResizeGate,
  loadPanelLayout,
  savePanelLayout,
  type PanelLayout,
  type PanelSize,
} from "./reviewLayoutLogic";
import {
  addNote,
  deleteNote,
  flushDelay,
  loadActiveId,
  nextActiveAfterDelete,
  NOTES_LAYOUT_KEY,
  renameNote,
  resolveActiveId,
  saveActiveId,
  updateBody,
} from "./notesLogic";

/** How long after the last keystroke the notes are written to disk. */
const AUTOSAVE_MS = 400;

/**
 * The longest a change may sit unsaved in memory. The trailing debounce alone
 * would defer the write forever while the user types continuously; this cap
 * forces a flush so a crash loses at most this much. Combined with the atomic,
 * never-truncating write on the Rust side, notes survive a crash without any
 * explicit save.
 */
const AUTOSAVE_MAX_WAIT_MS = 1500;

/** Next sequence number for {@link addNote}: one past the highest `note-N` id. */
function nextSeq(notes: Note[]): number {
  let max = 0;
  for (const n of notes) {
    const m = /^note-(\d+)$/.exec(n.id);
    if (m) max = Math.max(max, Number(m[1]));
  }
  return max + 1;
}

/**
 * The floating Notes / scratchpad panel — one panel, several named notes.
 *
 * A DOM floating panel modelled on `TerminalPanel`/`ReviewPanel`: draggable by
 * its header, resizable by the native grip, minimizing to a thin labeled bar
 * rather than closing. Hosted at app level so it survives tab switches. Notes are
 * **user-global** (`%APPDATA%/code-basics/notes.json`), so the same scratchpad is
 * available in every workspace.
 *
 * Every decision (create/rename/delete, active-tab selection, persistence keys)
 * lives in the pure, tested `notesLogic`; this shell only wires the DOM.
 */
export function NotesPanel({
  onClose,
  onSendToAgent,
}: {
  onClose: () => void;
  /** Run the active note's text as an agent prompt (opens the Review panel). */
  onSendToAgent: (note: Note) => void;
}) {
  const panelRef = useRef<HTMLDivElement>(null);

  const [notes, setNotes] = useState<Note[]>([]);
  const [activeId, setActiveId] = useState<string | undefined>();
  const [renamingId, setRenamingId] = useState<string | null>(null);
  const [savedFlash, setSavedFlash] = useState<string | null>(null);
  const [minimized, setMinimized] = useState(false);

  const seqRef = useRef(1);
  const saveTimer = useRef<ReturnType<typeof setTimeout> | undefined>(undefined);
  // When the oldest still-unsaved change was made, so the debounce can be capped
  // (see AUTOSAVE_MAX_WAIT_MS). Null when nothing is pending.
  const pendingSince = useRef<number | null>(null);
  // The latest notes, so the unmount flush writes what is on screen without
  // re-subscribing the effect.
  const latestRef = useRef<Note[]>([]);
  latestRef.current = notes;

  const [pos, setPos] = useState<PanelLayout | undefined>(() => {
    const saved = loadPanelLayout(localStorage, NOTES_LAYOUT_KEY);
    return saved.left !== undefined && saved.top !== undefined ? saved : undefined;
  });
  const [size] = useState<PanelSize | undefined>(() => {
    const saved = loadPanelLayout(localStorage, NOTES_LAYOUT_KEY);
    return saved.width !== undefined && saved.height !== undefined
      ? { width: saved.width, height: saved.height }
      : undefined;
  });

  // Load the notes once, and seed the sequence + active tab from them.
  useEffect(() => {
    let alive = true;
    void api
      .readNotes()
      .then((file) => {
        if (!alive) return;
        setNotes(file.notes);
        seqRef.current = nextSeq(file.notes);
        setActiveId(resolveActiveId(file.notes, loadActiveId(localStorage)));
      })
      .catch(() => {
        // A store that will not read leaves an empty scratchpad, not an error —
        // the same tolerance the backend applies.
      });
    return () => {
      alive = false;
      // Flush any pending write so a close does not lose the last keystrokes.
      if (saveTimer.current) {
        clearTimeout(saveTimer.current);
        void api.writeNotes({ version: 1, notes: latestRef.current }).catch(() => {});
      }
    };
  }, []);

  // Flush a pending write when the window tears down (close, reload) — the
  // React unmount cleanup above only runs on a graceful panel close, and misses
  // the app quitting with the panel still open. Best effort: fire-and-forget,
  // the timing of a teardown does not let us await it.
  useEffect(() => {
    const flush = () => {
      if (!saveTimer.current) return; // nothing pending
      clearTimeout(saveTimer.current);
      saveTimer.current = undefined;
      pendingSince.current = null;
      void api.writeNotes({ version: 1, notes: latestRef.current }).catch(() => {});
    };
    window.addEventListener("pagehide", flush);
    window.addEventListener("beforeunload", flush);
    return () => {
      window.removeEventListener("pagehide", flush);
      window.removeEventListener("beforeunload", flush);
    };
  }, []);

  // Persist the size the user drags the grip to (see ReviewPanel for the gate
  // reasoning). Shared key, so the panel reopens at its last size.
  useEffect(() => {
    const panel = panelRef.current;
    if (!panel || typeof ResizeObserver !== "function") return;
    const gate = createResizeGate();
    let timer: ReturnType<typeof setTimeout> | undefined;
    const observer = new ResizeObserver(() => {
      const width = panel.offsetWidth;
      const height = panel.offsetHeight;
      if (!gate.persist({ width, height })) return;
      if (timer) clearTimeout(timer);
      timer = setTimeout(() => {
        const clamped = clampPanelSize(
          { width, height },
          { width: window.innerWidth, height: window.innerHeight },
        );
        const saved = loadPanelLayout(localStorage, NOTES_LAYOUT_KEY);
        savePanelLayout(localStorage, { ...saved, ...clamped }, NOTES_LAYOUT_KEY);
      }, 200);
    });
    observer.observe(panel);
    return () => {
      if (timer) clearTimeout(timer);
      observer.disconnect();
    };
  }, []);

  const scheduleSave = (next: Note[]) => {
    if (saveTimer.current) clearTimeout(saveTimer.current);
    if (pendingSince.current === null) pendingSince.current = Date.now();
    const delay = flushDelay(pendingSince.current, Date.now(), AUTOSAVE_MS, AUTOSAVE_MAX_WAIT_MS);
    saveTimer.current = setTimeout(() => {
      saveTimer.current = undefined;
      pendingSince.current = null;
      void api.writeNotes({ version: 1, notes: next }).catch(() => {});
    }, delay);
  };

  // Every note mutation goes through here: update the UI and schedule a write.
  const commit = (next: Note[]) => {
    setNotes(next);
    scheduleSave(next);
  };

  const chooseActive = (id: string) => {
    setActiveId(id);
    saveActiveId(localStorage, id);
  };

  const active = notes.find((n) => n.id === activeId);

  const onAdd = () => {
    const { notes: next, activeId: id } = addNote(notes, seqRef.current, Date.now());
    seqRef.current += 1;
    commit(next);
    chooseActive(id);
    setRenamingId(id);
  };

  const onDelete = (id: string) => {
    const nextActive = nextActiveAfterDelete(notes, id, activeId);
    commit(deleteNote(notes, id));
    setActiveId(nextActive);
    if (nextActive) saveActiveId(localStorage, nextActive);
  };

  const onRename = (id: string, title: string) => commit(renameNote(notes, id, title, Date.now()));

  const onBody = (body: string) => {
    if (!active) return;
    commit(updateBody(notes, active.id, body, Date.now()));
  };

  const onSaveInstruction = () => {
    if (!active) return;
    void api
      .saveNoteAsInstruction(active.title, active.body)
      .then(() => setSavedFlash("Saved. Add it via Enhancements → Add Instructions."))
      .catch((e) => setSavedFlash(`Could not save: ${e}`));
  };

  // Drag by the header — identical to TerminalPanel/ReviewPanel; the clamp is
  // pure. A press that never moves stays a click, so the buttons keep working.
  const onHeaderPointerDown = (e: React.PointerEvent<HTMLDivElement>) => {
    if (e.button !== 0) return;
    if ((e.target as HTMLElement).closest("button, input, textarea")) return;
    const panel = panelRef.current;
    if (!panel) return;

    const rect = panel.getBoundingClientRect();
    const grabX = e.clientX - rect.left;
    const grabY = e.clientY - rect.top;
    const header = e.currentTarget;
    header.setPointerCapture(e.pointerId);

    let latest: PanelLayout = { left: rect.left, top: rect.top };
    let moved = false;
    const onMove = (ev: PointerEvent) => {
      moved = true;
      const s = { width: panel.offsetWidth, height: panel.offsetHeight };
      const viewport = { width: window.innerWidth, height: window.innerHeight };
      latest = clampPanelPosition({ left: ev.clientX - grabX, top: ev.clientY - grabY }, s, viewport);
      setPos(latest);
    };
    const onUp = () => {
      header.releasePointerCapture(e.pointerId);
      header.removeEventListener("pointermove", onMove);
      header.removeEventListener("pointerup", onUp);
      if (moved) savePanelLayout(localStorage, latest, NOTES_LAYOUT_KEY);
    };
    header.addEventListener("pointermove", onMove);
    header.addEventListener("pointerup", onUp);
  };

  return (
    <>
      {minimized && (
        <button
          className="review-pill notes-pill"
          onClick={() => setMinimized(false)}
          title="Restore notes"
        >
          <span>Notes</span>
        </button>
      )}

      <div
        className="review-panel notes-panel"
        hidden={minimized}
        ref={panelRef}
        style={{
          ...(pos ? { left: pos.left, top: pos.top, right: "auto", bottom: "auto" } : {}),
          ...(size ? { width: size.width, height: size.height } : {}),
        }}
      >
        <div className="review-header" onPointerDown={onHeaderPointerDown}>
          <strong>Notes</strong>
          <span style={{ flex: 1 }} />
          <button onClick={() => setMinimized(true)} title="Minimize (keeps your notes)">
            —
          </button>
          <button onClick={onClose} title="Close">
            ✕
          </button>
        </div>

        <div className="notes-tabs">
          {notes.map((n) =>
            renamingId === n.id ? (
              <input
                key={n.id}
                className="notes-tab-rename"
                autoFocus
                defaultValue={n.title}
                onBlur={(e) => {
                  onRename(n.id, e.target.value);
                  setRenamingId(null);
                }}
                onKeyDown={(e) => {
                  if (e.key === "Enter") {
                    onRename(n.id, (e.target as HTMLInputElement).value);
                    setRenamingId(null);
                  } else if (e.key === "Escape") {
                    setRenamingId(null);
                  }
                }}
              />
            ) : (
              <span
                key={n.id}
                className={`notes-tab${n.id === activeId ? " active" : ""}`}
                onClick={() => chooseActive(n.id)}
                onDoubleClick={() => setRenamingId(n.id)}
                title="Click to open, double-click to rename"
              >
                {n.title}
                <button
                  className="notes-tab-close"
                  onClick={(e) => {
                    e.stopPropagation();
                    onDelete(n.id);
                  }}
                  title="Delete this note"
                >
                  ✕
                </button>
              </span>
            ),
          )}
          <button className="notes-tab-add" onClick={onAdd} title="New note">
            +
          </button>
        </div>

        {active ? (
          <>
            <textarea
              className="notes-editor"
              value={active.body}
              placeholder="Jot a note, a reminder, a prompt for later…"
              onChange={(e) => onBody(e.target.value)}
            />
            <div className="notes-footer">
              <button onClick={() => onSendToAgent(active)} title="Run this note as an agent prompt">
                Send to agent ▶
              </button>
              <button onClick={onSaveInstruction} title="Save this note into the instruction library">
                Save as instruction
              </button>
              {savedFlash && (
                <span className="faint" style={{ fontSize: 12, alignSelf: "center" }}>
                  {savedFlash}
                </span>
              )}
            </div>
          </>
        ) : (
          <div className="notes-empty">
            No notes yet. Click <strong>+</strong> to start one.
          </div>
        )}
      </div>
    </>
  );
}

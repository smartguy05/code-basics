import { useCallback, useEffect, useRef, useState } from "react";
import * as api from "../ipc/api";
import type { Task, TasksFile, Workspace } from "../ipc/types";
import { useDockEntry } from "./DockContext";
import { dockId } from "./dockLogic";
import {
  clampPanelPosition,
  clampPanelSize,
  createResizeGate,
  loadPanelLayout,
  savePanelLayout,
  type PanelLayout,
  type PanelSize,
} from "./reviewLayoutLogic";
import { TASKS_LAYOUT_KEY } from "./tasksPanelLogic";
import {
  canAddTask,
  cleanTaskTitle,
  nextSelectedAfterDelete,
  ownerLabel,
  taskPrompt,
  toggledStatus,
} from "./tasksLogic";

/**
 * The per-codebase task list as a floating window, modelled on {@link SqlPanel}
 * and {@link NotesPanel}: one panel per codebase, minimizing to a dock pill
 * rather than closing, re-opened from the chrome by a request token.
 *
 * The store is the **backend's** (`cb_core::tasks`, `.code-basics/tasks.json`,
 * gitignored): every mutation goes through a `commands/tasks.rs` command that
 * returns the whole updated `TasksFile`, so this panel re-renders from the
 * persisted truth rather than keeping an optimistic copy. What decisions it does
 * make — the agent prompt, the status/owner toggles, the selection after a
 * delete — are the pure, tested helpers in {@link tasksLogic}.
 *
 * # Minimized means hidden, not unmounted
 *
 * `hidden` on the panel keeps it mounted while out of sight (an in-progress edit
 * in a textarea survives); switching the *feature* off is the caller's job and
 * unmounts it (`tasksPanelMounted` in `WorkspaceTab`).
 */
export function TasksPanel({
  workspace,
  onClose,
  restoreRequest,
  onLaunchAgent,
}: {
  workspace: Workspace;
  onClose: () => void;
  /**
   * Changes on every request to open the panel (the titlebar/Plugins action or
   * the `plugin.tasks` command). A minimized panel restores itself when it
   * changes — a boolean could not say "open it again" about an open panel.
   */
  restoreRequest: number;
  /**
   * Launch the coding agent in an interactive terminal with `prompt`. Owned by
   * `WorkspaceTab` (it holds the terminal machinery and the agent picker); the
   * panel only decides *what* to say, via {@link taskPrompt}.
   */
  onLaunchAgent: (prompt: string) => void;
}) {
  const panelRef = useRef<HTMLDivElement>(null);
  const [minimized, setMinimized] = useState(false);
  const [file, setFile] = useState<TasksFile | null>(null);
  const [selectedId, setSelectedId] = useState<string | undefined>(undefined);
  const [draftTitle, setDraftTitle] = useState("");
  const [error, setError] = useState<string | null>(null);

  const tasks = file?.tasks ?? [];

  useEffect(() => setMinimized(false), [restoreRequest]);

  // Load the store on mount (and whenever the codebase changes, though this
  // instance is keyed per root by `WorkspaceTab`, so that is once).
  useEffect(() => {
    let live = true;
    api
      .readTasks(workspace.root)
      .then((f) => {
        if (live) setFile(f);
      })
      .catch((e) => {
        if (live) setError(String(e));
      });
    return () => {
      live = false;
    };
  }, [workspace.root]);

  const refresh = useCallback((f: TasksFile) => {
    setFile(f);
    setError(null);
  }, []);
  const fail = useCallback((e: unknown) => setError(String(e)), []);

  const add = () => {
    if (!canAddTask(draftTitle)) return;
    api
      .createTask(workspace.root, cleanTaskTitle(draftTitle), "")
      .then((f) => {
        refresh(f);
        setDraftTitle("");
      })
      .catch(fail);
  };

  const complete = (task: Task) => {
    api
      .completeTask(workspace.root, task.id, toggledStatus(task.status))
      .then(refresh)
      .catch(fail);
  };

  const saveTitle = (task: Task, value: string) => {
    const clean = cleanTaskTitle(value);
    if (clean === task.title) return;
    api.updateTask(workspace.root, task.id, clean, task.body).then(refresh).catch(fail);
  };

  const saveBody = (task: Task, value: string) => {
    if (value === task.body) return;
    api.updateTask(workspace.root, task.id, task.title, value).then(refresh).catch(fail);
  };

  const remove = (task: Task) => {
    const next = nextSelectedAfterDelete(tasks, task.id, selectedId);
    api
      .deleteTask(workspace.root, task.id)
      .then((f) => {
        refresh(f);
        setSelectedId(next);
      })
      .catch(fail);
  };

  // Assign to AI: record the owner, then launch the agent with the task text.
  // The persist happens first so the AI owner is durable even if the terminal
  // fails to spawn — the task is still "the AI's" and re-runnable.
  const assignAi = (task: Task) => {
    api
      .assignTask(workspace.root, task.id, "ai")
      .then((f) => {
        refresh(f);
        onLaunchAgent(taskPrompt(task));
      })
      .catch(fail);
  };

  const assignMe = (task: Task) => {
    api.assignTask(workspace.root, task.id, "me").then(refresh).catch(fail);
  };

  const restore = useCallback(() => setMinimized(false), []);
  useDockEntry(
    minimized
      ? {
          id: dockId(workspace.root, "tasks"),
          scope: workspace.root,
          label: "Tasks",
          order: 0,
          status: workspace.name,
          onRestore: restore,
        }
      : null,
  );

  const [pos, setPos] = useState<PanelLayout | undefined>(() => {
    const saved = loadPanelLayout(localStorage, TASKS_LAYOUT_KEY);
    return saved.left !== undefined && saved.top !== undefined ? saved : undefined;
  });
  const [size] = useState<PanelSize | undefined>(() => {
    const saved = loadPanelLayout(localStorage, TASKS_LAYOUT_KEY);
    return saved.width !== undefined && saved.height !== undefined
      ? { width: saved.width, height: saved.height }
      : undefined;
  });

  // Persist the size the user drags the native grip to. The gate stops the mount
  // default and the 0×0 a minimize produces from being written over a real size.
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
        const saved = loadPanelLayout(localStorage, TASKS_LAYOUT_KEY);
        savePanelLayout(localStorage, { ...saved, ...clamped }, TASKS_LAYOUT_KEY);
      }, 200);
    });
    observer.observe(panel);
    return () => {
      if (timer) clearTimeout(timer);
      observer.disconnect();
    };
  }, []);

  // Drag by the header — identical to SqlPanel/NotesPanel; the clamp is pure.
  const onHeaderPointerDown = (e: React.PointerEvent<HTMLDivElement>) => {
    if (e.button !== 0) return;
    if ((e.target as HTMLElement).closest("button, input, textarea, select")) return;
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
      if (moved) savePanelLayout(localStorage, latest, TASKS_LAYOUT_KEY);
    };
    header.addEventListener("pointermove", onMove);
    header.addEventListener("pointerup", onUp);
  };

  return (
    <div
      className="review-panel tasks-panel"
      hidden={minimized}
      ref={panelRef}
      style={{
        ...(pos ? { left: pos.left, top: pos.top, right: "auto", bottom: "auto" } : {}),
        ...(size ? { width: size.width, height: size.height } : {}),
      }}
    >
      <div className="review-header" onPointerDown={onHeaderPointerDown}>
        <strong>Tasks</strong>
        <span className="faint" style={{ fontSize: 12 }}>
          {workspace.name}
        </span>
        <span style={{ flex: 1 }} />
        <button onClick={() => setMinimized(true)} title="Minimize">
          —
        </button>
        <button onClick={onClose} title="Close">
          ✕
        </button>
      </div>

      <div
        className="tasks-body"
        style={{ display: "flex", flexDirection: "column", gap: 8, padding: 8, overflow: "auto", flex: 1 }}
      >
        <div style={{ display: "flex", gap: 6 }}>
          <input
            value={draftTitle}
            placeholder="New task…"
            onChange={(e) => setDraftTitle(e.target.value)}
            onKeyDown={(e) => {
              if (e.key === "Enter") {
                e.preventDefault();
                add();
              }
            }}
            style={{ flex: 1 }}
          />
          <button disabled={!canAddTask(draftTitle)} onClick={add}>
            Add
          </button>
        </div>

        {error !== null && <div className="ask-error">{error}</div>}

        {tasks.length === 0 && <div className="faint">No tasks yet.</div>}

        {tasks.map((task) => {
          const done = task.status === "done";
          const expanded = selectedId === task.id;
          return (
            <div
              key={task.id}
              style={{ border: "1px solid var(--border)", borderRadius: 6, padding: 8 }}
            >
              <div style={{ display: "flex", gap: 6, alignItems: "center" }}>
                <input
                  type="checkbox"
                  checked={done}
                  title={done ? "Reopen" : "Mark done"}
                  onChange={() => complete(task)}
                />
                <input
                  key={`${task.id}:${task.updatedAtMs}:title`}
                  defaultValue={task.title}
                  onBlur={(e) => saveTitle(task, e.target.value)}
                  style={{
                    flex: 1,
                    textDecoration: done ? "line-through" : "none",
                    opacity: done ? 0.6 : 1,
                  }}
                />
                <span
                  title={`Assigned to ${ownerLabel(task.owner)}`}
                  style={{
                    fontSize: 11,
                    padding: "1px 6px",
                    borderRadius: 10,
                    border: "1px solid var(--border)",
                    background: task.owner === "ai" ? "var(--accent-soft, transparent)" : "transparent",
                  }}
                >
                  {ownerLabel(task.owner)}
                </span>
                <button
                  title={expanded ? "Collapse" : "Details"}
                  onClick={() => setSelectedId(expanded ? undefined : task.id)}
                >
                  {expanded ? "▾" : "▸"}
                </button>
                <button title="Delete" onClick={() => remove(task)}>
                  ✕
                </button>
              </div>

              {expanded && (
                <div style={{ marginTop: 6 }}>
                  <textarea
                    key={`${task.id}:${task.updatedAtMs}:body`}
                    defaultValue={task.body}
                    placeholder="Details… (also the prompt when assigned to the AI)"
                    onBlur={(e) => saveBody(task, e.target.value)}
                    style={{ width: "100%", minHeight: 60, boxSizing: "border-box" }}
                  />
                  <div style={{ display: "flex", gap: 6, marginTop: 6 }}>
                    {task.owner !== "ai" && (
                      <button
                        onClick={() => assignAi(task)}
                        title="Assign to the AI and launch it in a terminal with this task"
                      >
                        Assign to AI
                      </button>
                    )}
                    {task.owner !== "me" && (
                      <button onClick={() => assignMe(task)} title="Assign back to yourself">
                        Assign to me
                      </button>
                    )}
                  </div>
                </div>
              )}
            </div>
          );
        })}
      </div>
    </div>
  );
}

# Work item: Running-processes panel + crash-orphan detection

Global panel listing everything the app is currently running (runs/builds/terminals/
review/behavioral) across all open codebases, each with Kill, plus a "Possible orphans"
section for processes spawned in a previous/crashed session that are still alive.

Motivation: terminals + runs now spawn headless (no console window), removing the visual
cue that something is still running.

Plan: C:\Users\AnthonyJames\.claude\plans\right-now-when-i-glimmering-hamster.md

Key design:
- cb-core `running/` module: RunningRecord{pid,kind,label,root,key,program,startedAtMs},
  RunningStore (Arc<Mutex>, write-through to user-global running.json like notes.rs),
  probe.rs (sysinfo — NEW dep — liveness+identity), classify.rs (pure orphan decision).
- Threaded through spawn points: Supervisor::run_tracked + PtyManager::open_tracked record
  on insert / remove on reap. Store injected via AppState into per-slot supervisors, the
  global supervisor, and pty.
- Startup: store.init() loads file, probes each pid, keeps alive+identity-matching as
  orphans, prunes rest.
- Commands: list_running, kill_running (routes to cancel/close/kill_tree by pid);
  terminal_open gains label + terminal_set_label for rename freshness.
- Frontend: global RunningPanel.tsx (titlebar button, open/close, NOT minimizable),
  runningLogic.ts (pure), polls list_running ~1.5s.

PID-reuse safety: orphan surfaced/killed only when live pid identity (name+start time)
matches the record; abstain+warn otherwise.

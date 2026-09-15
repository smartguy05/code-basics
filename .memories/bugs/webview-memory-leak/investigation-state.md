# Investigation state — paused 2026-09-11, resume Monday

## STATUS: reported leak NOT fixed yet. Diagnosis shifted mid-investigation.

The three fixes already implemented (see `completed.md`) are **real, valid leak
fixes** and pass all gates — but reproduction proved **none of them is the leak
the user actually hit**. Keep them; they are improvements. The real leak is still
open.

## What the user reports
- App RAM climbs to **11-13 GB** and OOM-crashes. User calls it a "webview" leak.
- Trigger: **opening the Intent view** (originally described as "performing git
  actions"). Reproduced in BOTH the installed 1.6.x build AND the dev build (which
  has my three fixes) — so it is a **shipped bug my fixes did not touch**.

## Hard evidence gathered (measurements, not theory)
1. **It is NOT the webview.** During the climb, `msedgewebview2` stayed flat at
   ~1.9 GB. The process that hit **13 GB is `cb-app.exe` (the Rust backend)**.
   (Sampler CSVs were in the session scratchpad: `memsample2.csv` showed cb-app
   climbing 1522MB→13GB over ~60s at 16:42-16:43, oscillating 2-13GB every few
   seconds, then crashing to a 2MB husk.)
2. **The oscillation matters:** value bounces (4462→2254→6266→10371→12981 MB) —
   Rust frees on drop, so this is a **repeated multi-GB TRANSIENT allocation
   every ~2s** (matches the 2s Changes poll), occasionally overlapping to OOM —
   NOT a slow monotonic accumulation.
3. **The core intent path is CHEAP.** Headless repro (`intents::load` +
   `attribution::attribute` + `coverage::review` + `providers::statuses`, 200
   iterations against the real 12MB log + real repo) stayed flat at **13-40 MB**,
   ~80ms/iter. So `intent_groups` and `find_sessions`/`statuses` are NOT the hog.
4. Working-tree diff is tiny (767 insertions, 12 files) — not data volume.
5. `import_intent_history` (which DOES mine all ~425MB of transcripts) is a
   **button** (`ChangesView.tsx:345`), not on the poll.

## THE CONFOUND to resolve first on Monday
My samplers grabbed the **largest cb-app process**. There are several: the dev app
(`target\debug\cb-app.exe`) AND my own agent's hook processes
(`C:\Program Files\code-basics\cb-app.exe record-intent` / `quality-gate`), which
fire on every one of MY tool calls (user-scope intent capture is installed). At
16:42 I was actively running tools, so **the 13GB process may have been one of MY
`record-intent` Stop/PostToolUse hooks mining this session's huge transcript, not
the dev app.** This session's transcript is hundreds of MB.

This is plausibly the REAL bug too: the user runs Claude Code, and the intent
Stop/why hooks fire right after agent runs — exactly when they "do git actions /
open the intent view." So a `record-intent`/`whyhook` path that loads a large
transcript into memory would balloon for them too.

## NEXT STEPS (Monday), in order
1. **Isolate by exe path.** Re-sample filtering to `target\debug\cb-app.exe` ONLY
   (exclude `Program Files`), and reproduce while the AGENT IS IDLE (I run no
   tools during the user's repro) so my hooks don't contaminate. This definitively
   says whether the dev app itself balloons.
2. **Audit the record/why hook path for transcript loading** — the live lead when
   paused. Was reading `src-tauri/src/recorder.rs` `ingest()` (line ~117:
   `hook::ingest` then `hook::ask_for_intent`). Check `intents/hook.rs::ingest`
   and `ask_for_intent`, and `whyhook.rs`, for `read_to_string` / mining of the
   **session transcript** (Claude Code `~/.claude/projects/<enc>/*.jsonl`). A
   `read_to_string` of a hundreds-of-MB transcript, or parsing it to serde `Value`
   (~5-10x), on every PostToolUse/Stop = the multi-GB transient. NOTE: `hook.rs`
   and `recorder.rs` have UNCOMMITTED changes from the intent-duplicate bug — read
   the current on-disk version.
3. If confirmed, fix = **stream/cap the transcript read** (don't `read_to_string`
   the whole file; read incrementally with a byte cap, or only the needed
   tail/turn), analogous to the `transcript_cwd` concern noted below.
4. **Also suspect `find_sessions`/`transcript_cwd`** (`claude_code.rs:219/270`):
   `transcript_cwd` does `read_to_string`-free line reading but parses each of up
   to 40 lines to serde `Value`; a transcript line can be many MB. Cheap in the
   200x repro (returns on first line with `cwd`), but worth capping per-line bytes
   defensively.
5. Re-check whether the DEV app (clean of my hooks) actually balloons on the
   intent view. If it does NOT, the user's leak IS the hook path (step 2) and the
   fix ships in the installed binary (rebuild + reinstall).

## Files with the leads
- `src-tauri/src/recorder.rs` (ingest, ~line 117) — UNCOMMITTED changes present
- `crates/core/src/intents/hook.rs` (`ingest`, `ask_for_intent`) — UNCOMMITTED
- `crates/core/src/intents/whyhook.rs` (reads hook config files, not transcripts —
  but check `why::record_note` and what it loads)
- `crates/core/src/intents/providers/claude_code.rs` (`history` line 204 mines
  transcripts; `find_sessions` 219; `transcript_cwd` 270)

## Do NOT re-do
- Don't re-blame the frontend (webview is flat).
- Don't re-blame `intent_groups`/attribution/`statuses` (measured cheap).
- Don't hand-edit the live 12MB `edits.jsonl` (old app appends concurrently).

# Completed: Intent tab drives code-basics to ~11 GB RAM

## Symptom (reported while using the app)

Opening the **Intent** tab on a large, long-lived repo (ONEflight) pushed
`code-basics.exe` to ~11 GB. (rust-analyzer's separate ~3 GB is normal, not this
bug.) Only happened on a repo with lots of history and registered intents.

## Root cause (multiple, compounding)

Measured against `../ONEflight/.code-basics/intents`: `edits.jsonl` 10 MB / 2553
records across **38 branches**; current branch `staging` had **0** records;
`prune-state.json` highSeq 1,560,572. Frontend was innocent — `intent_groups`
returns only integer indices/counts, never line/file contents.

1. **`retire` ran the whole-store content check every ~2 s poll, ignoring
   branch.** `run_if_head_moved` (`intents/retire.rs`) called `execute` on every
   tick despite its name/comment; `execute` loads with `LoadOptions::default()`
   (no branch filter → all 2553 records). `verdict` re-split the HEAD blob and
   cloned **every line** into a fresh `Vec<String>` + `HashSet` **per record**, so
   a file with 531 records rebuilt its anchor set 531×. O(records × filesize),
   growing with every branch worked.
2. **Redundant full re-parses per tick**: `execute` → `intents::load` (uncached)
   + `next_seq` (`mod.rs`) each re-parsed the 10 MB log; plus a second
   `diff_all(WorkingToHead)`.
3. **564 whole-file records for gitignored `.memories/*.html`** — `.memories` is
   gitignored (0 tracked), so `verdict` keeps a whole-file record forever when the
   file is not in HEAD (`retire.rs`). Permanent, unbounded accumulation — the
   "stale since v0.1" fuel.
4. **Attribution cross-product** (`git/attribution.rs::find_candidates`): nested
   scan `diff_lines × records × record_lines`. Secondary (≤~1 GB with this data)
   but a real multiplier under a large working diff.

Windows' allocator does not return freed peaks to the OS, and the 2 s poll had no
in-flight guard, so overlapping slow ticks held RSS at ~11 GB.

## Fix

- **HEAD-move gate** (`retire.rs::should_run_content_check`): the content verdict
  runs only when HEAD moved since the last prune (or was never seen). Unchanged
  HEAD is the one-ref no-op the comment always claimed.
- **Per-path anchor set**: `plan` builds each file's HEAD anchor set once
  (`anchor_set_of_blob`) and shares it via `verdict_with`; `verdict` kept as a
  wrapper so its tests are unchanged. No more per-record blob reparse or the
  intermediate `Vec<String>` clone.
- **Archive stale branches + purge gitignored whole-file records**
  (`retire::plan_archive_stale` pure + `archive_stale` I/O, `live_branches`).
  Spawned off `open_workspace` (`commands/workspace.rs::spawn_archive_stale`).
  Branch-stale records archived (recoverable via re-mine); gitignored whole-file
  records archived **and tombstoned** (permanent). Added `Repo::is_path_ignored`.
- **Stop recording** gitignored whole-file intents at the source
  (`intents/hook.rs`, opens the repo once for branch + ignore; removed the now
  redundant `current_branch`).
- **`next_seq` re-parse removed from `execute`**: uses the already-loaded records'
  max seq (`file_high`) for the high-water mark.
- **Indexed `find_candidates`** (`attribution.rs::CandidateIndex`): per-file
  `[side][level]` HashMap in ascending `(record, line)` order, so output is
  identical to the old nested scan (all 64 retire+attribution tests pass).
- **Frontend poll in-flight guard** (`views/useChangesModel.ts::intentInFlight`):
  drops a tick while the previous `intentGroups` is still running.

## Files touched

`crates/core/src/intents/retire.rs` (+ retire_tests.rs), `intents/mod.rs`
(execute), `intents/hook.rs`, `git/attribution.rs`, `git/repo.rs`,
`src-tauri/src/commands/workspace.rs`, `src/views/useChangesModel.ts`.

## Verify

Full `cargo test -p cb-core` green; `pnpm typecheck` green. Live before/after:
open ONEflight on a feature branch with many records, click Intent, watch
`code-basics.exe` RAM hold steady across ticks; confirm `edits.jsonl` shrinks
after the archive (38 dead branches + gitignored `.memories` records move to
`edits-archive.jsonl`) and current-branch cards are unchanged.

## Follow-up: oversized untracked file (the residual 7-8 GB + minute-long stall)

After the intent fix, ONEflight on `staging` (0 intent records) still hit ~7-8 GB
and took a full minute to show any file's diff. Cause was unrelated to intents: a
**368 MB, 1.22M-line untracked `DevOps/Scripts/tmp/backup.sql`**. `diff_all`/
`file_diff` run with `recurse_untracked_dirs(true)` + `show_untracked_content(true)`,
and git only auto-flags a file binary when it looks binary or clears its 512 MB
`core.bigFileThreshold` — a large *text* dump slips through and is expanded to
1.22M `DiffLine` structs on every 2 s poll (diff_all is called by intent, erosion,
coverage, status), then copied again by attribution/grouping/erosion/IPC/CodeMirror.

Fix: `MAX_DIFF_FILE_BYTES = 5 MiB` (`git/repo.rs`) applied via
`DiffOptions::max_size(...)` in `file_diff`, `diff_all`, and `commit_diff`. git2
then reports an oversized file binary — no read, no hunks — so the whole pipeline
skips it, while ordinary generated sources (a 0.8 MB migration) still diff. Test:
`git_operations.rs::an_oversized_text_file_is_reported_binary_and_never_expanded`.
User-side mitigation regardless: gitignore/remove such dumps from the tree.

Lesson (cross-WI, worth CLAUDE.md if it recurs): git's binary heuristic does NOT
catch a large *text* file; any line-by-line diff path must cap file size itself.

## Related

[stale-committed-intents], [intent-duplicate-and-stale] (retirement design),
[webview-memory-leak] (other memory work).

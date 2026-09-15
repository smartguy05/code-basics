# TODOs — webview/backend memory leak

## Open (the real leak — resume Monday)
- [ ] Re-sample filtering to `target\debug\cb-app.exe` ONLY (exclude Program
      Files), agent idle, reproduce intent-view → confirm whether the DEV app
      itself balloons or if it was my hook processes. (See investigation-state.md)
- [ ] Audit `recorder.rs::ingest` → `hook::ingest`/`ask_for_intent` and
      `why::record_note` for loading the full session transcript into memory
      (`read_to_string` / serde `Value` on a hundreds-of-MB `.jsonl`). Prime suspect.
- [ ] Defensively cap `claude_code.rs::transcript_cwd` per-line byte reads.
- [ ] Once root cause found: fix, add failing-first test, rebuild release + reinstall.

## Done (valid fixes, NOT the reported leak — keep)
- [x] DiffView: in-place doc sync instead of MergeView rebuild on git mutations.
- [x] IntentCache: mtime+len parse cache for the 2s intent poll (+ tests).
- [x] Exact-duplicate-line compaction of edits/labels logs (+ tests).
- [x] Channel `onmessage` release at stream end, all 11 api.ts sites (+ tests).

## Verify before shipping any of the above
- [ ] `pnpm typecheck`, `pnpm test`, `cargo fmt --check`, `cargo test -p cb-core`
      (note: `redis::discover` test fails on an unrelated 8.3-short-path quirk).
- [ ] The double-install fix (hook.rs/recorder.rs, already uncommitted) + all
      backend fixes need **rebuild release + reinstall** to reach the running binary.

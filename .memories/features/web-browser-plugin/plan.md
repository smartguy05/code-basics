# Plan — Phase 4a (pure decisions only)

Engine decision is **settled**: raw `wry` (`wry = "0.55.1"` as a direct
`src-tauri` dependency — `tauri::wry` does **not** exist on Windows, it is
`#[cfg(target_os = "android")]`). Never a Tauri-created webview for remote
content: Tauri pushes `__TAURI_INTERNALS__` into every webview it creates and
`plugin:__TAURI_CHANNEL__|fetch` is exempt from the ACL origin check with no
check that the requesting webview owns the channel id.

## Modules, in dependency order

1. `model.rs`   — the wire types. Six availability variants, camelCase pinned.
2. `url.rs`     — `normalize_input`. Refuses rather than searches.
3. `origin.rs`  — `navigation_verdict`, `origin_of`, `same_origin`.
4. `ring.rs`    — bounded ring that counts drops and reports gaps.
5. `text.rs`    — `truncate_page_text`, reporting the total.
6. `console.rs` — `classify_level`; unknown is `Other`, never `Error`.
7. `script.rs`  — injected script bodies; try/catch + JSON-encoded arguments.
8. `ipc.rs`     — `parse_page_message`, the hostile-input boundary.
9. `consent.rs` — `decide(tool, consent, state)`.
10. `framing.rs` — **reuse** `crate::mcp::ndjson` (see notes.md); no second copy.

Plus `src/components/browserPanelLogic.ts` + test.

## Method

Test first, per module: write the test, run it, watch it fail for the right
reason, then implement. Mutation-check the rules that matter (break the rule,
watch the named test fail, revert).

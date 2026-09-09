# Related docs

- The plan: `~/.claude/plans/major-enchancements-add-witty-hopper.md` §4 (762-1016).
- `docs/architecture/ipc-contract.md` — the hand-mirrored Rust↔TS rule that
  `model.rs`'s key-pinning tests enforce.
- `docs/architecture/frontend.md` — panel/layout conventions.
- `crates/core/src/mcp/ndjson.rs` — the NDJSON framing this reuses, and its
  doc comment explaining why it is not `lsp/framing.rs`.
- `crates/core/src/lsp/model.rs` — `Availability`, the six-variant precedent.
- `crates/core/src/dap/model.rs` — `DebugState`, the same precedent for a state
  machine that must not collapse.
- `src/components/sqlPanelLogic.ts` — the `{open, restoreToken}` state machine
  this copies, including same-reference-on-no-op.
- `src/components/reviewLayoutLogic.ts` — `createResizeGate`, the 0x0 lesson.
- `docs/architecture/browser-panel.md` — written in 4b: the Tauri channel
  finding, the z-order compromise, the defence-in-depth list, and what wry
  0.55.1 does and does not expose.
- `src/components/SqlPanel.tsx` — the panel recipe `BrowserPanel` copies.
- `src-tauri/src/commands/about.rs` — the precedent for a wire type defined in
  `src-tauri` with its key-pinning test beside it; `BrowserSnapshot` follows it.

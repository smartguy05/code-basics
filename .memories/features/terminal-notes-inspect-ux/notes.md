# Notes / gotchas — terminal/notes/inspect UX

- **Windowless spawn is centralized.** `configure_process_group` (tokio Command, used by the
  Supervisor and LSP transport) now carries `CREATE_NO_WINDOW | CREATE_NEW_PROCESS_GROUP`.
  Raw `std::process::Command` sites that must NOT join a process group use the separate
  `no_window` helper. Keep the two distinct — a supervised child needs the group for
  tree-kill; taskkill/git/dotnet must not join it.
- **`windows_creation_flags()` is defined on all platforms** (pure arithmetic) so its value
  is unit-testable without a Windows host; only the usage sites are `#[cfg(windows)]`.
- **Attention flash overrides pill colour** (`.review-pill.attention` keyframes hard-set
  `background`). The terminal pill only applies its custom colour when `!attention`, so the
  flash still reads. This is intentional, not a bug.
- **Notes colour persists; terminal colour/title do not.** The Notes panel is long-lived
  (one global instance), so its pill colour is in localStorage. Terminals are ephemeral
  (the `terminals` array seeds to `[]` on restart), so title/colour live only on the
  in-memory `TerminalDescriptor`.
- **rustfmt reorders `pub use`** — the `#[cfg(windows)] pub use kill::no_window;` block gets
  sorted ahead of the pair re-export. Run `cargo fmt` and accept its ordering.

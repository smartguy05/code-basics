# Editor Context MCP

## What / why
Claude Code operates blind to what the user is looking at in code-basics. Expose the
editor's live context over MCP so an agent can anchor to the user's current focus
without pasting: active file, selection, cursor, open files, recently edited files,
visible viewport range.

## Tool surface (confirmed)
- `get_active_file` — active file path + cursor (line/char) + viewport range + dirty/pinned.
- `get_selection` — start/end (line/char) + selected text (length-capped).
- `get_open_files` — all open tabs: path, active, dirty, pinned.
- `get_recent_files` — recently *edited* files, edit-recency order (new frontend tracking).

## Decisions (confirmed with user 2026-09-14)
- **Scope: per-workspace (Roslyn twin).** Install bakes `--workspace <root>`; agent gets
  that repo's editor state regardless of which IDE tab is foreground.
- **Privacy: FeatureId `EditorContextMcp`, default ON.** Frontend only pushes context to
  the backend while the feature is enabled. Installing into an agent stays a separate,
  previewed consent step. Selection text is length-capped.
- Read-only. No writes ever.

## Position convention
1-based line, 0-based UTF-16 character — the app-wide convention (see api.ts:1286-1302).
</content>
</invoke>

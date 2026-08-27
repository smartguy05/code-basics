# Notes

## Secrets picker for multi-project folders (bug fix, same branch)
- Symptom: with several .NET projects in one folder, the "Secrets…" button only
  ever opened the **selected run config's** project — no way to reach the others.
- Root cause: `RunView` Secrets button was `onClick={openSecrets(selected.project)}`
  gated by `canEditSecrets(selected)` — bound to the run-config selection, not the
  project list.
- Fix: new pure `secretsProjects(projects)` in `editorSourceLogic.ts` (dotnet +
  readable, scan order; tested in `editorSourceLogic.test.ts`). RunView now renders
  a **project picker**: 0 projects → disabled button; 1 → direct button; >1 →
  `.dropdown` listing each project by name, each opening `openSecrets(p.manifestPath)`.
- `read/write_project_secrets` take the workspace-relative `.csproj` path
  (`Project.manifestPath` == `RunConfig.project` format), resolved via
  `secrets::resolve_project_path`, so `manifestPath` works for any project.
- Removed the now-unused `canEditSecrets` helper.

## Terminal Ctrl+V paste
- Copy/paste (added in 41d441c) wired paste only to Ctrl+Shift+V; user expected
  plain Ctrl+V. Widened `terminalKeyAction`: `e.ctrlKey && key === "v"` → paste
  (covers Ctrl+V and Ctrl+Shift+V). Ctrl+C stays interrupt; copy on
  Ctrl+Shift+C / Ctrl+Insert. TerminalView unchanged (already routes paste →
  clipboard.readText → onData). Test added in terminalLogic.test.ts.
- If Ctrl+Shift+V also failed, suspect WebView clipboard-read permission and
  switch to the Tauri clipboard plugin.

## Terminal numbering "bug" — not a bug
- Report: terminal numbers not unique per codebase (Terminal 1 then Terminal 2).
- Current source already numbers per `WorkspaceTab` (`terminalSeq = useRef(0)`,
  landed in `0ff822c`, in main). Pre-`0ff822c` terminals were app-level with one
  shared counter — which matches the symptom.
- User was running the **installed** build (`C:\Program Files\code-basics\cb-app.exe`),
  which predates the fix. Resolution: rebuild + reinstall; no code change.

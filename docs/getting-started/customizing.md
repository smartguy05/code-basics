# Customizing code-basics

Open **File → Settings** to change the app's appearance and keyboard shortcuts.
These preferences are global: the same choices apply to every workspace you
open, and they are restored when the app starts again.

## Appearance

The Appearance tab starts with two built-in themes, **Dark** and **Light**.
Built-in themes are read-only. Duplicate one when you want to use it as the
starting point for your own theme.

A custom theme can set:

- the UI and code font families;
- UI and code font sizes independently;
- application surfaces, borders, controls, tabs, and status colours;
- editor syntax colours; and
- terminal and diff colours.

Changes preview immediately while Settings is open. **Apply** saves them;
**Cancel** restores the appearance that was active before the dialog opened.
Deleting the active custom theme selects a built-in theme instead.

### Moving a theme between computers

Use **Export** to download the selected custom theme as JSON. **Import** accepts
a theme JSON file and validates it before adding it to the theme list. Imported
themes become separate custom themes, so importing does not overwrite either
built-in theme.

## Keyboard shortcuts

The Keyboard tab is the complete list of app-owned commands. Search by command,
category, or current shortcut. To change a command, select its shortcut control
and press one key chord. A shortcut can contain Ctrl, Shift, Alt, or Cmd/Meta,
but not a multi-step sequence.

The app refuses duplicate assignments. Clear the conflicting command first, or
choose a different chord. **Clear** leaves a command unbound; **Reset** restores
that command's default; **Reset all shortcuts** restores every default.

Important defaults include:

| Shortcut | Action |
|---|---|
| `Ctrl+N` | Search everything |
| `Ctrl+Shift+N` | Search files |
| `Ctrl+Shift+A` | Search actions |
| `Ctrl+/` | Ask the codebase |
| `Ctrl+F` | Find in the visible console or editor |
| `F7` / `Shift+F7` | Next / previous change |
| `Ctrl+=` / `Ctrl+-` / `Ctrl+0` | Increase / decrease / reset code size |

Search Symbols has no default shortcut. Assign one in Settings if you use it
frequently.

Some keyboard behavior belongs to CodeMirror or the interactive terminal rather
than to code-basics. The **Native keys** tab lists those bindings for reference;
they are not editable in this settings dialog. Destructive commands still show
their normal confirmation after they are invoked from a custom shortcut.

## Project attention signals

Work continues when you switch to another project. A background project tab
shows the most important event waiting there:

- **red** means a build, test run, terminal, or launched app failed and remains
  until you visit that project;
- **amber** means a hidden terminal rang its attention bell and remains until
  you view or restore it; and
- **green** means background work completed successfully and fades after a
  short acknowledgement window.

Stopping work yourself is quiet. Signals are raised only for work you are not
already viewing, and a failure is never replaced by a later success.

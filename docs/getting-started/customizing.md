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

### Window opacity

Beneath the theme colours is a **Window** section with an opacity slider. It
applies **only while no file or diff is open** — the moment you open one the
window is fully opaque again, so nothing you are reading is ever shown through.
Floating panels are unaffected: a terminal, the Notes panel, the SQL console and
the agent panel all stay solid over a translucent window, which is the point.

The default is 100%, which is exactly the window as it was before the setting
existed. The slider stops at 30% so the file tree and console labels stay
readable whatever is behind them.

Transparency needs support from the window manager, so the slider is available
on Windows and is **disabled with the reason stated** elsewhere — on macOS it
would require a private API this build does not enable, and on Linux it needs a
compositing window manager that cannot be detected reliably. Where it is
disabled the window is simply opaque; nothing else changes.

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

## Terminal

The Terminal tab chooses the shell new terminals run. It lists what was actually
found on this machine, each with the resolved path it would launch, plus a
**System default** entry — which names the platform default when it can, and
stays unlabelled rather than guessing when it cannot.

Three states are kept distinct, because they mean different things: while the
list is still being read the tab says so; a machine where no shell could be
found says *that*, rather than showing an empty picker that would claim none
exist; and a choice whose shell has since vanished is reported without being
erased. A shell can be missing because `PATH` is temporarily broken or a tool is
mid-upgrade, and silently dropping your choice over a transient absence is not
recoverable — so the preference is kept, new terminals fall back to the platform
default, and this tab tells you which shell it could not find.

Unlike every other control in this dialog, this one has no live preview. There
is nothing to preview: a terminal's shell is fixed when the terminal opens, and a
running session cannot have one swapped underneath it. The setting applies to
terminals you open next. To open a single terminal in a different shell without
changing the default, use **New terminal in** in the terminal button's caret
menu.

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

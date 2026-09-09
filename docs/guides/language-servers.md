# Language servers

"Find usages" and "go to definition" are answered by a real language server for the language of the file you are looking at — the same programs VS Code and Neovim drive. The counts are therefore semantic: `Order.Total` and `Invoice.Total` are different symbols, and a rename or a reflection-free call site is found or missed exactly as the compiler would.

**Nothing is bundled.** The four servers below are hundreds of megabytes between them and their licences differ per option, so code-basics launches the copy already on your machine and redistributes none of them. The cost of that choice is that a server can simply be absent — which is reported as "No language server" with the command to install it, never worked around and never rendered as a count of zero.

## Which server per language

| Language | Files | Program looked for, in order | Install |
|---|---|---|---|
| C# | `.cs` | `Microsoft.CodeAnalysis.LanguageServer(.exe)` inside the VS Code **C# extension** | Install `ms-dotnettools.csharp` in VS Code (or Cursor / Windsurf / VS Code Insiders / a `.vscode-server`) |
| TypeScript / JavaScript | `.ts .tsx .js .jsx .mjs .cjs .mts .cts` | `typescript-language-server` on `PATH` | `npm i -g typescript-language-server typescript@5` |
| Rust | `.rs` | `rust-analyzer` on `PATH` | `rustup component add rust-analyzer` |
| Python | `.py .pyi` | `basedpyright-langserver`, then `pyright-langserver` — both on `PATH` | `pip install basedpyright`, or `npm i -g pyright` |

Any other extension gets no server and no row — not a failure, just a language this feature does not cover.

**`typescript@5`, not `typescript`.** The `latest` tag now resolves to TypeScript 7, whose package ships `tsc` and no tsserver at all. `typescript-language-server` starts anyway, falls back to a stub that reports itself as version 1.0.0, and answers every query with an empty list — so following the unpinned command gives you a server that confidently reports zero usages for everything.

Four deliberate absences, all for one reason: each would connect successfully and then give a count that is wrong, which is the only outcome this feature treats as worse than having no server at all. All four were run against a two-file fixture with exactly one call site (`crates/core/tests/lsp_oracle.rs`).

- **`ruff` is never used**, even though it speaks LSP. It is a linter with no `references` and no `definition`, so it would connect happily and then answer nothing — which on screen reads as "this method is unused". A server that cannot answer must be recognised as such, not started.
- **`pylsp` is never used.** It starts, initialises and reports itself ready, then answers `references` with **zero** for a symbol that has one usage.
- **`jedi-language-server` is never used.** It ignores `includeDeclaration: false` and returns the declaration alongside the call site, so every count it gives is one too high — "1 usage" would appear above a method nothing calls. It also publishes no diagnostics, so there is no signal that it has finished starting.
- **`ms-dotnettools.csdevkit` is never touched.** It is a different, proprietary extension. Only the MIT-licensed `ms-dotnettools.csharp` is searched, and the search prefix ends with `csharp-` so it cannot match the other one.

### How C# is found

There is no `PATH` entry for the Roslyn server, so it is searched for in this order:

1. **`CB_ROSLYN_PATH`**, if set and non-empty — a directory containing the executable, or the executable itself under whatever name it has. A stale value does *not* disable C#: the search falls through to the extensions, and the variable still appears in "Looked in" so the fall-through is visible.
2. The C# extension's bundled copy under your home directory, per editor, in this order: `.vscode`, `.vscode-insiders`, `.vscode-server`, `.cursor`, `.windsurf`. Directory order decides overall and the extension version decides within a directory — the same rule `PATH` already follows. Picking the highest version across all editors would launch a Windsurf-installed server for somebody working in VS Code, which is unpredictable in a way you cannot see.

The server is started with `--stdio --logLevel Warning --autoLoadProjects`, which is enough to load a solution without the non-standard `solution/open` handshake the VS Code extension performs. A large solution takes tens of seconds to load; the inline row says *"Language server loading…"* for that whole time and never a number.

## Pointing the app at a specific server

Everything below lives in the `lsp` block of `.code-basics/config.json`, documented field by field in the [configuration reference](../reference/configuration.md#lsp). The block is optional and absent by default; saving a run configuration never introduces it.

```json
{
  "lsp": {
    "servers": {
      "csharp": {
        "program": "C:/tools/roslyn/Microsoft.CodeAnalysis.LanguageServer.exe"
      },
      "typescript": {
        "program": "typescript-language-server",
        "args": ["--stdio", "--log-level", "2"]
      },
      "python": { "enabled": false }
    }
  }
}
```

Server ids are `csharp`, `typescript`, `rust`, `python`. Every field is optional and every absence means "use the built-in default".

- **`program`** is an absolute path or a bare name resolved on `PATH` (through `PATHEXT` on Windows, so npm's `typescript-language-server.cmd` shim works by name). **If it does not resolve, that server fails** with an error naming this file — it never falls back to discovery. You asked for one specific build, usually to match a project's SDK, and quietly starting a different one would attribute its counts and jumps to a server that never ran.
- **`args` replaces** the built-in list rather than appending to it, so an unwanted default can be removed. Omitting the key keeps the defaults; `"args": []` launches with none — and the Roslyn server without `--stdio` never says a word, which looks like a hang rather than a mistake.
- **`enabled: false`** switches one language off. Only an explicit `false` counts.
- **`env`** layers over the inherited environment for that server's process only.
- **`uriStyle`** (`"encoded"` / `"plain"`) is an escape hatch for a server that rejects one spelling of the drive colon in `file:` URIs. File identity is decided on paths, not on URI strings, so it rarely matters.

The block is read **once per session**, and a session survives a rescan — only changing the workspace *root* restarts one. So editing `lsp` takes effect the next time the workspace is opened, not on save. That is deliberate: a rescan runs on every configuration save, and restarting Roslyn each time would cost tens of seconds of "still loading" over a solution that did not change.

## When something is wrong

The titlebar shows an indicator whenever a server has something to say — and nothing at all when every server is ready, since a green light is noise. Click it for one row per server: what it is doing, the detail (a version line, an exit code, an error), the hint, and **every path that was searched** when nothing was found. The list of searched paths is not trimmed to the first entry, because "not found" is only actionable if you can see what was tried.

| What you see | What it means | What to do |
|---|---|---|
| *No language server* | Nothing installed or configured for this language | Install it from the table above, or set `program` |
| *Language server starting…* | The process is up and handshaking | Wait — seconds |
| *…loading projects…* | It is reading the solution or workspace | Wait — tens of seconds for a large C# solution |
| *Language server failed* | The process died; the row carries the exit code or error | Check `.code-basics/lsp-logs/`, then the `program`/`args` you set |
| *This server cannot answer* | This server does not offer that capability at all | Use a different server for that language; retrying will not help |
| *Usages gave up waiting* | The server was still loading after two minutes | Close and reopen the file tab to start asking again |
| *Usages paused* | The file could not be sent to the server, so nothing is being counted | The next edit that gets through resumes it; the row carries the reason |

Servers write their own logs into `.code-basics/lsp-logs/`, which is gitignored — a verbose trace of one editing session, rewritten on every launch and safe to delete at any time.

## Renaming a symbol (F2)

Put the caret on a class, method, property or variable and press **F2**. A field
opens over the symbol, prefilled with its current name and selected; Enter
applies the rename everywhere the server says it occurs, Escape or clicking away
cancels.

It is answered by the same server as everything else on this page, and that has
a consequence worth stating plainly: **there is no text-based fallback.** A
server that does not advertise `renameProvider`, or is not installed, refuses and
says which server it looked for. The symbol index behind the search palette is
*not* used — a near miss there costs a keystroke, whereas a near miss in a rename
writes the wrong text into files you may not have open.

Two files change in two different ways, because they have two different truths:

- **Files you have open** are edited in the editor, as one undo step each. Their
  buffers go dirty and you save them as usual. The backend never writes to disk
  behind an open tab — there is no file watcher, so the next Ctrl+S would
  clobber it.
- **Files you do not have open** are written to disk by the backend and are
  re-indexed for the search palette immediately. They are **not** undoable in
  the editor, so the summary notification says so and points at the Changes tab,
  where line-level revert is what this app is actually good at.

### What it refuses, and why

A rename is refused **whole**, having written nothing, if any of these hold. A
partial rename does not compile, so there is no useful half-measure:

- Two edits overlap, or two insertions land at one position with no defined order.
- An edit names a line the file does not have — the server is describing a
  different version of it, and applying that edit somewhere plausible is the
  worst available outcome.
- An edit falls outside the workspace, or on a non-`file:` URI (Roslyn really
  does emit `source-generated:` ones).
- The server asks to create, rename or delete a **file**. Renaming a type does
  not rename its file here; that is a much larger blast radius and is declined.
- None of a file's edits land on a token matching the old name, which means the
  server computed them from text this app does not have.

If a `didChange` is still owed when you press F2 — you typed within the last
quarter second — the app flushes it and waits before opening the field, and
re-checks the document version again at Enter. That ordering is the whole
defence: ranges computed against a stale mirror are *plausible and wrong*.

One qualification is surfaced rather than hidden: if the server was promoted at
its readiness ceiling (90 seconds, for a project that never finished loading),
the rename may have **missed call sites**, and you are told before it is applied.

### What has not been verified

The rename path is covered by unit and integration tests plus a scripted server,
and was measured against real Roslyn 2.140.9 — which, usefully, turned out to
answer with zero-width **insertions** carrying a minimal diff (renaming `Walker`
to `HeapWalker` inserts `"Heap"`) rather than replacing whole identifiers. Any
code assuming a replacement is a live bug for C#, not a theoretical one.

`typescript-language-server` and `rust-analyzer` have **not** answered this code
yet. Their edit style is reasoned about rather than observed, and the
enclosing-token check is designed to cover both — but that is an argument, not
evidence.

## Verifying against a real server

The whole subsystem is otherwise tested against a scripted stand-in, which cannot catch a protocol regression against the real thing. `crates/core/tests/lsp_oracle.rs` closes that: it writes a small project per language, starts a session through the same discovery the app uses, and asserts the one usage it knows is there.

```sh
cargo test -p cb-core --test lsp_oracle -- --ignored --nocapture --test-threads=1
```

`#[ignore]`d because the result depends on which servers the machine has; a language with no server installed prints a line and is skipped, and nothing else is. `--test-threads=1` is required — four language servers indexing at once starve Roslyn's project load past its readiness ceiling.

## Related

- [Using the app](../getting-started/using-the-app.md#finding-where-a-method-is-used) — what the inline row and middle-click do
- [Configuration reference](../reference/configuration.md#lsp) — the `lsp` block, field by field
- [The core crate](../architecture/core-crate.md) — how the client, the capability gates and the result mapping work

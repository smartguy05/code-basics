import {
  useEffect,
  useImperativeHandle,
  useRef,
  useState,
  forwardRef,
} from "react";
import { FitAddon } from "@xterm/addon-fit";
import { SearchAddon } from "@xterm/addon-search";
import { WebLinksAddon } from "@xterm/addon-web-links";
import { Terminal } from "@xterm/xterm";
import { openUrl } from "@tauri-apps/plugin-opener";
import type { ProcessEvent } from "../ipc/types";
import { decorate, filterLines, stripAnsi, type Severity } from "./consoleLogic";

/** How much raw output to keep for Copy All / diagnostics. */
const RAW_CAP = 1_000_000;

const SEARCH_DECORATIONS = {
  matchBackground: "#3d55a8",
  matchBorder: "#3d55a8",
  matchOverviewRuler: "#5a78dc",
  activeMatchBackground: "#5a78dc",
  activeMatchBorder: "#5a78dc",
  activeMatchColorOverviewRuler: "#d6dae2",
};

export interface ConsoleHandle {
  write(text: string): void;
  clear(): void;
  /** Render a process event, including the exit line. */
  handle(event: ProcessEvent): void;
}

/**
 * A terminal view of process output.
 *
 * xterm rather than a `<pre>`: `dotnet` and `vitest` both emit ANSI colour and
 * redraw progress with bare carriage returns, which a plain text node renders
 * as unreadable noise.
 *
 * On top of the raw stream it carries the troubleshooting affordances: Ctrl+F
 * search, copy-on-select, and a right-click menu with Copy All and Copy
 * diagnostics (command line + exit + the last output lines, paste-ready).
 */
export const OutputConsole = forwardRef<ConsoleHandle, { className?: string }>(
  function OutputConsole({ className }, ref) {
    const hostRef = useRef<HTMLDivElement>(null);
    const termRef = useRef<Terminal | null>(null);
    const searchRef = useRef<SearchAddon | null>(null);

    /** Plain-text tail of everything written, for clipboard features. */
    const rawRef = useRef("");
    /** The last `started` / `exited` events, for Copy diagnostics. */
    const startedRef = useRef<Extract<ProcessEvent, { type: "started" }> | null>(null);
    const exitedRef = useRef<Extract<ProcessEvent, { type: "exited" }> | null>(null);

    const [searchOpen, setSearchOpen] = useState(false);
    const [query, setQuery] = useState("");
    const [filterOn, setFilterOn] = useState(false);
    const [severity, setSeverity] = useState<Severity>("all");
    const [menu, setMenu] = useState<{ x: number; y: number } | null>(null);
    const searchInputRef = useRef<HTMLInputElement>(null);

    /** Mirror of the filter state, readable from the imperative handle. */
    const filterRef = useRef<{ active: boolean; severity: Severity; text: string }>({
      active: false,
      severity: "all",
      text: "",
    });
    const rebuildTimer = useRef<number | null>(null);

    function appendRaw(text: string) {
      const next = rawRef.current + text;
      rawRef.current = next.length > RAW_CAP ? next.slice(-RAW_CAP) : next;
    }

    /** Re-render the terminal from the raw buffer, filtered or not. */
    function rebuildView() {
      const term = termRef.current;
      if (!term) return;
      const filter = filterRef.current;
      const content = filter.active
        ? filterLines(rawRef.current, filter.severity, filter.text)
        : rawRef.current;

      term.reset();
      if (content) term.write(decorate(content.endsWith("\n") ? content : `${content}\r\n`));
      term.scrollToBottom();
    }

    /** Rebuilds are debounced: chatty processes stream many chunks a second. */
    function scheduleRebuild() {
      if (rebuildTimer.current !== null) return;
      rebuildTimer.current = window.setTimeout(() => {
        rebuildTimer.current = null;
        rebuildView();
      }, 250);
    }

    function updateFilter(on: boolean, level: Severity, text: string) {
      setFilterOn(on);
      setSeverity(level);
      filterRef.current = {
        active: on || level !== "all",
        severity: level,
        text: on ? text : "",
      };
      rebuildView();
    }

    /** Route text to the terminal, honouring an active filter. */
    function emit(term: Terminal, decorated: string) {
      if (filterRef.current.active) {
        scheduleRebuild();
      } else {
        term.write(decorated);
      }
    }

    function copyText(text: string) {
      void navigator.clipboard.writeText(text);
    }

    function copyAll() {
      copyText(stripAnsi(rawRef.current));
    }

    /** A paste-ready troubleshooting block. */
    function copyDiagnostics() {
      const started = startedRef.current;
      const exited = exitedRef.current;
      const lines = stripAnsi(rawRef.current).split(/\r?\n/);
      const tail = lines.slice(-100).join("\n");

      const block = [
        started && `$ ${started.program} ${started.args.join(" ")}`,
        started && `cwd: ${started.cwd}`,
        exited &&
          `exit: ${exited.code ?? "killed"} (${exited.success ? "success" : "failure"}${
            exited.cancelled ? ", cancelled" : ""
          }) after ${(exited.durationMs / 1000).toFixed(2)}s`,
        exited === null && started !== null && "exit: still running",
        "",
        "--- last output ---",
        tail,
      ]
        .filter((part): part is string => typeof part === "string")
        .join("\n");

      copyText(block);
    }

    useEffect(() => {
      if (!hostRef.current) return;

      const term = new Terminal({
        fontFamily:
          '"JetBrains Mono", "SF Mono", Menlo, Consolas, "Liberation Mono", monospace',
        fontSize: 12,
        convertEol: true,
        scrollback: 20000,
        allowProposedApi: true,
        theme: {
          background: "#12141a",
          foreground: "#d6dae2",
          cursor: "#12141a",
        },
      });

      const fit = new FitAddon();
      term.loadAddon(fit);
      // URLs in the output ("Now listening on: https://...") open in the
      // system browser — inside the webview they would replace the app.
      term.loadAddon(
        new WebLinksAddon((_event, uri) => {
          void openUrl(uri);
        }),
      );
      const search = new SearchAddon();
      term.loadAddon(search);
      searchRef.current = search;

      term.open(hostRef.current);
      fit.fit();

      // Terminal muscle memory: select-to-copy, and Ctrl+C copies a selection
      // (there is no shell on the other side to interrupt).
      term.onSelectionChange(() => {
        const selection = term.getSelection();
        if (selection) void navigator.clipboard.writeText(selection);
      });
      term.attachCustomKeyEventHandler((event) => {
        if (event.type !== "keydown") return true;
        if (event.ctrlKey && event.key.toLowerCase() === "c" && term.hasSelection()) {
          return false; // selection already copied by onSelectionChange
        }
        return true;
      });

      // Ctrl+F is intercepted at the window level, capture phase, so the
      // webview's own find bar never sees it. Only the visible console reacts
      // (hidden tabs have no offsetParent).
      const onKeyDown = (event: KeyboardEvent) => {
        if (!event.ctrlKey || event.key.toLowerCase() !== "f") return;
        if (!hostRef.current || hostRef.current.offsetParent === null) return;
        event.preventDefault();
        event.stopPropagation();
        setSearchOpen(true);
        setTimeout(() => searchInputRef.current?.focus(), 0);
      };
      window.addEventListener("keydown", onKeyDown, true);

      termRef.current = term;

      const observer = new ResizeObserver(() => {
        // Fitting a detached or zero-sized terminal throws.
        try {
          fit.fit();
        } catch {
          /* the pane is hidden; the next resize will fit it */
        }
      });
      observer.observe(hostRef.current);

      return () => {
        observer.disconnect();
        window.removeEventListener("keydown", onKeyDown, true);
        term.dispose();
        termRef.current = null;
        searchRef.current = null;
      };
    }, []);

    function findNext(text: string, backwards = false) {
      const search = searchRef.current;
      if (!search || !text) return;
      if (backwards) {
        search.findPrevious(text, { decorations: SEARCH_DECORATIONS });
      } else {
        search.findNext(text, { decorations: SEARCH_DECORATIONS });
      }
    }

    function closeSearch() {
      setSearchOpen(false);
      searchRef.current?.clearDecorations();
      updateFilter(false, "all", "");
      termRef.current?.focus();
    }

    useImperativeHandle(ref, () => ({
      write(text: string) {
        appendRaw(text);
        termRef.current?.write(text);
      },
      clear() {
        rawRef.current = "";
        startedRef.current = null;
        exitedRef.current = null;
        // reset() rather than clear(): clear() keeps the cursor line, which
        // leaves a stray fragment of output behind.
        termRef.current?.reset();
      },
      handle(event: ProcessEvent) {
        const term = termRef.current;
        if (!term) return;

        switch (event.type) {
          case "started":
            startedRef.current = event;
            exitedRef.current = null;
            appendRaw(`$ ${event.program} ${event.args.join(" ")}\n  in ${event.cwd}\n`);
            emit(
              term,
              `\x1b[38;5;245m$ ${event.program} ${event.args.join(" ")}\r\n` +
                `  in ${event.cwd}\x1b[0m\r\n`,
            );
            break;
          case "output":
            appendRaw(event.text);
            emit(term, decorate(event.text));
            break;
          case "exited": {
            exitedRef.current = event;
            const seconds = (event.durationMs / 1000).toFixed(2);
            if (event.cancelled) {
              appendRaw(`\ncancelled after ${seconds}s\n`);
              emit(term, `\r\n\x1b[33mcancelled after ${seconds}s\x1b[0m\r\n`);
            } else if (event.success) {
              appendRaw(`\nfinished in ${seconds}s\n`);
              emit(term, `\r\n\x1b[32mfinished in ${seconds}s\x1b[0m\r\n`);
            } else {
              appendRaw(`\nexited with code ${event.code ?? "unknown"} after ${seconds}s\n`);
              emit(
                term,
                `\r\n\x1b[31mexited with code ${event.code ?? "unknown"} after ${seconds}s\x1b[0m\r\n`,
              );
            }
            break;
          }
          case "failed":
            appendRaw(`\n${event.message}\n`);
            emit(term, `\r\n\x1b[31m${event.message}\x1b[0m\r\n`);
            break;
        }
      },
    }));

    return (
      <div
        className="console-host"
        onContextMenu={(e) => {
          e.preventDefault();
          const bounds = e.currentTarget.getBoundingClientRect();
          setMenu({ x: e.clientX - bounds.left, y: e.clientY - bounds.top });
        }}
      >
        {searchOpen && (
          <div className="console-search">
            <input
              ref={searchInputRef}
              placeholder={filterOn ? "Filter lines" : "Find in output"}
              value={query}
              onChange={(e) => {
                setQuery(e.target.value);
                if (filterOn) {
                  updateFilter(true, severity, e.target.value);
                } else {
                  findNext(e.target.value);
                }
              }}
              onKeyDown={(e) => {
                if (e.key === "Enter" && !filterOn) findNext(query, e.shiftKey);
                if (e.key === "Escape") closeSearch();
              }}
            />
            <button
              onClick={() => findNext(query, true)}
              disabled={filterOn}
              title="Previous match (Shift+Enter)"
            >
              ↑
            </button>
            <button
              onClick={() => findNext(query)}
              disabled={filterOn}
              title="Next match (Enter)"
            >
              ↓
            </button>
            <select
              value={severity}
              onChange={(e) => updateFilter(filterOn, e.target.value as Severity, query)}
              title="Hide lines below this severity"
            >
              <option value="all">All levels</option>
              <option value="info">Info+</option>
              <option value="warn">Warn+</option>
              <option value="error">Errors</option>
            </select>
            <button
              className={filterOn ? "primary" : ""}
              onClick={() => updateFilter(!filterOn, severity, query)}
              title="Hide lines that do not contain the text (instead of jumping between matches)"
            >
              Filter
            </button>
            <button onClick={closeSearch} title="Close (Esc)">
              ×
            </button>
          </div>
        )}

        {menu && (
          <>
            <div className="dropdown-backdrop" onClick={() => setMenu(null)} />
            <div className="dropdown-menu" style={{ left: menu.x, top: menu.y }}>
              <div
                className="dropdown-item"
                onClick={() => {
                  const selection = termRef.current?.getSelection();
                  if (selection) copyText(selection);
                  setMenu(null);
                }}
              >
                Copy selection
              </div>
              <div
                className="dropdown-item"
                onClick={() => {
                  copyAll();
                  setMenu(null);
                }}
              >
                Copy all output
              </div>
              <div
                className="dropdown-item"
                onClick={() => {
                  copyDiagnostics();
                  setMenu(null);
                }}
                title="Command line, exit code, and the last 100 lines — paste-ready"
              >
                Copy diagnostics
              </div>
              <div
                className="dropdown-item"
                onClick={() => {
                  setSearchOpen(true);
                  setMenu(null);
                  setTimeout(() => searchInputRef.current?.focus(), 0);
                }}
              >
                Find… (Ctrl+F)
              </div>
            </div>
          </>
        )}

        <div className={`console ${className ?? ""}`} ref={hostRef} />
      </div>
    );
  },
);

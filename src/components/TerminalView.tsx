import { forwardRef, useEffect, useImperativeHandle, useRef } from "react";
import { FitAddon } from "@xterm/addon-fit";
import { WebLinksAddon } from "@xterm/addon-web-links";
import { Terminal } from "@xterm/xterm";
import { openUrl } from "@tauri-apps/plugin-opener";
import { terminalKeyAction, terminalKeyEffect } from "./terminalLogic";
import { onAppearanceChange, terminalAppearance } from "../appearance";

/** Imperative surface the hosting panel drives. */
export interface TerminalViewHandle {
  /** Write raw bytes straight to the terminal — no post-processing. */
  write(bytes: string): void;
  /** Focus the terminal so keystrokes land in it. */
  focus(): void;
  /** Re-fit to the host size and report the new dimensions. */
  fit(): void;
}

/**
 * A raw interactive terminal.
 *
 * Deliberately **not** `OutputConsole`: that view re-colours, filters and
 * rebuilds its buffer, which is right for watching a batch process and fatal
 * for an interactive program that redraws its own screen with cursor moves and
 * clears (Claude Code's TUI, a shell's line editor). Here bytes go straight in
 * via {@link TerminalViewHandle.write}, and keystrokes come straight out via
 * `onData` — the terminal emulator is xterm, and the process on the other end
 * owns what is drawn.
 *
 * xterm answers Device Status Report queries (`ESC [ 6 n`) itself, which is why
 * an interactive shell started in a PTY does not hang here the way it does with
 * no emulator attached.
 */
export const TerminalView = forwardRef<
  TerminalViewHandle,
  {
    /** Keystrokes (and pasted text) the user produced. */
    onData: (data: string) => void;
    /** The terminal was resized; report the new column/row count. */
    onResize: (cols: number, rows: number) => void;
  }
>(function TerminalView({ onData, onResize }, ref) {
  const hostRef = useRef<HTMLDivElement>(null);
  const termRef = useRef<Terminal | null>(null);
  const fitRef = useRef<FitAddon | null>(null);
  // Keep the latest callbacks reachable from the mount-once effect without
  // re-running it (which would tear down and rebuild the terminal).
  const onDataRef = useRef(onData);
  const onResizeRef = useRef(onResize);
  onDataRef.current = onData;
  onResizeRef.current = onResize;

  useEffect(() => {
    if (!hostRef.current) return;

    const term = new Terminal({
      ...terminalAppearance(),
      scrollback: 20000,
      allowProposedApi: true,
      // A block cursor that blinks, as an interactive shell expects.
      cursorBlink: true,
    });

    const fit = new FitAddon();
    term.loadAddon(fit);
    // URLs printed by a program open in the system browser, not the webview.
    term.loadAddon(
      new WebLinksAddon((_event, uri) => {
        void openUrl(uri);
      }),
    );

    term.open(hostRef.current);
    try {
      fit.fit();
    } catch {
      /* the pane may be momentarily zero-sized; a later resize fits it */
    }

    // Keystrokes and pasted text flow straight to the PTY. Ctrl+C is left
    // alone on purpose — in an interactive shell it is an interrupt (`\x03`),
    // not a copy, and xterm forwards it here as such.
    const dataSub = term.onData((data) => onDataRef.current(data));

    // Copy/paste. A raw PTY terminal has none of its own, and Ctrl+C must stay
    // the interrupt, so the usual terminal chords do it instead. Both halves of
    // the decision are `terminalLogic`'s: which chord this is
    // (`terminalKeyAction`) and what to do about it (`terminalKeyEffect`) — this
    // handler only carries the effect out, because a `.tsx` is not loadable by
    // the test suite and the `preventDefault` below is the part that matters.
    term.attachCustomKeyEventHandler((event) => {
      const action = terminalKeyAction(event, term.hasSelection());
      const effect = terminalKeyEffect(action);
      // Returning false stops only xterm's own key *translation* — it does not
      // stop the webview running its native editing command for the chord. For
      // a paste that meant a second, unbracketed and unnormalized write to the
      // PTY (see `terminalKeyEffect`); for a copy it races our `writeText`
      // against a native copy of the hidden textarea's selection. So the
      // handled chords are prevented outright, and a passthrough never is:
      // Ctrl+C is the shell interrupt and F5 is an app shortcut.
      if (effect.preventDefault) event.preventDefault();
      if (effect.copy) {
        const selection = term.getSelection();
        if (selection) void navigator.clipboard?.writeText(selection).catch(() => {});
        return false;
      }
      if (effect.paste) {
        // `term.paste` is the same helper xterm's own paste path uses, so the
        // text is CRLF-normalized and bracketed exactly once and the app has
        // one paste behaviour rather than two. The read is async and
        // permission-gated, and now that the native path is prevented a
        // rejection means nothing pastes at all — so it is logged rather than
        // swallowed, since a silently dead chord is the harder bug to find.
        void navigator.clipboard
          ?.readText()
          .then((text) => {
            if (text) term.paste(text);
          })
          .catch((e) =>
            console.error("code-basics: clipboard read failed; nothing was pasted", e),
          );
        return false;
      }
      return true;
    });

    termRef.current = term;
    fitRef.current = fit;
    // Report the initial size so the PTY starts matched to the view.
    onResizeRef.current(term.cols, term.rows);

    const observer = new ResizeObserver(() => {
      try {
        fit.fit();
        onResizeRef.current(term.cols, term.rows);
      } catch {
        /* hidden (minimized) panes measure zero; the next resize fits them */
      }
    });
    observer.observe(hostRef.current);
    const stopAppearance = onAppearanceChange(() => {
      const appearance = terminalAppearance();
      term.options.fontFamily = appearance.fontFamily;
      term.options.fontSize = appearance.fontSize;
      term.options.theme = appearance.theme;
      try { fit.fit(); } catch { /* hidden */ }
    });

    return () => {
      observer.disconnect();
      stopAppearance();
      dataSub.dispose();
      term.dispose();
      termRef.current = null;
      fitRef.current = null;
    };
  }, []);

  useImperativeHandle(ref, () => ({
    write(bytes: string) {
      termRef.current?.write(bytes);
    },
    focus() {
      termRef.current?.focus();
    },
    fit() {
      try {
        fitRef.current?.fit();
        const term = termRef.current;
        if (term) onResizeRef.current(term.cols, term.rows);
      } catch {
        /* not measurable right now */
      }
    },
  }));

  return <div className="terminal-host" ref={hostRef} />;
});

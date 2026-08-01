import { useEffect, useImperativeHandle, useRef, forwardRef } from "react";
import { FitAddon } from "@xterm/addon-fit";
import { Terminal } from "@xterm/xterm";
import type { ProcessEvent } from "../ipc/types";

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
 */
export const OutputConsole = forwardRef<ConsoleHandle, { className?: string }>(
  function OutputConsole({ className }, ref) {
    const hostRef = useRef<HTMLDivElement>(null);
    const termRef = useRef<Terminal | null>(null);
    const fitRef = useRef<FitAddon | null>(null);

    useEffect(() => {
      if (!hostRef.current) return;

      const term = new Terminal({
        fontFamily:
          '"JetBrains Mono", "SF Mono", Menlo, Consolas, "Liberation Mono", monospace',
        fontSize: 12,
        convertEol: true,
        scrollback: 20000,
        theme: {
          background: "#12141a",
          foreground: "#d6dae2",
          cursor: "#12141a",
        },
      });

      const fit = new FitAddon();
      term.loadAddon(fit);
      term.open(hostRef.current);
      fit.fit();

      termRef.current = term;
      fitRef.current = fit;

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
        term.dispose();
        termRef.current = null;
      };
    }, []);

    useImperativeHandle(ref, () => ({
      write(text: string) {
        termRef.current?.write(text);
      },
      clear() {
        termRef.current?.clear();
      },
      handle(event: ProcessEvent) {
        const term = termRef.current;
        if (!term) return;

        switch (event.type) {
          case "started":
            term.write(
              `\x1b[38;5;245m$ ${event.program} ${event.args.join(" ")}\r\n` +
                `  in ${event.cwd}\x1b[0m\r\n`,
            );
            break;
          case "output":
            term.write(event.text);
            break;
          case "exited": {
            const seconds = (event.durationMs / 1000).toFixed(2);
            if (event.cancelled) {
              term.write(`\r\n\x1b[33mcancelled after ${seconds}s\x1b[0m\r\n`);
            } else if (event.success) {
              term.write(`\r\n\x1b[32mfinished in ${seconds}s\x1b[0m\r\n`);
            } else {
              term.write(
                `\r\n\x1b[31mexited with code ${event.code ?? "unknown"} after ${seconds}s\x1b[0m\r\n`,
              );
            }
            break;
          }
          case "failed":
            term.write(`\r\n\x1b[31m${event.message}\x1b[0m\r\n`);
            break;
        }
      },
    }));

    return <div className={`console ${className ?? ""}`} ref={hostRef} />;
  },
);

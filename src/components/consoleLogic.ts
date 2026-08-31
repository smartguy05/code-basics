//! Pure decisions for the output console: colouring a chunk of process output,
//! stripping ANSI for the clipboard, and ranking lines by severity so the filter
//! can hide what is quieter than a chosen threshold.
//!
//! Output is stored as lines rather than one string because ranking an unmarked
//! line needs to know which *stream* it came from, and a flat buffer cannot say.
//! Tested headlessly (vitest, node environment); `OutputConsole.tsx` only renders.

import type { Stream } from "../ipc/types";

const URL_RE = /https?:\/\/[^\s"'<>)\]]+/g;

/**
 * Severity tokens worth colouring when the tool did not colour them itself:
 * the .NET console logger's level prefixes (`info:`, `warn:`, `fail:`, ...)
 * and MSBuild's `warning CS1234:` / `error CS1234:` diagnostics.
 */
export const SEVERITIES: [RegExp, string][] = [
  [/^(\s*)(trce|dbug)(?=:)/, "$1\x1b[90m$2\x1b[39m"], // grey
  [/^(\s*)(info)(?=:)/, "$1\x1b[32m$2\x1b[39m"], // green
  [/^(\s*)(warn)(?=:)/, "$1\x1b[33m$2\x1b[39m"], // yellow
  [/^(\s*)(fail|crit)(?=:)/, "$1\x1b[1;31m$2\x1b[22;39m"], // bold red
  [/\b(warning( [A-Z]+\d+)?)(?=:)/, "\x1b[33m$1\x1b[39m"], // yellow
  [/\b(error( [A-Z]+\d+)?)(?=:)/, "\x1b[1;31m$1\x1b[22;39m"], // bold red
];

/**
 * Colour URLs and severity markers in a chunk of output.
 *
 * Lines that already contain ANSI styling are left alone — the tool knows
 * better than a heuristic. URLs go bright blue + underlined so they read as
 * the clickable links they are.
 */
export function decorate(text: string): string {
  return text
    .split(/(\r?\n|\r)/)
    .map((part) => {
      if (/\r|\n/.test(part) || part.includes("\x1b[")) return part;

      let out = part;
      for (const [pattern, replacement] of SEVERITIES) {
        const styled = out.replace(pattern, replacement);
        if (styled !== out) {
          out = styled;
          break;
        }
      }
      return out.replace(URL_RE, "\x1b[94;4m$&\x1b[39;24m");
    })
    .join("");
}

/** Strip ANSI escape sequences, for clipboard-bound text. */
export function stripAnsi(text: string): string {
  // eslint-disable-next-line no-control-regex
  return text.replace(/\x1b\[[0-9;?]*[A-Za-z]|\x1b\][^\x07]*\x07/g, "");
}

/** The criticality filter's levels, in ascending severity. */
export type Severity = "all" | "info" | "warn" | "error";

export const SEVERITY_RANK: Record<Severity, number> = {
  all: 0,
  info: 1,
  warn: 2,
  error: 3,
};

/**
 * Rank a single (ANSI-stripped) line. Unclassified output ranks 0.
 *
 * A level marker the tool wrote always wins: it is the author's own statement
 * of severity, and no heuristic beats that. Only a line carrying **no** marker
 * falls back to where it came from, and then only from `stderr` — a program
 * that writes to `stderr` at all is saying something went wrong, which is the
 * whole reason the stream exists.
 *
 * The fallback deliberately skips indented and blank lines. A stack trace's
 * frames are every one of them unmarked `stderr` lines, and ranking each on its
 * own would make "how many errors" the number of frames rather than the number
 * of failures; {@link filterConsoleLines} instead lets them inherit the
 * severity of the line they hang off.
 *
 * `meta` — this app's own `started`/`exited` banners — never falls back: those
 * lines are written by the console, not by the program.
 */
export function lineSeverity(line: string, stream?: LineStream): number {
  if (/^\s*(fail|crit):/.test(line) || /\berror( [A-Z]+\d+)?:/i.test(line)) return 3;
  if (/^\s*warn:/.test(line) || /\bwarning( [A-Z]+\d+)?:/i.test(line)) return 2;
  if (/^\s*info:/.test(line)) return 1;
  if (stream === "stderr" && line.trim() !== "" && !/^\s/.test(line)) return 3;
  return 0;
}

/**
 * Keep only the lines at or above `severity` that contain `text`.
 *
 * A line starting with whitespace inherits the previous line's severity, so a
 * stack trace stays attached to the `fail:` line that produced it.
 */
export function filterLines(raw: string, severity: Severity, text: string): string {
  const needle = text.toLowerCase();
  const threshold = SEVERITY_RANK[severity];
  const out: string[] = [];
  let current = 0;

  for (const line of raw.split(/\r?\n/)) {
    const plain = stripAnsi(line);
    if (!(/^\s/.test(plain) && plain.trim() !== "")) {
      current = lineSeverity(plain);
    } else if (lineSeverity(plain) > current) {
      current = lineSeverity(plain);
    }

    const matches =
      current >= threshold && (!needle || plain.toLowerCase().includes(needle));
    if (matches) out.push(line);
  }
  return out.join("\r\n");
}

/**
 * Which stream a stored line came from.
 *
 * `meta` is this app's own console furniture — the `started` and `exited`
 * banners `OutputConsole.handle` writes — kept distinct from the program's two
 * real streams so the stderr fallback in {@link lineSeverity} cannot mistake
 * the app's own words for the program's.
 */
export type LineStream = Stream | "meta";

/** One stored output line, with the stream that produced it. */
export interface ConsoleLine {
  text: string;
  stream: LineStream;
}

/**
 * Append a chunk of output to the stored lines.
 *
 * Output arrives in chunks that have nothing to do with line boundaries: a
 * single write can carry half a line, and the rest of it can arrive in the next
 * chunk. So the **last element is always the current, unterminated tail** — a
 * chunk ending in a newline leaves an empty tail behind, and the next chunk
 * continues into it. Without that, a line split across two chunks would be
 * classified twice, on two fragments neither of which need carry the marker
 * that decides the severity of either.
 *
 * A tail that has already taken text keeps the stream it started on: the line
 * belongs to whichever stream began it. Only an empty tail adopts the incoming
 * stream.
 *
 * `capLines` bounds the store, dropping from the left — the oldest output is
 * what a scrollback is allowed to lose.
 */
export function appendConsoleLines(
  prev: ConsoleLine[],
  stream: LineStream,
  text: string,
  capLines: number,
): ConsoleLine[] {
  if (text === "") return prev;

  const parts = text.split(/\r?\n/);
  const out = prev.length === 0 ? [{ text: "", stream }] : prev.slice();
  const tail = out[out.length - 1]!;

  // The first part continues the tail rather than starting a line of its own.
  out[out.length - 1] = {
    text: tail.text + parts[0]!,
    stream: tail.text === "" ? stream : tail.stream,
  };
  for (const part of parts.slice(1)) out.push({ text: part, stream });

  return out.length > capLines ? out.slice(out.length - capLines) : out;
}

/** Join stored lines back into the plain text a clipboard or a report wants. */
export function joinConsoleLines(lines: ConsoleLine[]): string {
  return lines.map((line) => line.text).join("\n");
}

/**
 * Keep only the stored lines at or above `severity` that contain `text`.
 *
 * The stream-aware counterpart of {@link filterLines}, and it keeps that
 * function's central rule: a line starting with whitespace **inherits** the
 * previous line's severity, so a stack trace stays attached to the `fail:` line
 * that produced it and does not vanish the moment the threshold rises. A
 * continuation that is itself *louder* than what it hangs off raises the
 * running severity rather than lowering it.
 */
export function filterConsoleLines(
  lines: ConsoleLine[],
  severity: Severity,
  text: string,
): string {
  const needle = text.toLowerCase();
  const threshold = SEVERITY_RANK[severity];
  const out: string[] = [];
  let current = 0;

  for (const line of lines) {
    const plain = stripAnsi(line.text);
    const rank = lineSeverity(plain, line.stream);
    const continuation = /^\s/.test(plain) && plain.trim() !== "";
    if (!continuation || rank > current) current = rank;

    const matches = current >= threshold && (!needle || plain.toLowerCase().includes(needle));
    if (matches) out.push(line.text);
  }
  return out.join("\r\n");
}

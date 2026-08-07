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

/** Rank a single (ANSI-stripped) line. Unclassified output ranks 0. */
export function lineSeverity(line: string): number {
  if (/^\s*(fail|crit):/.test(line) || /\berror( [A-Z]+\d+)?:/i.test(line)) return 3;
  if (/^\s*warn:/.test(line) || /\bwarning( [A-Z]+\d+)?:/i.test(line)) return 2;
  if (/^\s*info:/.test(line)) return 1;
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

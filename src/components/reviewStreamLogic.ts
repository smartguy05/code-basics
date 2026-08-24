//! Rendering Claude Code's `--output-format stream-json` NDJSON into readable
//! console text. Pure and DOM-free so it is tested under vitest (node env).
//!
//! Claude's headless text mode buffers the whole answer to the end, which looks
//! hung during a long review. Stream-json emits one JSON object per line as each
//! step happens; this turns those into what a reviewer wants to watch — the
//! agent's text and a `● Tool(arg)` line per action — and drops the rest
//! (session init noise, thinking, tool results, rate-limit chatter). Codex's
//! `exec` already streams human text, so it needs none of this.

const ESC = "\x1b[";
const dim = (s: string) => `${ESC}2m${s}${ESC}0m`;
const cyan = (s: string) => `${ESC}36m${s}${ESC}0m`;
const red = (s: string) => `${ESC}31m${s}${ESC}0m`;

/** xterm needs `\r\n`; model text carries bare `\n`. */
function toCrlf(text: string): string {
  return text.replace(/\r?\n/g, "\r\n");
}

/** A line splitter over a chunked stream: yields complete lines, holds the rest. */
export interface NdjsonBuffer {
  /** Feed a raw output chunk; returns the complete, non-empty lines it completed. */
  push(chunk: string): string[];
  /** At end of stream, return any trailing unterminated line (once). */
  flush(): string[];
}

export function createNdjsonBuffer(): NdjsonBuffer {
  let partial = "";
  const clean = (lines: string[]) =>
    lines.map((l) => l.replace(/\r$/, "")).filter((l) => l.length > 0);
  return {
    push(chunk: string): string[] {
      partial += chunk;
      const lines = partial.split("\n");
      partial = lines.pop() ?? "";
      return clean(lines);
    },
    flush(): string[] {
      const rest = clean([partial]);
      partial = "";
      return rest;
    },
  };
}

/** Concise one-line summary of a tool call's arguments. */
function summariseInput(input: unknown): string {
  if (!input || typeof input !== "object") return "";
  const o = input as Record<string, unknown>;
  const candidate =
    o.file_path ?? o.path ?? o.command ?? o.pattern ?? o.query ?? o.url ?? o.description;
  const s = typeof candidate === "string" ? candidate : JSON.stringify(input);
  return s.length > 80 ? `${s.slice(0, 77)}…` : s;
}

/**
 * Turn one NDJSON line into console text (with ANSI), or `null` to drop it.
 * Never throws — a non-JSON line is simply dropped.
 */
export function formatClaudeStream(line: string): string | null {
  let obj: { type?: string; [k: string]: unknown };
  try {
    obj = JSON.parse(line);
  } catch {
    return null;
  }

  switch (obj.type) {
    case "system": {
      if (obj.subtype === "init") {
        const model = typeof obj.model === "string" ? obj.model : "claude";
        const mode = typeof obj.permissionMode === "string" ? obj.permissionMode : "?";
        return `${dim(`▶ ${model} · permission-mode ${mode}`)}\r\n`;
      }
      return null; // hook_*, thinking_tokens, etc.
    }

    case "assistant": {
      const message = obj.message as { content?: unknown } | undefined;
      const blocks = message?.content;
      if (!Array.isArray(blocks)) return null;
      const parts: string[] = [];
      for (const b of blocks as Array<Record<string, unknown>>) {
        if (b.type === "text" && typeof b.text === "string" && b.text.trim()) {
          parts.push(toCrlf(b.text));
        } else if (b.type === "tool_use" && typeof b.name === "string") {
          parts.push(`${cyan(`● ${b.name}`)}${dim(`(${summariseInput(b.input)})`)}`);
        }
        // thinking / other block types are intentionally skipped.
      }
      return parts.length ? `${parts.join("\r\n")}\r\n` : null;
    }

    case "result": {
      const ms = typeof obj.duration_ms === "number" ? obj.duration_ms : null;
      const secs = ms == null ? "?" : (ms / 1000).toFixed(1);
      const cost =
        typeof obj.total_cost_usd === "number" ? ` · $${obj.total_cost_usd.toFixed(4)}` : "";
      const ok = obj.is_error !== true && obj.subtype !== "error";
      // The result text duplicates the last assistant message — already printed.
      return ok
        ? `${dim(`✓ review complete in ${secs}s${cost}`)}\r\n`
        : `${red(`✗ review failed (${typeof obj.subtype === "string" ? obj.subtype : "error"})`)}\r\n`;
    }

    default:
      return null; // user (tool_result), rate_limit_event, stream_event, …
  }
}

/**
 * Whether a Claude stream line signals the run needs the user's attention — the
 * closest thing a headless review has to "requires input": a permission was
 * denied, or a tool was blocked. Used to flash the minimized pill.
 */
export function claudeLineNeedsAttention(line: string): boolean {
  let obj: { type?: string; [k: string]: unknown };
  try {
    obj = JSON.parse(line);
  } catch {
    return false;
  }

  if (obj.type === "result") {
    return Array.isArray(obj.permission_denials) && obj.permission_denials.length > 0;
  }

  if (obj.type === "user") {
    const message = obj.message as { content?: unknown } | undefined;
    const content = message?.content;
    if (Array.isArray(content)) {
      for (const b of content as Array<Record<string, unknown>>) {
        if (b.type === "tool_result" && b.is_error === true) {
          const text =
            typeof b.content === "string" ? b.content : JSON.stringify(b.content ?? "");
          if (/permission|requires approval|not allowed|denied/i.test(text)) return true;
        }
      }
    }
  }

  return false;
}

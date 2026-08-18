import { describe, expect, it } from "vitest";
import { createNdjsonBuffer, formatClaudeStream } from "./reviewStreamLogic";

// Strip ANSI so assertions read on the visible text.
const plain = (s: string | null) => (s == null ? null : s.replace(/\x1b\[[0-9;]*m/g, ""));

describe("createNdjsonBuffer", () => {
  it("yields only complete lines and holds the partial", () => {
    const buf = createNdjsonBuffer();
    expect(buf.push('{"a":1}\n{"b":2}\n{"c"')).toEqual(['{"a":1}', '{"b":2}']);
    // The dangling '{"c"' is held until its newline arrives.
    expect(buf.push(':3}\n')).toEqual(['{"c":3}']);
  });

  it("tolerates CRLF and drops blank lines", () => {
    const buf = createNdjsonBuffer();
    expect(buf.push('{"a":1}\r\n\r\n{"b":2}\n')).toEqual(['{"a":1}', '{"b":2}']);
  });

  it("flush returns a trailing unterminated line, then nothing", () => {
    const buf = createNdjsonBuffer();
    expect(buf.push('{"a":1}')).toEqual([]);
    expect(buf.flush()).toEqual(['{"a":1}']);
    expect(buf.flush()).toEqual([]);
  });
});

describe("formatClaudeStream", () => {
  it("renders an init header with model and permission mode", () => {
    const out = plain(
      formatClaudeStream(JSON.stringify({ type: "system", subtype: "init", model: "claude-opus", permissionMode: "plan" })),
    );
    expect(out).toContain("claude-opus");
    expect(out).toContain("plan");
  });

  it("skips hook and thinking-token system noise", () => {
    expect(formatClaudeStream(JSON.stringify({ type: "system", subtype: "hook_started" }))).toBeNull();
    expect(formatClaudeStream(JSON.stringify({ type: "system", subtype: "thinking_tokens", estimated_tokens: 50 }))).toBeNull();
  });

  it("prints assistant text with CRLF line endings for xterm", () => {
    const out = formatClaudeStream(
      JSON.stringify({ type: "assistant", message: { content: [{ type: "text", text: "line one\nline two" }] } }),
    );
    expect(out).not.toBeNull();
    expect(plain(out)).toContain("line one");
    expect(plain(out)).toContain("line two");
    expect(out).toContain("\r\n");
    expect(out).not.toMatch(/[^\r]\n/); // every \n is preceded by \r
  });

  it("renders a tool_use as a bullet with a concise argument summary", () => {
    const out = plain(
      formatClaudeStream(
        JSON.stringify({ type: "assistant", message: { content: [{ type: "tool_use", name: "Read", input: { file_path: "src/app.ts" } }] } }),
      ),
    );
    expect(out).toContain("Read");
    expect(out).toContain("src/app.ts");
  });

  it("skips assistant thinking blocks", () => {
    expect(
      formatClaudeStream(
        JSON.stringify({ type: "assistant", message: { content: [{ type: "thinking", thinking: "", signature: "x" }] } }),
      ),
    ).toBeNull();
  });

  it("skips tool_result (user) and rate-limit events", () => {
    expect(formatClaudeStream(JSON.stringify({ type: "user", message: { content: [{ type: "tool_result" }] } }))).toBeNull();
    expect(formatClaudeStream(JSON.stringify({ type: "rate_limit_event", rate_limit_info: {} }))).toBeNull();
  });

  it("renders a success footer with duration and cost", () => {
    const out = plain(
      formatClaudeStream(JSON.stringify({ type: "result", subtype: "success", is_error: false, duration_ms: 14203, total_cost_usd: 0.2162 })),
    );
    expect(out).toContain("14.2");
    expect(out).toContain("$0.21");
  });

  it("renders an error footer distinctly", () => {
    const out = plain(formatClaudeStream(JSON.stringify({ type: "result", subtype: "error", is_error: true, duration_ms: 1000 })));
    expect(out?.toLowerCase()).toContain("fail");
  });

  it("does not duplicate the final answer (result text is not printed)", () => {
    const out = plain(
      formatClaudeStream(JSON.stringify({ type: "result", subtype: "success", is_error: false, duration_ms: 100, result: "THE WHOLE ANSWER" })),
    );
    expect(out).not.toContain("THE WHOLE ANSWER");
  });

  it("returns null for a non-JSON line rather than throwing", () => {
    expect(formatClaudeStream("not json at all")).toBeNull();
    expect(formatClaudeStream("")).toBeNull();
  });
});

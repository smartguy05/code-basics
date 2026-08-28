import { describe, expect, it } from "vitest";
import {
  appendConsoleLines,
  decorate,
  filterConsoleLines,
  filterLines,
  joinConsoleLines,
  lineSeverity,
  stripAnsi,
} from "./consoleLogic";
import type { ConsoleLine } from "./consoleLogic";

const ESC = "\x1b";

describe("stripAnsi", () => {
  it("removes CSI colour sequences and keeps the text", () => {
    expect(stripAnsi(`${ESC}[32minfo${ESC}[39m: ready`)).toBe("info: ready");
  });

  it("removes an OSC hyperlink sequence terminated by BEL", () => {
    expect(stripAnsi(`${ESC}]8;;https://x.test\x07link`)).toBe("link");
  });

  it("leaves text with no escapes untouched, including newlines", () => {
    expect(stripAnsi("plain\r\nlines")).toBe("plain\r\nlines");
  });

  it("removes cursor-movement and erase sequences too", () => {
    expect(stripAnsi(`a${ESC}[2K${ESC}[1Gb`)).toBe("ab");
  });

  it("keeps a lone ESC that starts no recognised sequence", () => {
    expect(stripAnsi(`${ESC}Mtext`)).toBe(`${ESC}Mtext`);
  });
});

describe("decorate", () => {
  it("colours the .NET console logger's level prefixes", () => {
    expect(decorate("info: Foo[0]")).toBe(`${ESC}[32minfo${ESC}[39m: Foo[0]`);
    expect(decorate("warn: Foo[0]")).toBe(`${ESC}[33mwarn${ESC}[39m: Foo[0]`);
    expect(decorate("fail: Foo[0]")).toBe(`${ESC}[1;31mfail${ESC}[22;39m: Foo[0]`);
    expect(decorate("trce: Foo[0]")).toBe(`${ESC}[90mtrce${ESC}[39m: Foo[0]`);
  });

  it("keeps the indentation of an indented level prefix", () => {
    expect(decorate("  warn: hi")).toBe(`  ${ESC}[33mwarn${ESC}[39m: hi`);
  });

  it("colours MSBuild diagnostics mid-line", () => {
    expect(decorate("Foo.cs(1,2): error CS1234: bad")).toBe(
      `Foo.cs(1,2): ${ESC}[1;31merror CS1234${ESC}[22;39m: bad`,
    );
    expect(decorate("Foo.cs(1,2): warning CS0168: unused")).toBe(
      `Foo.cs(1,2): ${ESC}[33mwarning CS0168${ESC}[39m: unused`,
    );
  });

  it("underlines URLs in bright blue", () => {
    expect(decorate("Now listening on: https://localhost:5001 ok")).toBe(
      `Now listening on: ${ESC}[94;4mhttps://localhost:5001${ESC}[39;24m ok`,
    );
  });

  it("colours several URLs on one line", () => {
    expect(decorate("a http://x.test b http://y.test")).toBe(
      `a ${ESC}[94;4mhttp://x.test${ESC}[39;24m b ${ESC}[94;4mhttp://y.test${ESC}[39;24m`,
    );
  });

  it("leaves a line that already carries ANSI styling alone", () => {
    const already = `${ESC}[31mfail: https://x.test${ESC}[0m`;
    expect(decorate(already)).toBe(already);
  });

  it("applies only the first matching severity rule", () => {
    // `warn:` wins over the later `\berror ...:` rule on the same line.
    expect(decorate("warn: error CS1: nested")).toBe(
      `${ESC}[33mwarn${ESC}[39m: error CS1: nested`,
    );
  });

  it("preserves line separators exactly, decorating each line", () => {
    expect(decorate("info: a\r\nwarn: b\nplain")).toBe(
      `${ESC}[32minfo${ESC}[39m: a\r\n${ESC}[33mwarn${ESC}[39m: b\nplain`,
    );
  });

  it("leaves unclassified text untouched", () => {
    expect(decorate("Build succeeded.")).toBe("Build succeeded.");
    expect(decorate("")).toBe("");
  });
});

describe("lineSeverity", () => {
  it("ranks fail/crit prefixes and error diagnostics highest", () => {
    expect(lineSeverity("fail: boom")).toBe(3);
    expect(lineSeverity("   crit: boom")).toBe(3);
    expect(lineSeverity("Foo.cs(1,2): error CS1234: bad")).toBe(3);
    expect(lineSeverity("Error: something")).toBe(3);
  });

  it("ranks warnings second", () => {
    expect(lineSeverity("warn: careful")).toBe(2);
    expect(lineSeverity("Foo.cs(1,2): warning CS0168: unused")).toBe(2);
  });

  it("ranks info third and everything else zero", () => {
    expect(lineSeverity("info: ready")).toBe(1);
    expect(lineSeverity("Build succeeded.")).toBe(0);
    expect(lineSeverity("")).toBe(0);
    expect(lineSeverity("   at Foo.Bar()")).toBe(0);
  });

  it("does not classify a word that merely contains a level name", () => {
    expect(lineSeverity("information: not a prefix")).toBe(0);
    expect(lineSeverity("failure: not a prefix")).toBe(0);
  });
});

describe("filterLines", () => {
  it("keeps everything at the `all` level with no text", () => {
    expect(filterLines("a\nb\nc", "all", "")).toBe("a\r\nb\r\nc");
  });

  it("drops lines below the threshold", () => {
    const raw = "info: ok\nwarn: hmm\nfail: boom";
    expect(filterLines(raw, "warn", "")).toBe("warn: hmm\r\nfail: boom");
    expect(filterLines(raw, "error", "")).toBe("fail: boom");
  });

  it("keeps an indented continuation at the severity of the line above", () => {
    const raw = "fail: boom\n   at Foo.Bar()\n   at Baz()\ninfo: ok";
    expect(filterLines(raw, "error", "")).toBe(
      "fail: boom\r\n   at Foo.Bar()\r\n   at Baz()",
    );
  });

  it("lets an indented line raise the inherited severity", () => {
    const raw = "Build started\n   error CS1234: bad\n   at Foo()";
    expect(filterLines(raw, "error", "")).toBe("   error CS1234: bad\r\n   at Foo()");
  });

  it("resets the inherited severity on a blank line", () => {
    const raw = "fail: boom\n   at Foo()\n\n   at Bar()";
    expect(filterLines(raw, "error", "")).toBe("fail: boom\r\n   at Foo()");
  });

  it("resets the inherited severity on the next unindented line", () => {
    const raw = "fail: boom\n   at Foo()\nBuild succeeded.\n   trailing";
    expect(filterLines(raw, "error", "")).toBe("fail: boom\r\n   at Foo()");
  });

  it("matches the text case-insensitively", () => {
    const raw = "alpha\nBETA\ngamma";
    expect(filterLines(raw, "all", "beta")).toBe("BETA");
    expect(filterLines(raw, "all", "AlPh")).toBe("alpha");
  });

  it("requires both the threshold and the text", () => {
    const raw = "info: boom\nfail: boom\nfail: quiet";
    expect(filterLines(raw, "error", "boom")).toBe("fail: boom");
  });

  it("matches against the stripped text but emits the original ANSI line", () => {
    const raw = `${ESC}[31mfail${ESC}[0m: boom`;
    expect(filterLines(raw, "all", "fail: boom")).toBe(raw);
  });

  it("classifies severity from the stripped text, not the escapes", () => {
    const raw = `${ESC}[33mwarn${ESC}[39m: careful\nplain`;
    expect(filterLines(raw, "warn", "")).toBe(`${ESC}[33mwarn${ESC}[39m: careful`);
  });

  it("splits on CRLF as well as LF and rejoins with CRLF", () => {
    expect(filterLines("a\r\nb", "all", "")).toBe("a\r\nb");
  });

  it("returns an empty string when nothing matches", () => {
    expect(filterLines("alpha\nbeta", "all", "zzz")).toBe("");
    expect(filterLines("", "error", "")).toBe("");
  });
});

describe("lineSeverity with a stream", () => {
  it("prefers the tool's own marker over the stream it arrived on", () => {
    // A .NET app writes its whole log to stdout; a marked line still ranks.
    expect(lineSeverity("fail: boom", "stdout")).toBe(3);
    expect(lineSeverity("warn: slow", "stdout")).toBe(2);
    // And a marker on stderr is not promoted past what it says.
    expect(lineSeverity("warn: slow", "stderr")).toBe(2);
    expect(lineSeverity("info: ready", "stderr")).toBe(1);
  });

  it("falls back to error for an unmarked stderr line", () => {
    expect(lineSeverity("Unhandled exception.", "stderr")).toBe(3);
  });

  it("does not fall back for stdout, meta, or an absent stream", () => {
    expect(lineSeverity("Unhandled exception.", "stdout")).toBe(0);
    expect(lineSeverity("exited with code 1", "meta")).toBe(0);
    expect(lineSeverity("Unhandled exception.")).toBe(0);
  });

  it("leaves indented and blank stderr lines unranked, for the inheritance rule", () => {
    // Every frame of a stack trace is an unmarked stderr line. Ranking each one
    // would count frames rather than failures; filterConsoleLines instead lets
    // them inherit from the line they hang off.
    expect(lineSeverity("   at Foo.Bar()", "stderr")).toBe(0);
    expect(lineSeverity("", "stderr")).toBe(0);
    expect(lineSeverity("   ", "stderr")).toBe(0);
  });
});

describe("appendConsoleLines", () => {
  const CAP = 100;

  it("keeps an unterminated chunk as the tail and continues into it", () => {
    // The bug this exists to prevent: a line split across two chunks being
    // classified twice, on fragments neither of which carries the marker.
    let lines = appendConsoleLines([], "stdout", "fai", CAP);
    lines = appendConsoleLines(lines, "stdout", "l: boom\n", CAP);

    expect(lines.map((l) => l.text)).toEqual(["fail: boom", ""]);
    expect(lineSeverity(lines[0]!.text, lines[0]!.stream)).toBe(3);
  });

  it("leaves an empty tail behind a chunk that ends in a newline", () => {
    const lines = appendConsoleLines([], "stdout", "a\nb\n", CAP);
    expect(lines.map((l) => l.text)).toEqual(["a", "b", ""]);
  });

  it("splits on either line ending", () => {
    const lines = appendConsoleLines([], "stdout", "a\r\nb\nc", CAP);
    expect(lines.map((l) => l.text)).toEqual(["a", "b", "c"]);
  });

  it("keeps the stream the line started on, and adopts it into an empty tail", () => {
    let lines = appendConsoleLines([], "stderr", "half ", CAP);
    lines = appendConsoleLines(lines, "stdout", "a line\n", CAP);
    expect(lines[0]).toEqual({ text: "half a line", stream: "stderr" });

    // The tail left behind is empty, so the next chunk's stream is the truth.
    lines = appendConsoleLines(lines, "stdout", "next", CAP);
    expect(lines[1]).toEqual({ text: "next", stream: "stdout" });
  });

  it("drops the oldest lines past the cap", () => {
    let lines: ConsoleLine[] = [];
    for (let i = 0; i < 6; i += 1) lines = appendConsoleLines(lines, "stdout", `${i}\n`, 3);
    expect(lines.map((l) => l.text)).toEqual(["4", "5", ""]);
  });

  it("returns the same array for an empty chunk", () => {
    const lines = appendConsoleLines([], "stdout", "a", CAP);
    expect(appendConsoleLines(lines, "stdout", "", CAP)).toBe(lines);
  });
});

const CAP_JOIN = 100;

describe("joinConsoleLines", () => {
  it("round-trips text that was appended in awkward chunks", () => {
    let lines = appendConsoleLines([], "stdout", "one\ntw", CAP_JOIN);
    lines = appendConsoleLines(lines, "stdout", "o\nthree", CAP_JOIN);
    expect(joinConsoleLines(lines)).toBe("one\ntwo\nthree");
  });
});

describe("filterConsoleLines", () => {
  const lines = (...specs: [string, ConsoleLine["stream"]][]): ConsoleLine[] =>
    specs.map(([text, stream]) => ({ text, stream }));

  it("keeps an unmarked stderr line at the error threshold", () => {
    const input = lines(
      ["ready", "stdout"],
      ["Unhandled exception.", "stderr"],
    );
    expect(filterConsoleLines(input, "error", "")).toBe("Unhandled exception.");
  });

  it("keeps a stack trace attached to the stderr line that produced it", () => {
    const input = lines(
      ["ready", "stdout"],
      ["Unhandled exception.", "stderr"],
      ["   at Foo.Bar()", "stderr"],
      ["   at Baz.Qux()", "stderr"],
      ["still going", "stdout"],
    );
    expect(filterConsoleLines(input, "error", "")).toBe(
      "Unhandled exception.\r\n   at Foo.Bar()\r\n   at Baz.Qux()",
    );
  });

  it("does not promote a meta banner to an error", () => {
    const input = lines(["exited with code 1", "meta"], ["warn: slow", "stdout"]);
    expect(filterConsoleLines(input, "error", "")).toBe("");
    expect(filterConsoleLines(input, "warn", "")).toBe("warn: slow");
  });

  it("keeps everything at the 'all' threshold", () => {
    const input = lines(["a", "stdout"], ["b", "stderr"]);
    expect(filterConsoleLines(input, "all", "")).toBe("a\r\nb");
  });

  it("still applies the text needle", () => {
    const input = lines(["fail: alpha", "stdout"], ["fail: beta", "stdout"]);
    expect(filterConsoleLines(input, "error", "beta")).toBe("fail: beta");
  });

  it("lets a louder continuation raise the running severity", () => {
    const input = lines(["info: starting", "stdout"], ["  error CS1234: broke", "stdout"]);
    expect(filterConsoleLines(input, "error", "")).toBe("  error CS1234: broke");
  });
});

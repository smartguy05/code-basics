import { describe, expect, it } from "vitest";
import { diagramBody, inDocumentFrame } from "./frontMatterLogic";
import type { ValidationError } from "../../ipc/types";

/**
 * The exact bytes `store::render` writes, reproduced from a real run of
 * `store::render` + `mermaid::validate` over this repository's own core:
 *
 *   FILE  : "---\ncode-basics: v1\nderivation: user\n---\n\nflowchart LR\n…"
 *   WHOLE : Err(ValidationError { rule: DiagramType, line: 1,
 *           detail: "'---' is not a diagram type this app renders" })
 *   BODY  : Ok(())
 */
const STORED =
  "---\ncode-basics: v1\nderivation: user\n---\n\nflowchart LR\n  A[\"Api\"] --> B[\"Core\"]\n";

describe("diagramBody", () => {
  it("drops the front matter block a stored diagram opens with", () => {
    expect(diagramBody(STORED).body).toBe('flowchart LR\n  A["Api"] --> B["Core"]\n');
  });

  it("counts the lines it dropped, so a line number can be put back", () => {
    // `---`, two keys, `---`, and the single blank line `render` writes.
    expect(diagramBody(STORED).offset).toBe(5);
  });

  it("leaves a bare diagram exactly as it is", () => {
    const bare = "flowchart LR\n  A --> B\n";
    expect(diagramBody(bare)).toEqual({ body: bare, offset: 0 });
  });

  it("leaves a fenced markdown document alone — the validator handles fences", () => {
    const doc = "# Notes\n\n```mermaid\nflowchart LR\n  A --> B\n```\n";
    expect(diagramBody(doc)).toEqual({ body: doc, offset: 0 });
  });

  it("does not treat a `---` further down the file as an opening delimiter", () => {
    const doc = "flowchart LR\n---\n  A --> B\n";
    expect(diagramBody(doc)).toEqual({ body: doc, offset: 0 });
  });

  it("keeps an unterminated block, because nothing separated it from the body", () => {
    const doc = "---\ncode-basics: v1\nflowchart LR\n";
    expect(diagramBody(doc)).toEqual({ body: doc, offset: 0 });
  });

  it("strips a block this version cannot read the keys of, as the renderer does", () => {
    // `store::parse` hands such a file back whole and calls it a user diagram,
    // but Mermaid strips the block on the way to drawing it, so what renders
    // is the body — and the check has to agree with the picture.
    const doc = "---\nunknown: yes\n---\nflowchart LR\n";
    expect(diagramBody(doc)).toEqual({ body: "flowchart LR\n", offset: 3 });
  });

  it("keeps the author's own blank lines, dropping only the one render wrote", () => {
    const doc = "---\ncode-basics: v1\n---\n\n\nflowchart LR\n";
    expect(diagramBody(doc)).toEqual({ body: "\nflowchart LR\n", offset: 4 });
  });

  it("survives CRLF, which is what a Windows editor saves", () => {
    const doc = "---\r\ncode-basics: v1\r\n---\r\n\r\nflowchart LR\r\n";
    expect(diagramBody(doc).body).toBe("flowchart LR\n");
    expect(diagramBody(doc).offset).toBe(4);
  });
});

describe("inDocumentFrame", () => {
  const problem: ValidationError = { rule: "diagramType", line: 3, detail: "no" };

  it("is null for source that renders", () => {
    expect(inDocumentFrame(null, 5)).toBeNull();
  });

  it("moves the line back into the frame of the whole file", () => {
    expect(inDocumentFrame(problem, 5)?.line).toBe(8);
  });

  it("changes nothing else about what the validator said", () => {
    const moved = inDocumentFrame(problem, 5);
    expect(moved).toEqual({ rule: "diagramType", line: 8, detail: "no" });
  });

  it("is the identity when there was no front matter", () => {
    expect(inDocumentFrame(problem, 0)).toEqual(problem);
  });
});

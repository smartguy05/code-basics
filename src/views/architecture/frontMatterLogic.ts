import type { ValidationError } from "../../ipc/types";

/**
 * The part of a stored diagram that is actually the diagram, and how far down
 * the file it starts.
 *
 * # The defect this exists to close
 *
 * `arch_read_diagram` returns the file *exactly as it is on disk*, front
 * matter included, and `mermaid::validate` documents itself as taking the
 * post-front-matter body — `store::parse` is what separates the two on the
 * Rust side. Handing the whole file to `arch_validate` therefore asks the
 * validator to read `---` as a diagram type, which it refuses. Executed
 * against this repository's own `store::render` output:
 *
 *     FILE  : "---\ncode-basics: v1\nderivation: user\n---\n\nflowchart LR\n…"
 *     WHOLE : Err(ValidationError { rule: DiagramType, line: 1,
 *             detail: "'---' is not a diagram type this app renders" })
 *     BODY  : Ok(())
 *
 * That is every diagram this app has ever written, including the copy the
 * "Save a copy…" button has just made — while Mermaid, whose own front matter
 * regex strips the block, draws them perfectly well a pane above. A permanent
 * "this will not render" under a picture that plainly does render is the
 * confidently-wrong label the whole feature is built to avoid.
 *
 * # Why the stripping is here and not in the command
 *
 * The tempting fix is to have `arch_read_diagram` hand back the body. It was
 * rejected: the editor edits the **file**, and `store::write` reads `level`,
 * `generated` and `sourceCommit` out of the text it is given (only the
 * derivation is taken from disk). An editor opened on a body would save a file
 * whose front matter had quietly lost those keys — a data loss, to fix a
 * label. Keeping the whole file in the buffer and narrowing only the *check*
 * costs one pure function, and leaves the on-disk contract — "exactly as it is
 * on disk" — as it is, with `nodeTargets.ts` and the canvas already relying on
 * it.
 *
 * # What counts as front matter here
 *
 * The delimiters, and nothing else: an opening line that is exactly `---` and
 * the first line after it that trims to `---`. This deliberately mirrors
 * `store::split` and Mermaid's own front-matter regex rather than
 * `store::read_keys`, because the question being answered is *what will the
 * renderer see*, not *what does this app understand*. A block whose keys this
 * version cannot read is still a block Mermaid removes before drawing, so the
 * check has to agree with the picture. Anything that is not a terminated
 * leading block is left completely alone — a fenced markdown document is the
 * validator's own business, and it handles fences itself.
 */
export interface DiagramBody {
  /** The text the renderer will see. */
  body: string;
  /** How many lines were removed from the top to get there. */
  offset: number;
}

/** Split a stored diagram into the body that renders and the lines above it. */
export function diagramBody(text: string): DiagramBody {
  const lines = text.split(/\r?\n/);
  // Exactly `---`, as `store::split`'s `strip_prefix("---\n")` requires: an
  // indented or decorated first line is not a front matter opener.
  if (lines[0] !== "---") return { body: text, offset: 0 };

  const close = lines.findIndex((line, index) => index > 0 && line.trim() === "---");
  // Never terminated, so nothing separated a header from a body and there is
  // no body to take. Left whole, exactly as `store::split` leaves it.
  if (close === -1) return { body: text, offset: 0 };

  // Drop the single blank line `store::render` writes between the two, and no
  // more — blank lines the author put there are theirs, and eating one would
  // shift every line number reported below it.
  const start = lines[close + 1] === "" ? close + 2 : close + 1;
  return { body: lines.slice(start).join("\n"), offset: start };
}

/**
 * Move a problem found in the body back into the frame of the whole file.
 *
 * `ValidationError.line` is documented as 1-based **in the source as given**,
 * so a check run over the body reports lines in the body's frame while the
 * editor holds the file. Showing that number, or scrolling to it, would point
 * at a line of front matter — off by exactly the block that was removed.
 */
export function inDocumentFrame(
  problem: ValidationError | null,
  offset: number,
): ValidationError | null {
  if (problem === null) return null;
  return { ...problem, line: problem.line + offset };
}

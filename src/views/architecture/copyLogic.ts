/**
 * The one decision the "save a copy" button makes: what the copy is called,
 * and whether that name is free.
 *
 * Small, and here rather than in the view because of what it prevents.
 * `store::write` treats a save under an existing name as an *edit of that
 * file*: it keeps the provenance already on disk and replaces the body. So a
 * copy saved under a name somebody else's diagram already has does not fail, it
 * silently overwrites a drawing. The check that stops that has to compare the
 * name the backend will actually file the copy under, which means computing the
 * extension exactly as `store::file_name` does — and that is a rule worth
 * pinning to a test rather than repeating from memory inside a component.
 *
 * Nothing here re-derives the backend's *safety* checks. `file_name` already
 * refuses separators, drive prefixes and names made of dots, with a message
 * that says which, and a second implementation of that in TypeScript would be
 * one more thing to keep in agreement with the one that matters. A hostile
 * name reaches the command and comes back rejected.
 */

/** The chosen name, or the reason it was not chosen. */
export type CopyName =
  | { ok: true; name: string }
  | { ok: false; reason: string };

/** Anything with a file name — a `DiagramFile` satisfies it unchanged. */
interface NamedFile {
  name: string;
}

/**
 * Work out what a copy would be called, and refuse if that name is taken.
 *
 * The extension is appended only when there is not already one, and "already
 * one" is judged case-insensitively because that is how `store::file_name`
 * judges it — appending to `Map.MD` would produce `Map.MD.md`, a file under a
 * name the user did not type and one that would then miss every collision
 * check made against what they did type.
 *
 * A collision is also judged case-insensitively, and deliberately over-refuses:
 * on Windows `NOTES.md` and `notes.md` are one file and the save would replace
 * it, while on Linux they are two. Refusing on both platforms costs the user a
 * second attempt at a name; allowing it on the platform that disagrees costs
 * them a diagram.
 */
export function copyDiagramName(
  raw: string,
  existing: readonly NamedFile[],
): CopyName {
  const trimmed = raw.trim();
  if (trimmed === "") {
    return { ok: false, reason: "Give the copy a name before saving it." };
  }

  const name = hasMarkdownExtension(trimmed) ? trimmed : `${trimmed}.md`;
  const clash = existing.find(
    (file) => file.name.toLowerCase() === name.toLowerCase(),
  );
  if (clash) {
    return {
      ok: false,
      reason:
        `${clash.name} already exists in this workspace. Saving over it would replace ` +
        `a diagram somebody drew — it would be treated as an edit of that file, not as ` +
        `a new one — so choose another name.`,
    };
  }

  return { ok: true, name };
}

/** True when the name already ends in a Markdown extension, in any case. */
function hasMarkdownExtension(name: string): boolean {
  const dot = name.lastIndexOf(".");
  // A leading dot is not an extension: `.md` is a name with nothing in it to
  // name a diagram, and the backend says so.
  if (dot <= 0) return false;
  return name.slice(dot + 1).toLowerCase() === "md";
}

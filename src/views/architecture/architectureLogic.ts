/**
 * The Architecture tab's list of diagrams, and the two labels that sit beside
 * one — with no React and no DOM in sight.
 *
 * Everything here is a function of its arguments. The view around it stays a
 * rendering shell, which is the only way any of this gets tested: the vitest
 * suite runs in a node environment with no jsdom, so nothing in this file may
 * touch an element, an event or a mermaid renderer.
 *
 * # Why a list of named diagrams and not a level selector
 *
 * The obvious control for two derived views of one workspace is a zoom or a
 * "level" slider, and it was rejected deliberately. An audit of what the two
 * graphs actually contain established that they are **different pictures, not
 * two magnifications of one**: the component map
 * (`architecture::components::component_graph`) drops solution grouping and
 * every `projectReference` arrow, and adds `dataStore` boxes that appear
 * nowhere in the project map. Nothing is a zoomed-in version of anything.
 *
 * Presented as levels, a user who opens only "level 2" loses the compile-time
 * structure of their solution and has no way to know a level exists that would
 * have shown it — the tool would be hiding a whole picture behind a control
 * that promises only detail. Presented as two names in a list, both are
 * visible, both are one click away, and the difference between them is
 * something the interface can state in a sentence ({@link BUILTIN_DIAGRAMS}
 * carries that sentence) rather than something the user has to infer from a
 * number.
 *
 * The saved diagrams from `arch_list_diagrams` join the same list underneath,
 * because to a reader they are the same kind of thing: a picture of this
 * workspace they can open. They are *not* the same kind of thing to the tool —
 * see {@link DiagramEntry.source}.
 */

/** Which of the two derived graphs a built-in entry stands for. */
export type BuiltinDiagram = "project" | "component";

/** Where an entry in the list came from. */
export type DiagramSource = "builtin" | "saved";

/** One of the two derived diagrams, and what it is for. */
export interface BuiltinDiagramInfo {
  builtin: BuiltinDiagram;
  label: string;
  /**
   * One sentence saying what this picture shows and what it leaves out. This
   * is the whole argument against a level selector made visible: the two are
   * different questions, so each one states its question.
   */
  description: string;
}

/**
 * The two derived diagrams, in the order they are listed.
 *
 * The project map is first because it is the one that can be checked against
 * the files on disk line by line — it is derived from manifest literals, and
 * every arrow in it is something an author wrote down. The component map is
 * inference over signals, admitted under a stricter gate precisely because it
 * is inference, and it belongs second for the same reason.
 */
export const BUILTIN_DIAGRAMS: readonly BuiltinDiagramInfo[] = [
  {
    builtin: "project",
    label: "Project map",
    description:
      "Every project the scan found, grouped by solution and workspace, with the references their manifests declare.",
  },
  {
    builtin: "component",
    label: "Component map",
    description:
      "Services and the data stores they declare a client for. Not a zoomed-in project map: solution grouping and project references are absent, and the boxes are inferred rather than read.",
  },
];

/**
 * One row of the diagram list.
 *
 * `source` is the field that matters, and it exists because a label is not
 * enough to tell the two apart: nothing stops a user saving a hand-drawn file
 * called `Project map.md`, and if the list distinguished entries by their text
 * alone that file would be indistinguishable from the derived graph the tool
 * computes. They behave completely differently — one is recomputed from the
 * manifests on every open and cannot be edited, the other is bytes on disk
 * that can be — so the difference is carried structurally, in `source`, in
 * the mutually exclusive `builtin`/`file` payloads, and in an `id` whose
 * namespaces cannot collide.
 *
 * Generic over the saved file so this module needs no import from `ipc/types`
 * — a `DiagramFile` satisfies `{ name: string }` and comes back out with its
 * own type intact, which is what lets the tests pass a real one and prove the
 * two shapes still agree.
 */
export interface DiagramEntry<T extends { name: string } = { name: string }> {
  /** Unique within the list; `builtin:…` and `saved:…` cannot collide. */
  id: string;
  source: DiagramSource;
  label: string;
  /** The sentence from {@link BUILTIN_DIAGRAMS}, or `null` for a saved file. */
  description: string | null;
  /** Which derived graph to ask for; `null` on a saved entry. */
  builtin: BuiltinDiagram | null;
  /** The file to read; `null` on a built-in entry. */
  file: T | null;
}

/**
 * The two built-ins followed by the saved diagrams, deterministically ordered.
 *
 * Saved diagrams are sorted by file name with a plain code-unit comparison
 * rather than `localeCompare`, which is locale- and ICU-version-dependent and
 * would put the same workspace's diagrams in a different order on two
 * machines. A list that reshuffles is a list nobody builds muscle memory for.
 *
 * The input is never mutated — `arch_list_diagrams`' array belongs to the
 * caller's state, and sorting it in place would reorder it under React.
 */
export function diagramEntries<T extends { name: string }>(
  saved: readonly T[],
): DiagramEntry<T>[] {
  const builtins: DiagramEntry<T>[] = BUILTIN_DIAGRAMS.map((info) => ({
    id: `builtin:${info.builtin}`,
    source: "builtin" as const,
    label: info.label,
    description: info.description,
    builtin: info.builtin,
    file: null,
  }));

  const files = [...saved]
    .sort((a, b) => (a.name < b.name ? -1 : a.name > b.name ? 1 : 0))
    .map((item) => ({
      id: `saved:${item.name}`,
      source: "saved" as const,
      label: diagramLabel(item.name),
      description: null,
      builtin: null,
      file: item,
    }));

  return [...builtins, ...files];
}

/**
 * The name a saved diagram is listed under.
 *
 * The `.md` is dropped because every one of these files has it and a suffix
 * repeated on every row carries no information. Any other extension is left
 * alone: it is unusual enough that it is probably telling the reader
 * something, and trimming a suffix this function did not expect would be
 * editing a name rather than tidying it.
 */
function diagramLabel(name: string): string {
  return name.endsWith(".md") ? name.slice(0, -".md".length) : name;
}

/**
 * The three shapes a derivation can arrive in.
 *
 * Both `Derivation` (a graph's) and `DiagramDerivation` (a stored file's) are
 * accepted, because the badge says the same thing about either and a reader
 * does not care which command the value came back from. They differ in one
 * place only: the graph's `derived` carries the scanner version, the file's is
 * a bare `"derived"`. Written structurally rather than imported for the reason
 * given at the top of this file.
 */
type DerivationLike =
  | "derived"
  | "user"
  | { derived: { scanner: number } }
  | { inferred: { agent: string } };

/**
 * The badge text beside a diagram: where it came from, and therefore how much
 * standing it has.
 *
 * The Rust enum is tagged **externally** — the payload-carrying variants cross
 * as a single-key object and the others as a bare string, with no discriminant
 * field to switch on (`types.ts` pins the exact JSON). So the narrowing here
 * is `typeof === "string"` first and then a key test, and never a look at
 * `.kind` or `.type`, which do not exist.
 *
 * The three origins are kept apart because they fail differently, and
 * collapsing them into one word would lose exactly the distinction a reader
 * needs: derived is reproducible and can only be wrong if the rules are;
 * inferred came from a language model and may be confidently wrong, so the
 * agent is *named*; user-authored is authoritative about intent and says
 * nothing about what the code does.
 *
 * `edited` is appended rather than substituted. Both halves are true of an
 * edited file and both matter — the arrows still came from that agent, and a
 * person has since changed them — so a badge reading only "edited" would be
 * dropping the more important half.
 *
 * A shape this function does not recognise reads "Origin unknown" rather than
 * being guessed at or silently dropped. The badge always says something,
 * because a diagram with no origin stated reads as one whose origin was
 * checked and found unremarkable.
 */
export function derivationLabel(
  derivation: DerivationLike,
  edited = false,
): string {
  const base = originText(derivation);
  if (base === null) return "Origin unknown";
  return edited ? `${base}, edited` : base;
}

function originText(derivation: DerivationLike): string | null {
  if (derivation === "derived") return "Derived";
  if (derivation === "user") return "User-authored";
  if (typeof derivation !== "object" || derivation === null) return null;

  if ("derived" in derivation) return "Derived";
  if ("inferred" in derivation) {
    const agent = (derivation as { inferred?: { agent?: unknown } }).inferred?.agent;
    // An agent name is the whole point of this variant, but an empty one is not
    // worth rendering as "Inferred by " with nothing after it — and it is still
    // an inference, which is the part the reader must not lose.
    return typeof agent === "string" && agent.trim() !== ""
      ? `Inferred by ${agent.trim()}`
      : "Inferred by an agent";
  }
  return null;
}

/**
 * How the warning count reads beside a diagram, or `null` when there is
 * nothing to say.
 *
 * `ArchGraph.warnings` is load-bearing and currently invisible: the deriver
 * records everything it found and refused to draw — an unresolvable reference,
 * a workspace membership it would not infer, a relation no edge kind can
 * express — and today those reach a person only as `%%` comments inside the
 * mermaid source, which mermaid does not render. A view that shows the picture
 * without them is showing a diagram that looks complete and is not, which is
 * the failure this whole feature exists to avoid. Surfacing them is a
 * requirement, not a nicety.
 *
 * The empty case produces **nothing at all** rather than "0 warnings". A
 * banner with no news trains people to stop reading banners, and this one has
 * to be read on the day it is not empty.
 *
 * Blank entries are not counted. A warning nobody can read is not a warning,
 * and counting it would promise the reader a line the panel below cannot show.
 * The count is the whole message: the wording deliberately says "could not
 * draw" rather than "errors", because none of these is the tool going wrong.
 */
export function warningSummary(
  warnings: readonly string[] | null | undefined,
): string | null {
  if (!warnings) return null;
  const count = warnings.filter((warning) => warning.trim() !== "").length;
  if (count === 0) return null;
  const noun = count === 1 ? "thing" : "things";
  return `${count} ${noun} this diagram could not draw`;
}

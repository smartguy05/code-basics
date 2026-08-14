/**
 * Where clicking a box in a diagram takes you — or, more often than any other
 * answer here, nowhere.
 *
 * Two completely different problems wearing one interface, and the difference
 * is the whole point of this file.
 *
 * For a **derived** diagram the answer is exact. `cb-core` minted every node id
 * itself (`architecture::graph`), a project node's id *is* the scan's
 * `Project.id`, and the node carries the path the scan recorded. So
 * {@link targetFor} is a lookup, not a match: it either finds the node and
 * reads its path, or it says no. Getting from a box on screen back to that
 * node is {@link targetsByDomId}'s job, because the id in the DOM is the
 * renderer's, not ours.
 *
 * For an **agent-authored** diagram there are no ids we minted. The file was
 * written by a language model, its node ids are whatever that model typed, and
 * the only way to connect one to a file is to match text against the symbol
 * index. {@link targetsForAuthored} does that, and it does it **exactly and
 * uniquely or not at all** — an ambiguous name yields no target rather than
 * the first of several.
 *
 * **{@link targetsForAuthored} is not wired to anything yet**, and this comment
 * says so rather than describing an app that does not exist. `ArchitectureView`
 * loads a saved diagram with `graph: null` and hands the canvas no symbol
 * index, so **no box in a saved diagram is clickable today**. The function and
 * its tests are the finished half; connecting them needs the view to fetch the
 * index and pass it down, which is a change to a file this did not touch.
 *
 * Both sides obey the same rule the rest of this project obeys: a box that
 * cannot be resolved is **not clickable**. The tempting alternative — guessing
 * from a label, or opening the workspace root, or offering a search — is worse
 * than doing nothing, because a click that opens the wrong file teaches the
 * user that clicks open the wrong file, and they stop trusting the ones that
 * are right. Nothing here has a fallback branch by design.
 *
 * No React and no DOM: the vitest suite runs in a node environment with no
 * jsdom, and the mermaid source is parsed as text.
 */

/** Where a click goes: a workspace-relative path, and a line if one is known. */
export interface Target {
  /** Relative to the workspace root, forward slashes, exactly as it arrived. */
  path: string;
  /** 1-based, matching an editor's gutter. Absent when nothing named a line. */
  line?: number;
}

/**
 * The parts of a node {@link targetFor} reads.
 *
 * Structural rather than an import of `ArchNode`, so this module depends on
 * nothing and the tests can load it without the IPC layer. A real `ArchNode`
 * satisfies it, which is what the tests pass.
 */
export interface GraphLike {
  nodes: readonly { id: string; path: string | null }[];
}

/**
 * Where the node with this id lives, or `null` if it does not live anywhere.
 *
 * `null` is the ordinary answer for a whole class of node and not an error
 * path. A solution or solution folder exists only inside a `.sln` file; an
 * external is something referenced from outside the workspace the scan never
 * saw; a data store is a database, cache or broker with no source in this
 * repository at all. All three carry `path: null` on the wire, and all three
 * must render as boxes you cannot click. Inventing a destination for one of
 * them — the solution's own file, the referencing project, a search for the
 * name — is exactly the wrong-answer failure this project has spent five
 * phases avoiding.
 *
 * A service node *is* clickable: `architecture::components` gives it the same
 * `path` a project node would, because it is a project with one more thing
 * declared about it.
 *
 * No line is ever returned. A graph node names a project or a component, not a
 * position inside a file, and a line the caller cannot substantiate would be a
 * cursor placed at random.
 *
 * Duplicate ids are not supposed to exist — `ArchGraph.nodes` is unique by id
 * — but if two nodes ever share one and disagree about the path, this abstains
 * rather than taking the first. Ordering is not evidence.
 */
export function targetFor(nodeId: string, graph: GraphLike): Target | null {
  const paths = new Set<string>();
  for (const node of graph.nodes) {
    if (node.id !== nodeId) continue;
    const path = node.path?.trim() ?? "";
    if (path === "") return null;
    paths.add(path);
  }
  const [only] = [...paths];
  if (paths.size !== 1 || only === undefined) return null;
  return { path: only };
}

// ---------------------------------------------------------------------------
// From a rendered element back to a graph node
// ---------------------------------------------------------------------------

/**
 * The Mermaid identifier the renderer mints for a graph node id.
 *
 * A **forward** mirror of `mermaid_id` in
 * `crates/core/src/architecture/mermaid.rs`, and forward on purpose. The
 * escaping is `_<hex>_` per non-alphanumeric character, which is reversible, so
 * a decoder is writable — and writing one would put a second copy of a Rust
 * rule in TypeScript, where nothing makes the two move together. This project
 * has already paid for that once. Applying the same transformation to ids we
 * already hold and comparing the results needs no second copy of anything: if
 * the Rust rule changes, every comparison stops matching and nothing becomes
 * clickable, which is the failure this feature is built to prefer over a click
 * that opens the wrong file.
 *
 * Iterating with `for…of` walks code points, which is what Rust's `chars()`
 * does, so an astral character escapes to the one value on both sides rather
 * than to a surrogate pair on this one.
 */
export function mermaidIdOf(nodeId: string): string {
  let out = "n";
  for (const ch of nodeId) {
    if (/^[A-Za-z0-9]$/.test(ch)) {
      out += ch;
      continue;
    }
    // `for…of` yields whole code points, so index 0 is the whole character.
    out += `_${(ch.codePointAt(0) ?? 0).toString(16)}_`;
  }
  return out;
}

/**
 * What Mermaid puts in front of a *vertex's* DOM id, before the diagram id is
 * prefixed onto the whole thing (`MERMAID_DOM_ID_PREFIX`, mermaid 11.16.1).
 */
const VERTEX_DOM_PREFIX = "flowchart-";

/**
 * Where each rendered element leads, keyed by the element's DOM `id`.
 *
 * The two ids in play are not the same id, and that is the whole difficulty.
 * `cb-core` mints a graph id (`crates-core`); the renderer escapes it into a
 * Mermaid identifier (`ncrates_2d_core`); Mermaid then decorates *that* into a
 * DOM id, differently for the two things it draws:
 *
 * * a vertex becomes `<diagramId>-flowchart-<identifier>-<counter>`, and
 * * a subgraph becomes `<diagramId>-<identifier>` — no prefix and no counter,
 *   because a subgraph never goes through the vertex table that assigns them.
 *
 * Mermaid does **not** put `data-id` on either: in 11.16.1 the flowchart node
 * shapes set `id` and `data-look`, and `data-id` appears only on edge paths. A
 * selector keyed on `data-id` therefore matches no box at all, which is the bug
 * this function exists to fix.
 *
 * Matching is anchored at both ends rather than done by substring, so
 * `cb-diagram-1` cannot claim `cb-diagram-10`'s boxes and a derived element
 * (`…-background`) cannot be mistaken for the node it was derived from. The
 * counter must be digits; the identifier is looked up in a table built from the
 * graph, so an identifier that was never minted from a node resolves to
 * nothing. Mermaid identifiers contain no `-` — every non-alphanumeric
 * character is escaped — which is what makes the split unambiguous.
 *
 * Subgraphs are clickable, deliberately. A solution and an npm/Cargo workspace
 * both carry the path of the file that declares them, and the box drawn around
 * their members is the only place that file appears in the picture. A solution
 * *folder* carries no path and so still resolves to nothing, by the same rule
 * as every other box: [`targetFor`] decides, and this function only finds which
 * node to ask it about.
 *
 * A blank `diagramId` resolves nothing. There is no anchor to match against,
 * and a caller with no render id has lost track of which drawing is on screen.
 */
export function targetsByDomId(
  diagramId: string,
  domIds: Iterable<string>,
  graph: GraphLike,
): Map<string, Target> {
  const targets = new Map<string, Target>();
  if (diagramId === "") return targets;

  // `mermaid_id` is injective by construction — an escape always contains an
  // underscore and an unescaped run never does — so two distinct node ids
  // cannot land on one key here.
  const byIdentifier = new Map<string, string>();
  for (const node of graph.nodes) byIdentifier.set(mermaidIdOf(node.id), node.id);

  for (const domId of domIds) {
    const identifier = identifierOf(domId, diagramId);
    if (identifier === null) continue;
    const nodeId = byIdentifier.get(identifier);
    if (nodeId === undefined) continue;
    const target = targetFor(nodeId, graph);
    if (target) targets.set(domId, target);
  }
  return targets;
}

/** The Mermaid identifier inside a rendered element's DOM id, if it is one. */
function identifierOf(domId: string, diagramId: string): string | null {
  const prefix = `${diagramId}-`;
  if (!domId.startsWith(prefix)) return null;
  const rest = domId.slice(prefix.length);

  // A subgraph: the identifier and nothing else. An identifier never contains
  // a `-`, so the absence of one is what distinguishes this shape.
  if (!rest.includes("-")) return rest === "" ? null : rest;

  if (!rest.startsWith(VERTEX_DOM_PREFIX)) return null;
  const body = rest.slice(VERTEX_DOM_PREFIX.length);
  const cut = body.lastIndexOf("-");
  if (cut <= 0) return null;
  if (!/^[0-9]+$/.test(body.slice(cut + 1))) return null;
  return body.slice(0, cut);
}

// ---------------------------------------------------------------------------
// Agent-authored diagrams
// ---------------------------------------------------------------------------

/**
 * One thing the symbol index knows about, as far as this module cares.
 *
 * A `SearchHit` from `searchEverywhere` satisfies it unchanged — `label` is
 * the file's name or the symbol's name, `path` is workspace-relative, `line`
 * is 1-based or `null` — which is how the caller supplies the index without
 * this module importing the IPC types.
 */
export interface IndexEntry {
  label: string;
  path: string | null;
  line: number | null;
}

/** One node declaration found in mermaid source. */
export interface DeclaredNode {
  id: string;
  label: string;
}

/**
 * Match an authored diagram's nodes onto files, exactly and uniquely.
 *
 * Returns a map from mermaid node id to destination, holding **only** the
 * nodes that resolved. A node that is absent from the map is not clickable,
 * and that is the expected outcome for most boxes in most hand-written
 * diagrams — an agent draws "Payment flow" and "Third-party gateway", neither
 * of which is a file.
 *
 * The matching rules, in the order they are tried and with the reason each is
 * as strict as it is:
 *
 * * **The label first, then the id.** The label is what the author wrote for a
 *   human to read and is far more likely to be a real name; the id is often an
 *   abbreviation (`A`, `svc1`) that could match anything.
 * * **Exact and case-sensitive.** `cb-core` already refuses casing-only near
 *   misses when it derives edges and reports them instead of matching them
 *   (`architecture::graph`); a case-insensitive match here would be this
 *   module being more confident than the layer that owns the question.
 * * **Unique.** Two index entries under one name means two answers, and there
 *   is nothing in a diagram to choose between them. Two entries pointing at
 *   the same path but different lines are also two answers — one file, two
 *   cursors — and are refused for the same reason.
 * * **No fallback after an ambiguity.** If the label matched several things,
 *   the id is *not* then tried. Having found that a key has two answers,
 *   reaching for another key until one gives a single answer is how you
 *   manufacture a confident wrong result.
 *
 * Entries with no path are skipped: an action hit opens no file, so it can
 * neither be a destination nor make one ambiguous.
 */
export function targetsForAuthored(
  source: string,
  index: readonly IndexEntry[],
): Map<string, Target> {
  const byName = new Map<string, Target[]>();
  for (const entry of index) {
    const path = entry.path?.trim() ?? "";
    if (path === "") continue;
    const target: Target =
      typeof entry.line === "number" && Number.isFinite(entry.line)
        ? { path, line: entry.line }
        : { path };
    const bucket = byName.get(entry.label);
    if (bucket) {
      if (!bucket.some((other) => other.path === target.path && other.line === target.line)) {
        bucket.push(target);
      }
    } else {
      byName.set(entry.label, [target]);
    }
  }

  const targets = new Map<string, Target>();
  for (const node of declaredNodes(source)) {
    for (const key of [node.label, node.id]) {
      const found = byName.get(key);
      if (!found || found.length === 0) continue;
      const [first] = found;
      // Found the key: this is the answer, whether or not it is a single one.
      // Falling through to the next key after an ambiguity is the guess this
      // function refuses to make.
      if (found.length === 1 && first !== undefined) targets.set(node.id, first);
      break;
    }
  }
  return targets;
}

/**
 * The node declarations in some mermaid source, in the order they appear.
 *
 * A declaration is an identifier with a shape opening immediately after it —
 * `A["Api"]`, `A(Api)`, `A[["Sln"]]`, `A[("db")]`, `A{Choice}`. That is the
 * same rule `mermaid.rs`'s validator uses to tell a declaration from a
 * mention, and it is deliberately the same: an identifier with no shape (the
 * `B` in `A --> B`) tells us nothing but an id, and edge-label text (`the
 * `calls` in `A -- calls --> B`) is not a node at all. Both are skipped, so
 * neither can be matched against the index and turned into a clickable box.
 *
 * What is skipped, and why each one would otherwise produce a phantom node:
 * YAML front matter (a `title:` may contain brackets), `%%` comments (the
 * renderer writes every abstained-on warning into one, and those quote real
 * project names), and the inside of any shape (an unquoted label's words are
 * not identifiers). Quote state is tracked while skipping a shape, so a label
 * containing a bracket does not end the shape early.
 *
 * An id declared twice with two different labels is dropped entirely rather
 * than resolved to either — mermaid's own precedence between them is not
 * something this file should be re-deriving, and a box whose label is in
 * dispute is exactly the box not to guess about. Declared twice identically is
 * no conflict at all and is kept.
 *
 * This is a reader for the subset of flowchart syntax that matters here, not a
 * mermaid parser. Anything it does not recognise yields no declaration, which
 * costs a click and never invents one.
 */
export function declaredNodes(source: string): DeclaredNode[] {
  const found: DeclaredNode[] = [];
  const seen = new Map<string, string>();
  const conflicted = new Set<string>();

  for (const line of bodyLines(source)) {
    for (const declaration of declarationsOnLine(line)) {
      const previous = seen.get(declaration.id);
      if (previous === undefined) {
        seen.set(declaration.id, declaration.label);
        found.push(declaration);
      } else if (previous !== declaration.label) {
        conflicted.add(declaration.id);
      }
    }
  }

  return found.filter((declaration) => !conflicted.has(declaration.id));
}

/** The source lines outside the leading YAML front matter block. */
function bodyLines(source: string): string[] {
  const lines = source.split(/\r?\n/);
  if (lines[0]?.trim() !== "---") return lines;
  const close = lines.findIndex((line, index) => index > 0 && line.trim() === "---");
  return close === -1 ? lines : lines.slice(close + 1);
}

function declarationsOnLine(line: string): DeclaredNode[] {
  const chars = Array.from(line);
  const declarations: DeclaredNode[] = [];
  let i = 0;

  while (i < chars.length) {
    const ch = chars[i];
    if (ch === "%" && chars[i + 1] === "%") break; // The rest is a comment.

    if (isIdentifierChar(ch ?? "")) {
      const start = i;
      while (i < chars.length && isIdentifierChar(chars[i] ?? "")) i += 1;
      const id = chars.slice(start, i).join("");
      const opener = chars[i];
      if (opener === "[" || opener === "(" || opener === "{") {
        const end = shapeEnd(chars, i);
        declarations.push({
          id,
          label: labelOf(chars.slice(i, end).join("")),
        });
        i = end;
      }
      continue;
    }
    i += 1;
  }
  return declarations;
}

function isIdentifierChar(ch: string): boolean {
  return ch !== "" && /^[A-Za-z0-9_]$/.test(ch);
}

/**
 * The index just past the shape opening at `open`.
 *
 * Bracket depth, so doubled and nested shapes (`[[…]]`, `([…])`, `[(…)]`) are
 * consumed whole. Brackets inside a quoted label are not counted — a project
 * named `Foo (Legacy)` is ordinary, and counting its parentheses would end the
 * shape in the middle of the name.
 */
function shapeEnd(chars: string[], open: number): number {
  let depth = 0;
  let quoted = false;
  for (let i = open; i < chars.length; i += 1) {
    const ch = chars[i];
    if (ch === '"') {
      quoted = !quoted;
      continue;
    }
    if (quoted) continue;
    if (ch === "[" || ch === "(" || ch === "{") depth += 1;
    else if (ch === "]" || ch === ")" || ch === "}") {
      depth -= 1;
      if (depth === 0) return i + 1;
    }
  }
  return chars.length;
}

/**
 * The label text inside a shape, with the renderer's one escape undone.
 *
 * `escape_label` in `mermaid.rs` escapes exactly one character — the quote,
 * as Mermaid's own `#quot;` — because with `htmlLabels: false` there is no
 * markup in a label for anything else to hide in. Undoing that one is
 * therefore the whole of the decoding, and undoing more (HTML entities, say)
 * would corrupt the names of real projects.
 */
function labelOf(shape: string): string {
  let chars = Array.from(shape);
  // Only strip a bracket pair that actually wraps the whole thing: in the
  // unquoted label `(A) or (B)` the first and last characters pair up by
  // shape and not by nesting, and stripping them would rewrite the name.
  while (
    chars.length >= 2 &&
    isShapePair(chars[0] ?? "", chars[chars.length - 1] ?? "") &&
    shapeEnd(chars, 0) === chars.length
  ) {
    chars = chars.slice(1, -1);
  }
  let text = chars.join("").trim();
  if (text.length >= 2 && text.startsWith('"') && text.endsWith('"')) {
    text = text.slice(1, -1);
  }
  return text.replaceAll("#quot;", '"').trim();
}

function isShapePair(open: string, close: string): boolean {
  return (
    (open === "[" && close === "]") ||
    (open === "(" && close === ")") ||
    (open === "{" && close === "}")
  );
}

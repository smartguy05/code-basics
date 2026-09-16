/**
 * How each node and edge in a derived architecture graph is coloured, and which
 * legend chips a given graph earns — with no React, no DOM and no colour
 * literal in sight.
 *
 * This is the {@link GraphCanvas} counterpart of `mermaid.rs`'s shape/arrow
 * vocabulary and its `Key`. The Rust renderer carried the distinctions in node
 * *shapes* and arrow *glyphs*; a bespoke SVG renderer carries them in *colour*
 * instead. The two rules that module is built on are kept here unchanged:
 *
 * * **Only what is drawn is explained.** {@link legendEntries} lists a category
 *   only when a node of that category is present, in a fixed order, exactly as
 *   `write_legend` lists only the symbols it used.
 * * **No colour literal lives here.** Every colour is a CSS custom-property
 *   *name* (`--accent`, `--pass`, …), resolved against the live theme by the
 *   renderer, so the light and custom themes keep working.
 *
 * # Why a project is coloured by its ecosystem
 *
 * A project map is almost entirely {@link ArchKind}`::Project`, so colouring by
 * kind alone paints nearly every box the one colour — a true picture, but a
 * dull and uninformative one. The one axis that genuinely distinguishes those
 * boxes is the ecosystem the scan found each under (.NET, Node, Rust), which is
 * already on the node, so that is what a project's colour encodes. Every other
 * kind keeps a colour that names its *role* — a service is a service whatever
 * language it is written in, a data store is not code at all — because for those
 * the role is the more important thing to see.
 *
 * `ArchKind` is imported *as a type only*, so this module still has no runtime
 * dependency on the IPC layer and the vitest suite (node environment, no jsdom)
 * loads it unchanged.
 */

import type { ArchKind } from "../../ipc/types";

/** A node category: the word beside it in the legend, and the colour it wears. */
export interface Category {
  /** The legend label. */
  label: string;
  /** A CSS custom-property name, leading `--` included. Never a literal colour. */
  colorVar: string;
}

/** The minimum a node needs for a colour: its kind, and its ecosystem if a project. */
export interface StyleableNode {
  kind: ArchKind;
  ecosystem?: string | null;
}

/**
 * The colour and legend word for each ecosystem a project can be found under.
 *
 * The three values `workspace.rs` produces (`dotnet`, `node`, `cargo`) plus a
 * catch-all for anything a manifest adapter might add later or a project the
 * scan could not classify — which keeps its box visible and honestly labelled
 * rather than uncoloured.
 */
const DOTNET: Category = { label: ".NET", colorVar: "--accent" };
const NODE: Category = { label: "Node", colorVar: "--syntax-type" };
const CARGO: Category = { label: "Rust", colorVar: "--syntax-string" };

const ECOSYSTEMS: Record<string, Category> = {
  dotnet: DOTNET,
  node: NODE,
  cargo: CARGO,
};

/** A project the scan could not place under one of {@link ECOSYSTEMS}. */
const OTHER_PROJECT: Category = { label: "Project", colorVar: "--text-dim" };

/**
 * The colour and legend word for the non-project kinds.
 *
 * A `project` is deliberately absent — it is coloured by ecosystem, not by kind
 * — so this is `Partial` and {@link categoryOf} routes a project away before it
 * reaches here. A `service` stays one colour whatever its ecosystem because the
 * role is what the component map exists to show.
 */
const KIND_CATEGORIES: Partial<Record<ArchKind, Category>> = {
  service: { label: "Service", colorVar: "--pass" },
  webApp: { label: "Web app", colorVar: "--syntax-property" },
  mobileApp: { label: "Mobile app", colorVar: "--skip" },
  dataStore: { label: "Data store", colorVar: "--fail" },
  solution: { label: "Container", colorVar: "--syntax-keyword" },
  solutionFolder: { label: "Container", colorVar: "--syntax-keyword" },
  external: { label: "External", colorVar: "--text-dim" },
};

/** The colour and legend word for a node: ecosystem for a project, kind otherwise. */
export function categoryOf(node: StyleableNode): Category {
  if (node.kind === "project") {
    const ecosystem = node.ecosystem ?? "";
    return ECOSYSTEMS[ecosystem] ?? OTHER_PROJECT;
  }
  return KIND_CATEGORIES[node.kind] ?? OTHER_PROJECT;
}

/**
 * The legend, in the order chips are drawn.
 *
 * Fixed so the legend never reshuffles between two graphs of the same
 * workspace: the ecosystems first (a project map's bulk), then the roles a
 * component map adds, ending with the two things that are not code the reader
 * looks for last. One entry per distinct label, which is what collapses a
 * solution and a folder inside it into one "Container" chip.
 */
const LEGEND_ORDER: readonly Category[] = [
  DOTNET,
  NODE,
  CARGO,
  OTHER_PROJECT,
  { label: "Service", colorVar: "--pass" },
  { label: "Web app", colorVar: "--syntax-property" },
  { label: "Mobile app", colorVar: "--skip" },
  { label: "Container", colorVar: "--syntax-keyword" },
  { label: "Data store", colorVar: "--fail" },
  { label: "External", colorVar: "--text-dim" },
];

/**
 * The legend chips this graph earns: one per category actually present, in
 * {@link LEGEND_ORDER}. An empty graph earns an empty legend.
 */
export function legendEntries(nodes: readonly StyleableNode[]): Category[] {
  const present = new Set(nodes.map((node) => categoryOf(node).label));
  return LEGEND_ORDER.filter((category) => present.has(category.label));
}

/** How an edge of a given kind is stroked. */
export interface EdgeStyle {
  /** A CSS custom-property name, leading `--` included. Never a literal colour. */
  colorVar: string;
  /** A package dependency is dashed, as it was `-.->` in Mermaid. */
  dashed: boolean;
}

/**
 * The stroke for each of the five edge kinds.
 *
 * The colours echo the arrow *meanings* the Mermaid renderer separated by glyph:
 * a compile-time reference is the accent, a package dependency is a dimmer dashed
 * accent, containment is a quiet structural line, a data-store client is green,
 * and a service-to-service HTTP call is amber.
 */
const EDGE_STYLES: Record<string, EdgeStyle> = {
  projectReference: { colorVar: "--accent", dashed: false },
  packageDependency: { colorVar: "--accent-dim", dashed: true },
  contains: { colorVar: "--border-strong", dashed: false },
  dataAccess: { colorVar: "--pass", dashed: false },
  serviceCall: { colorVar: "--skip", dashed: false },
};

/** How to stroke an edge of this kind. A quiet structural line for the unknown. */
export function edgeStyle(kind: string): EdgeStyle {
  return EDGE_STYLES[kind] ?? { colorVar: "--border-strong", dashed: false };
}

/**
 * Placing the nodes of a derived architecture graph as a top-down hierarchy,
 * as arithmetic.
 *
 * No layout library: the codebase's standing preference (see the
 * `panZoomLogic.ts` header) is a handful of tested functions over a dependency
 * to audit and ship, and a layered layout is exactly that. Mermaid did this
 * with dagre internally; a bespoke SVG renderer does it here instead, and does
 * it as pure arithmetic so the vitest suite (node environment, no jsdom) can
 * pin the properties that matter.
 *
 * # The shape of the answer
 *
 * Every node is assigned a **layer** — its depth from a source over directed
 * edges — and drawn in a row at `y = layer * rowGap`. Roots (nothing points at
 * them) sit at layer 0, at the top, and dependencies flow downward, which is
 * the orientation the reference asks for and the opposite of the old
 * `flowchart LR`. Within a row the nodes are ordered to sit near the parents
 * that point at them, so the edges between two rows cross as little as a single
 * cheap pass can manage.
 *
 * # Where this abstains
 *
 * A hand-written or agent-written graph can contain a **cycle**, which has no
 * source and no finite longest path. The layering is therefore bounded: a
 * node's layer never exceeds the node count, so a cycle settles at a stable
 * (if arbitrary) set of layers rather than looping forever. An edge naming a
 * node the graph does not contain contributes nothing — it cannot place a box
 * that is not there — and is left for the renderer to skip. Everything here is
 * deterministic: the same graph lays out identically every time, because a
 * picture that reshuffles between opens is one nobody builds a memory of.
 */

import type { Box } from "./panZoomLogic";

/** Where one node was placed: the centre of its circle, in content space. */
export interface Positioned {
  x: number;
  y: number;
}

/** The result of a layout: where each node goes, and the box they all fit in. */
export interface Layout {
  /** Node id → centre. Holds exactly the nodes that were passed in. */
  positions: Map<string, Positioned>;
  /** The content bounds of the drawing, margin included, for `fit`. */
  bounds: Box;
}

/** Tunables, all optional so a caller can take the defaults. */
export interface LayoutOptions {
  /** Vertical distance between adjacent layers. */
  rowGap?: number;
  /** Horizontal distance between adjacent nodes in a row. */
  colGap?: number;
  /** Slack added around the outermost centres, for the circles and labels. */
  margin?: number;
}

const DEFAULT_ROW_GAP = 130;
const DEFAULT_COL_GAP = 170;
const DEFAULT_MARGIN = 70;

/** An empty box, the answer for a graph with nothing to place. */
const EMPTY_BOX: Box = { x: 0, y: 0, width: 0, height: 0 };

/**
 * Lay a graph out top-down.
 *
 * Structural in both arguments (`{ id }[]`, `{ from, to }[]`) so a real
 * `ArchGraph` satisfies it and this module imports nothing from the IPC layer.
 * Duplicate node ids collapse to one position — a graph is not supposed to have
 * them, and placing one id twice is meaningless — so the last one wins, which
 * is harmless because they are the same box.
 */
export function layout(
  nodes: readonly { id: string }[],
  edges: readonly { from: string; to: string }[],
  options: LayoutOptions = {},
): Layout {
  const rowGap = options.rowGap ?? DEFAULT_ROW_GAP;
  const colGap = options.colGap ?? DEFAULT_COL_GAP;
  const margin = options.margin ?? DEFAULT_MARGIN;

  const ids = [...new Set(nodes.map((node) => node.id))];
  if (ids.length === 0) {
    return { positions: new Map(), bounds: EMPTY_BOX };
  }

  const present = new Set(ids);
  // Only edges whose endpoints both exist can layer or order anything. A
  // self-edge cannot push a node below itself, so it is dropped here too.
  const realEdges = edges.filter(
    (edge) => present.has(edge.from) && present.has(edge.to) && edge.from !== edge.to,
  );

  const layerOf = assignLayers(ids, realEdges);
  const rows = orderRows(ids, layerOf, realEdges);
  const positions = place(rows, rowGap, colGap);

  return { positions, bounds: boundsOf(positions, margin) };
}

/**
 * The layer of each node: its longest path from a source, bounded by the node
 * count so a cycle terminates.
 *
 * Iterative relaxation rather than a topological sort, because the input is not
 * guaranteed acyclic and a relaxation with a cap handles both cases with one
 * piece of code: on a DAG it converges to the true longest path, and on a cycle
 * every node is pinned below the count and the loop stops changing.
 */
function assignLayers(
  ids: readonly string[],
  edges: readonly { from: string; to: string }[],
): Map<string, number> {
  const layerOf = new Map<string, number>(ids.map((id) => [id, 0]));
  const cap = ids.length;

  // At most `cap` passes: each pass can lengthen the longest settled path by at
  // least one, and no path in a graph of `cap` nodes is longer than `cap`.
  for (let pass = 0; pass < cap; pass += 1) {
    let changed = false;
    for (const edge of edges) {
      const next = (layerOf.get(edge.from) ?? 0) + 1;
      if (next > (layerOf.get(edge.to) ?? 0) && next <= cap) {
        layerOf.set(edge.to, next);
        changed = true;
      }
    }
    if (!changed) break;
  }
  return layerOf;
}

/**
 * The order of the nodes within each layer.
 *
 * Every row starts sorted by id, which is what makes the layout deterministic
 * and stable. One top-down barycentre pass then reorders each row so a node
 * sits near the mean position of the parents pointing at it, which is the
 * cheapest thing that measurably reduces edge crossings; a node with no parent
 * in the row above keeps its id-sorted position as the tie-break, so the pass
 * can only improve on the stable order, never scramble it.
 */
function orderRows(
  ids: readonly string[],
  layerOf: Map<string, number>,
  edges: readonly { from: string; to: string }[],
): string[][] {
  const rows: string[][] = [];
  for (const id of [...ids].sort(compareId)) {
    const layer = layerOf.get(id) ?? 0;
    (rows[layer] ??= []).push(id);
  }
  // A cap-bounded layer can leave gaps in the array; treat a missing row as
  // empty rather than as `undefined`.
  for (let layer = 0; layer < rows.length; layer += 1) rows[layer] ??= [];

  for (let layer = 1; layer < rows.length; layer += 1) {
    const above = rows[layer - 1] ?? [];
    const indexAbove = new Map(above.map((id, index) => [id, index]));
    const parents = new Map<string, number[]>();
    for (const edge of edges) {
      if (layerOf.get(edge.to) !== layer) continue;
      const parentIndex = indexAbove.get(edge.from);
      if (parentIndex === undefined) continue;
      const bucket = parents.get(edge.to);
      if (bucket) bucket.push(parentIndex);
      else parents.set(edge.to, [parentIndex]);
    }

    const row = rows[layer] ?? [];
    const idSorted = new Map(row.map((id, index) => [id, index]));
    row.sort((a, b) => {
      const barA = barycentre(parents.get(a), idSorted.get(a) ?? 0);
      const barB = barycentre(parents.get(b), idSorted.get(b) ?? 0);
      return barA !== barB ? barA - barB : compareId(a, b);
    });
  }
  return rows;
}

/** The mean of the parent indices, or the id-sorted fallback when there are none. */
function barycentre(parentIndices: number[] | undefined, fallback: number): number {
  if (!parentIndices || parentIndices.length === 0) return fallback;
  return parentIndices.reduce((sum, index) => sum + index, 0) / parentIndices.length;
}

/**
 * Turn ordered rows into centres.
 *
 * Each row is centred on `x = 0`: a row of `n` nodes spans `(n - 1) * colGap`
 * and starts at `-span / 2`, so the hierarchy is symmetric about the middle
 * rather than growing off to one side.
 */
function place(rows: string[][], rowGap: number, colGap: number): Map<string, Positioned> {
  const positions = new Map<string, Positioned>();
  rows.forEach((row, layer) => {
    const span = (row.length - 1) * colGap;
    row.forEach((id, index) => {
      positions.set(id, { x: index * colGap - span / 2, y: layer * rowGap });
    });
  });
  return positions;
}

/** The box enclosing every centre, grown by `margin` on all sides. */
function boundsOf(positions: Map<string, Positioned>, margin: number): Box {
  if (positions.size === 0) return EMPTY_BOX;
  let minX = Infinity;
  let minY = Infinity;
  let maxX = -Infinity;
  let maxY = -Infinity;
  for (const { x, y } of positions.values()) {
    minX = Math.min(minX, x);
    minY = Math.min(minY, y);
    maxX = Math.max(maxX, x);
    maxY = Math.max(maxY, y);
  }
  return {
    x: minX - margin,
    y: minY - margin,
    width: maxX - minX + margin * 2,
    height: maxY - minY + margin * 2,
  };
}

/** Plain code-unit order, so the layout is identical on every machine. */
function compareId(a: string, b: string): number {
  return a < b ? -1 : a > b ? 1 : 0;
}

import { useMemo, useState } from "react";
import type {
  ElidedReason,
  InspectGraph,
  InspectNode,
} from "../ipc/types";
import { countLabel, objectMatches, targetLabel } from "./treeLogic";

/**
 * The object tree and the header that frames it.
 *
 * `CaptureHeader` lives here rather than in its own file because it exists
 * only to sit directly above a tree: it shares the value vocabulary (a capture
 * is as much "what could not be read" as "what was"), and splitting thirty
 * lines out would buy an import and no reuse.
 *
 * Both components are purely presentational — they receive an already-parsed
 * graph and never call IPC. Anything that needs a fresh capture (expanding
 * past a cap) is raised to the caller through `onExpand`.
 *
 * The rendering rule that governs every branch below is inherited from
 * `git/grouping.rs`: a wrong value is much worse than no value. Nothing here
 * ever synthesises, defaults or rounds a value. `null`, `elided` and
 * `unavailable` are three different statements and each is drawn differently,
 * because a user who reads "could not be read" as "was null" has been
 * actively misled by the tool they opened to find the truth.
 */

const ELIDED_WORDING: Record<ElidedReason, string> = {
  // Worded, never coded: the point of the row is to say why in a sentence the
  // user does not have to decode.
  depthLimit: "stopped here — depth limit reached",
  childLimit: "more items than the child limit allowed",
  nodeLimit: "stopped here — the capture hit its total node limit",
};

/**
 * The object a re-read would be rooted at, and the row that owns it.
 *
 * An `elided` row carries no address of its own — it is a marker where a cap
 * stopped the walk — so re-reading it means re-reading its nearest enclosing
 * *reference*. That capture is that ancestor, which is therefore also the row
 * the result must be spliced into: splicing a copy of the parent under the
 * elided child would render the parent's fields under a label that does not
 * own them, and show them twice.
 *
 * Null when no ancestor was a reference, in which case no expand affordance is
 * offered — an "Expand" button that cannot expand is worse than none.
 */
export interface ExpandTarget {
  /** The node the fresh capture replaces. */
  id: string;
  /** The address the fresh capture is rooted at. */
  address: string;
}

/** Ask for a fresh read of one object. `widen` names the cap that stopped the
 * previous read, so the backend can raise it; without it the re-read returns
 * the identical truncation. */
export type ExpandHandler = (
  target: ExpandTarget,
  widen: ElidedReason | null,
) => void;

interface ValueProps {
  node: InspectNode;
  /** Where an `elided` row inside this node would be re-read from. */
  enclosing: ExpandTarget | null;
  onExpand: ExpandHandler;
  onJumpTo: (nodeId: string) => void;
}

function Value({ node, enclosing, onExpand, onJumpTo }: ValueProps) {
  const value = node.value;

  const expandButton = (target: ExpandTarget, widen: ElidedReason | null) => (
    <button
      type="button"
      className="ov-expand"
      onClick={(event) => {
        event.stopPropagation();
        onExpand(target, widen);
      }}
    >
      Expand
    </button>
  );

  switch (value.kind) {
    case "primitive":
      return <span className="ov ov-primitive">{value.text}</span>;

    case "text":
      return (
        <span className="ov ov-text">
          <span className="ov-quoted">
            &quot;{value.text}
            {value.truncated ? "…" : ""}&quot;
          </span>
          {value.truncated && (
            // Both an ellipsis inside the quotes and a word outside them: a
            // cut-off string that reads as the whole value is a silent lie.
            <span className="ov-truncated" title="The captured string was cut short">
              truncated
            </span>
          )}
        </span>
      );

    case "null":
      return <span className="ov ov-null">null</span>;

    case "pair":
      // A dictionary entry is a pure container: its Key and Value children hold
      // the values, and the row's own value column stays empty rather than
      // inventing a summary of two things.
      return null;

    case "reference":
      return (
        <span className="ov ov-reference">
          <span className="ov-type">{value.typeName}</span>
          <span className="ov-address" title={value.address}>
            {value.address}
          </span>
          {value.expandable &&
            expandButton({ id: node.id, address: value.address }, null)}
        </span>
      );

    case "cycle":
      return (
        <button
          type="button"
          className="ov ov-cycle"
          title={`Already shown at ${value.path}`}
          onClick={(event) => {
            event.stopPropagation();
            onJumpTo(value.path);
          }}
        >
          ↩ already shown at <span className="ov-path">{value.path}</span>
        </button>
      );

    case "elided":
      return (
        <span className="ov ov-elided">
          <span className="ov-reason">{ELIDED_WORDING[value.reason]}</span>
          {enclosing != null && expandButton(enclosing, value.reason)}
        </span>
      );

    case "unavailable":
      return (
        <span className="ov ov-unavailable" title={value.reason}>
          <span className="ov-warn">⚠</span> could not read
          <span className="ov-reason">{value.reason}</span>
        </span>
      );
  }

  return null;
}

interface RowProps {
  node: InspectNode;
  depth: number;
  text: string;
  selectedId: string | null;
  enclosing: ExpandTarget | null;
  collapsed: Set<string>;
  toggle: (id: string) => void;
  onSelect: (node: InspectNode) => void;
  onExpand: ExpandHandler;
  onJumpTo: (nodeId: string) => void;
}

function Row({
  node,
  depth,
  text,
  selectedId,
  enclosing,
  collapsed,
  toggle,
  onSelect,
  onExpand,
  onJumpTo,
}: RowProps) {
  if (!objectMatches(node, text)) return null;

  // A cycle is a leaf by construction: it names a node already on the path, so
  // recursing into it is exactly the infinite walk the variant exists to stop.
  const isCycle = node.value.kind === "cycle";
  const isBranch = !isCycle && node.children.length > 0;
  const isCollapsed = collapsed.has(node.id);
  const count = isCycle ? null : countLabel(node);

  // This node supersedes the inherited target for its own children: an elided
  // row under it belongs to *it*, not to whatever encloses it.
  const childTarget: ExpandTarget | null =
    node.value.kind === "reference"
      ? { id: node.id, address: node.value.address }
      : enclosing;

  return (
    <>
      <div
        className={`test-node object-node ${selectedId === node.id ? "selected" : ""}`}
        style={{ paddingLeft: 6 + depth * 14 }}
        onClick={() => {
          onSelect(node);
          if (isBranch) toggle(node.id);
        }}
        title={node.id}
      >
        <span className="twisty">{isBranch ? (isCollapsed ? "▸" : "▾") : ""}</span>
        <span className="label">{node.label}</span>
        {node.typeName != null && node.value.kind !== "reference" && (
          <span className="ov-type">{node.typeName}</span>
        )}
        <Value
          node={node}
          enclosing={enclosing}
          onExpand={onExpand}
          onJumpTo={onJumpTo}
        />
        {count != null && <span className="badge">{count}</span>}
      </div>

      {isBranch &&
        !isCollapsed &&
        node.children.map((child) => (
          <Row
            key={child.id}
            node={child}
            depth={depth + 1}
            text={text}
            selectedId={selectedId}
            enclosing={childTarget}
            collapsed={collapsed}
            toggle={toggle}
            onSelect={onSelect}
            onExpand={onExpand}
            onJumpTo={onJumpTo}
          />
        ))}
    </>
  );
}

export interface ObjectTreeProps {
  roots: InspectNode[];
  filter: string;
  selectedId: string | null;
  onSelect: (node: InspectNode) => void;
  /** Ask the backend for a fresh capture rooted at this address. */
  onExpand: ExpandHandler;
  /** Scroll to and select the node a cycle points back at. */
  onJumpTo: (nodeId: string) => void;
}

export function ObjectTree({
  roots,
  filter,
  selectedId,
  onSelect,
  onExpand,
  onJumpTo,
}: ObjectTreeProps) {
  const [collapsed, setCollapsed] = useState<Set<string>>(new Set());
  const text = useMemo(() => filter.trim().toLowerCase(), [filter]);

  const toggle = (id: string) =>
    setCollapsed((previous) => {
      const next = new Set(previous);
      if (next.has(id)) next.delete(id);
      else next.add(id);
      return next;
    });

  if (roots.length === 0) {
    return <div className="empty">Nothing captured yet.</div>;
  }

  const visible = roots.filter((node) => objectMatches(node, text));
  if (visible.length === 0) {
    return <div className="empty">No fields match the current filter.</div>;
  }

  return (
    <div className="test-tree object-tree">
      {visible.map((node) => (
        <Row
          key={node.id}
          node={node}
          depth={0}
          text={text}
          selectedId={selectedId}
          enclosing={
            node.value.kind === "reference"
              ? { id: node.id, address: node.value.address }
              : null
          }
          collapsed={collapsed}
          toggle={toggle}
          onSelect={onSelect}
          onExpand={onExpand}
          onJumpTo={onJumpTo}
        />
      ))}
    </div>
  );
}

export interface CaptureHeaderProps {
  graph: InspectGraph;
}

/**
 * What was captured, and what the capture could not promise.
 *
 * Warnings render as a permanent band rather than behind a tooltip or a
 * disclosure: they qualify everything in the tree below, and a caveat the user
 * has to go looking for is a caveat they will not see.
 */
export function CaptureHeader({ graph }: CaptureHeaderProps) {
  const warnings = graph.warnings ?? [];

  return (
    <div className="capture-header">
      <div className="capture-row">
        <span className="capture-target" title={targetLabel(graph)}>
          {targetLabel(graph)}
        </span>
        {graph.target.runtimeVersion != null && (
          <span className="badge">{graph.target.runtimeVersion}</span>
        )}
        {graph.target.bitness != null && (
          <span className="badge">{graph.target.bitness}</span>
        )}
        <span className="muted">{graph.capturedAt}</span>
      </div>

      {warnings.length > 0 && (
        <ul className="capture-warnings">
          {warnings.map((warning, index) => (
            <li key={index}>
              <span className="ov-warn">⚠</span> {warning}
            </li>
          ))}
        </ul>
      )}
    </div>
  );
}

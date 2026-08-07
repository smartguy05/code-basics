import { useMemo, useState } from "react";
import type { TestNode, TestOutcome } from "../ipc/types";
import { formatDuration, testMatches } from "./treeLogic";

interface RowProps {
  node: TestNode;
  depth: number;
  selectedId: string | null;
  onSelect: (node: TestNode) => void;
  text: string;
  outcomes: Set<TestOutcome>;
  /** Branches collapsed by the user. */
  collapsed: Set<string>;
  toggle: (id: string) => void;
}

function Row({
  node,
  depth,
  selectedId,
  onSelect,
  text,
  outcomes,
  collapsed,
  toggle,
}: RowProps) {
  if (!testMatches(node, text, outcomes)) return null;

  const isBranch = node.children.length > 0;
  const isCollapsed = collapsed.has(node.id);

  return (
    <>
      <div
        className={`test-node ${selectedId === node.id ? "selected" : ""}`}
        style={{ paddingLeft: 6 + depth * 14 }}
        onClick={() => {
          onSelect(node);
          if (isBranch) toggle(node.id);
        }}
        title={node.case?.fullName ?? node.label}
      >
        <span className="twisty">{isBranch ? (isCollapsed ? "▸" : "▾") : ""}</span>
        <span className={`dot ${node.outcome}`} />
        <span className="label">{node.label}</span>
        {isBranch && (
          <span className="badge">
            {node.summary.passed}/{node.summary.total}
          </span>
        )}
        <span className="duration">{formatDuration(node.durationMs)}</span>
      </div>

      {isBranch &&
        !isCollapsed &&
        node.children.map((child) => (
          <Row
            key={child.id}
            node={child}
            depth={depth + 1}
            selectedId={selectedId}
            onSelect={onSelect}
            text={text}
            outcomes={outcomes}
            collapsed={collapsed}
            toggle={toggle}
          />
        ))}
    </>
  );
}

export interface TestTreeProps {
  nodes: TestNode[];
  filter: string;
  outcomes: Set<TestOutcome>;
  selectedId: string | null;
  onSelect: (node: TestNode) => void;
}

export function TestTree({
  nodes,
  filter,
  outcomes,
  selectedId,
  onSelect,
}: TestTreeProps) {
  const [collapsed, setCollapsed] = useState<Set<string>>(new Set());
  const text = useMemo(() => filter.trim().toLowerCase(), [filter]);

  const toggle = (id: string) =>
    setCollapsed((previous) => {
      const next = new Set(previous);
      if (next.has(id)) next.delete(id);
      else next.add(id);
      return next;
    });

  if (nodes.length === 0) {
    return <div className="empty">No results yet. Run the tests to see them here.</div>;
  }

  const visible = nodes.filter((node) => testMatches(node, text, outcomes));
  if (visible.length === 0) {
    return <div className="empty">No tests match the current filter.</div>;
  }

  return (
    <div className="test-tree">
      {visible.map((node) => (
        <Row
          key={node.id}
          node={node}
          depth={0}
          selectedId={selectedId}
          onSelect={onSelect}
          text={text}
          outcomes={outcomes}
          collapsed={collapsed}
          toggle={toggle}
        />
      ))}
    </div>
  );
}

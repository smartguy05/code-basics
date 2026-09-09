import { useEffect } from "react";
import { createPortal } from "react-dom";
import { TerminalPanel } from "./TerminalPanel";
import { slotKey, useRegions } from "./RegionContext";
import type { Dockable } from "./regionLayoutLogic";
import type { TerminalDescriptor } from "./terminalLogic";

/**
 * A terminal that can float or dock. It always renders its `TerminalPanel`
 * through a portal — into a docked region's slot when docked, otherwise into the
 * per-codebase floating layer — so moving between the two only changes the
 * portal's container and never remounts the panel, keeping the xterm session and
 * its scrollback alive. Both targets live inside the codebase's `hidden`-able
 * subtree, so a backgrounded codebase's terminals stay hidden exactly as before.
 */
export function DockableTerminal({
  descriptor,
  index,
  stackOffset,
  workspaceActive,
  floatLayer,
  onClose,
  onRaise,
  onAttentionChange,
  onCompleted,
  onRename,
  onRecolor,
}: {
  descriptor: TerminalDescriptor;
  index: number;
  stackOffset: number;
  workspaceActive: boolean;
  /** The codebase's floating-terminal layer (a DOM node), portal target when
   *  not docked. Null for the first frame before it mounts. */
  floatLayer: HTMLElement | null;
  onClose: () => void;
  onRaise: () => void;
  onAttentionChange: (wants: boolean) => void;
  onCompleted: (success: boolean) => void;
  onRename: (title: string) => void;
  onRecolor: (color: string | undefined) => void;
}) {
  const regions = useRegions();
  const dockable: Dockable = { kind: "terminal", id: descriptor.key };
  const key = slotKey(dockable);
  const wantsDock = regions?.dockedTerminals.has(descriptor.key) ?? false;
  const slotNode = wantsDock ? (regions?.slot(key) ?? null) : null;
  // Only *shown* docked once its slot exists, so there is no zero-size flicker
  // in the float layer while the region mounts.
  const showDocked = wantsDock && slotNode !== null;

  // Feed the region tab strip this terminal's label and close action while it is
  // docked (the strip is its header then).
  useEffect(() => {
    if (!regions || !wantsDock) return;
    regions.registerMeta(key, { label: descriptor.title, onClose });
    return () => regions.registerMeta(key, null);
  }, [regions, wantsDock, key, descriptor.title, onClose]);

  const panel = (
    <TerminalPanel
      title={descriptor.title}
      cwd={descriptor.cwd}
      command={descriptor.command}
      number={descriptor.number}
      index={index}
      stackOffset={stackOffset}
      color={descriptor.color}
      workspaceActive={workspaceActive}
      docked={showDocked}
      onDock={regions ? () => regions.dock(dockable, "right") : undefined}
      onClose={onClose}
      onRaise={onRaise}
      onAttentionChange={onAttentionChange}
      onCompleted={onCompleted}
      onRename={onRename}
      onRecolor={onRecolor}
    />
  );

  const target = slotNode ?? floatLayer;
  if (!target) return null;
  return createPortal(panel, target);
}

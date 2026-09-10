import { useEffect, useState, type ReactNode } from "react";
import { createPortal } from "react-dom";

/**
 * Portal `children` into a moving `target` **without ever remounting them**.
 *
 * This exists because of a React fact that is easy to get wrong: changing the
 * container passed to `createPortal` is *not* a re-parent — React deletes the old
 * portal fiber (unmounting the children, running their effect cleanups) and
 * creates a fresh one against the new container. For a docked terminal that meant
 * the mount-once open effect tore down the PTY and spawned a new empty one every
 * time it moved between the floating layer and a region slot; for a docked editor
 * it discarded the CodeMirror buffer and language-server document.
 *
 * The fix is to keep `createPortal`'s container **stable** — a single host `<div>`
 * created once and never swapped — and move *that node* between the real DOM
 * parents (`floatLayer` ↔ region slot) with `appendChild`. An imperative DOM move
 * does not touch React's tree, so the children stay mounted and their sessions,
 * buffers and scroll survive.
 *
 * `target` null keeps the host detached: the children stay mounted (rendered into
 * an off-DOM node) but are simply not visible, which is what preserves an
 * undocked-but-inactive editor rather than destroying it.
 */
export function StablePortal({
  target,
  children,
}: {
  target: HTMLElement | null;
  children: ReactNode;
}) {
  // Created once; its identity is the portal container for this component's whole
  // life, which is precisely what stops React from remounting on a target change.
  const [host] = useState(() => {
    const node = document.createElement("div");
    node.style.height = "100%";
    return node;
  });

  useEffect(() => {
    if (!target) return;
    target.appendChild(host);
    return () => {
      host.remove();
    };
  }, [host, target]);

  return createPortal(children, host);
}

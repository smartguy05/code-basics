import type { FileDiff } from "../ipc/types";

export function formatTime(seconds: number): string {
  return new Date(seconds * 1000).toLocaleString();
}

/** Render a file diff as plain unified text for the read-only commit view. */
export function unifiedText(diff: FileDiff): string {
  const lines: string[] = [];
  for (const hunk of diff.hunks) {
    lines.push(
      `@@ -${hunk.oldStart},${hunk.oldLines} +${hunk.newStart},${hunk.newLines} @@ ${hunk.header}`,
    );
    for (const line of hunk.lines) {
      const marker =
        line.origin === "addition" ? "+" : line.origin === "deletion" ? "-" : " ";
      lines.push(marker + line.content);
    }
  }
  return lines.join("\n");
}

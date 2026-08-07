import type {
  AttachableProcess,
  Attribution,
  InspectGraph,
  InspectNode,
} from "../ipc/types";

/**
 * The order the processes of one configuration are *displayed* in — never a
 * claim about which of them holds the user's objects.
 *
 * A `dotnet run` configuration produces at least two .NET processes: the CLI the
 * supervisor launched (`launched`) and whatever it started underneath
 * (`descendant`). The children are shown first because that is where an
 * application ends up when there is one; the launcher is still listed — it is a
 * real .NET process, and hiding it would be this tool deciding on the user's
 * behalf that they could not have meant it — wearing its caveat.
 */
const ATTRIBUTION_RANK: Record<Attribution, number> = {
  descendant: 0,
  launched: 1,
  unrelated: 2,
};

/**
 * The process to offer by default out of a set attributed to one configuration.
 *
 * Only `isApplication` decides this, and it is decided in the backend. Ranking
 * `descendant` above `launched` here — which is what this used to do — reads
 * "is a child of something we started" as "is the user's application", and the
 * .NET SDK makes that false routinely: a `dotnet run` starts the compiler server
 * and MSBuild worker nodes, all of which publish diagnostics channels and
 * outlive the build, and an ordinary application that is itself the launched pid
 * can start a worker of its own. Preselecting one of those aims the capture
 * button at a build server, charges the user a snapshot for it, and renders the
 * empty result under their configuration's name.
 *
 * A launcher that has been shown to be one is still returned when nothing has
 * evidence behind it, because every place that renders it also renders its
 * caveat — the sentence saying this pid is not the application. What is never
 * returned is a process with no evidence and no caveat, which would be presented
 * as the application while being nothing of the sort.
 *
 * Null means "nothing here is known to be the user's application", and callers
 * offer nothing rather than a guess.
 */
export function preferApplicationProcess(
  processes: AttachableProcess[],
): AttachableProcess | null {
  const ours = processes.filter((process) => process.attribution !== "unrelated");

  return (
    ours.find((process) => process.isApplication) ??
    ours.find(
      (process) =>
        process.attribution === "launched" && process.launcherCaveat != null,
    ) ??
    null
  );
}

/** Configuration, then the application first, then attribution, then pid. */
export function byConfigThenAttribution(
  a: AttachableProcess,
  b: AttachableProcess,
): number {
  const byName = (a.configName ?? "").localeCompare(b.configName ?? "");
  if (byName !== 0) return byName;

  const byEvidence = Number(b.isApplication) - Number(a.isApplication);
  if (byEvidence !== 0) return byEvidence;

  const byRank = ATTRIBUTION_RANK[a.attribution] - ATTRIBUTION_RANK[b.attribution];
  return byRank !== 0 ? byRank : a.pid - b.pid;
}

/** Name, then pid: two processes of the same name are told apart by their path. */
export function byNameThenPid(a: AttachableProcess, b: AttachableProcess): number {
  const byName = a.name.localeCompare(b.name);
  return byName !== 0 ? byName : a.pid - b.pid;
}

/**
 * Whether a capture failed because the target is no longer there.
 *
 * Matched on the message because that is all a failed command returns. A miss
 * costs nothing — the list is refreshed after every live capture anyway — so
 * this only has to be right often enough to be useful, never trusted.
 */
export function readsAsTargetGone(message: string): boolean {
  return /no longer running|not running|exited|no such process|does not exist|could not attach|attach failed/i.test(
    message,
  );
}

export function formatBytes(bytes: number): string {
  if (bytes < 1024) return `${bytes} B`;
  const megabytes = bytes / (1024 * 1024);
  if (megabytes < 1) return `${(bytes / 1024).toFixed(0)} KB`;
  if (megabytes < 1024) return `${megabytes.toFixed(1)} MB`;
  return `${(megabytes / 1024).toFixed(2)} GB`;
}

/** `capturedAt` is unix seconds, straight from `%t` in the dump's name. */
export function formatCaptured(seconds: number): string {
  return new Date(seconds * 1000).toLocaleString();
}

/**
 * Re-point a freshly captured subtree at the id it is being spliced under.
 *
 * A capture rooted at an address numbers its nodes from its own root, so
 * splicing it in unchanged would produce two nodes claiming the same id and a
 * cycle pointing at a path that no longer exists. Only ids inside the fresh
 * subtree are rewritten; a cycle whose path leaves it is left exactly as the
 * inspector wrote it, since guessing where it lands in the old tree would be
 * inventing a link that was never read.
 */
export function rebase(node: InspectNode, from: string, to: string): InspectNode {
  const value =
    node.value.kind === "cycle" && node.value.path.startsWith(from)
      ? { ...node.value, path: to + node.value.path.slice(from.length) }
      : node.value;

  return {
    ...node,
    id: node.id.startsWith(from) ? to + node.id.slice(from.length) : node.id,
    value,
    children: node.children.map((child) => rebase(child, from, to)),
  };
}

/** Replace the children of `targetId` with those of a freshly read node. */
export function spliceInto(
  nodes: InspectNode[],
  targetId: string,
  fresh: InspectNode,
): InspectNode[] {
  return nodes.map((node) => {
    if (node.id === targetId) {
      const rebased = rebase(fresh, fresh.id, targetId);
      return {
        ...node,
        value: rebased.value,
        children: rebased.children,
        hasMore: rebased.hasMore,
        childCountTotal: rebased.childCountTotal,
      };
    }
    return { ...node, children: spliceInto(node.children, targetId, fresh) };
  });
}

/** Escape a node id for use inside a `[title="…"]` attribute selector. */
export function selectorValue(id: string): string {
  return id.replace(/\\/g, "\\\\").replace(/"/g, '\\"');
}

/**
 * Whether two captures could genuinely disagree.
 *
 * A dump is a file: two reads of it are the same bytes, whatever id the
 * inspector stamped on each (it mints a fresh one per invocation). Anything
 * else is a process that went on running between the two reads, and a differing
 * snapshot is exactly the evidence that it did.
 */
export function couldHaveMoved(
  previous: InspectGraph,
  fresh: InspectGraph,
): boolean {
  const before = previous.target.target;
  const after = fresh.target.target;

  if (before.kind !== after.kind) return true;
  if (before.kind === "dump" && after.kind === "dump") {
    return before.path !== after.path;
  }
  return fresh.snapshotId !== previous.snapshotId;
}

/**
 * The one line of the user's own code that makes a caught exception certain.
 *
 * Shown, never written: this belongs in a `catch` block only its author can
 * choose, and a tool that edited someone's source to install a diagnostic would
 * be doing something far larger than it was asked to.
 *
 * The filename deliberately matches what the runtime's own crash handler writes
 * (`<executable>_<pid>_<unix seconds>.dmp`), because that is the shape the
 * dumps list here parses; a dump named anything else is invisible to this tab.
 */
export function setupSnippet(workspaceRoot: string): string {
  // A verbatim literal so a Windows path needs no escaping; only a quote would,
  // and doubling it is what C# expects.
  const dir = `${workspaceRoot.replace(/[\\/]+$/, "")}\\.code-basics\\dumps`.replace(
    /"/g,
    '""',
  );

  return `// dotnet add package Microsoft.Diagnostics.NETCore.Client
using System;
using System.IO;
using Microsoft.Diagnostics.NETCore.Client;

try
{
    // the work that can throw
}
catch (Exception)
{
    var dir = @"${dir}";
    Directory.CreateDirectory(dir);

    // Same name shape as the runtime's crash handler, so the Objects tab lists it.
    var exe = Path.GetFileName(Environment.ProcessPath) ?? "app";
    var stamp = DateTimeOffset.UtcNow.ToUnixTimeSeconds();
    var name = $"{exe}_{Environment.ProcessId}_{stamp}.dmp";

    // WithHeap is dump type 2 — the same type the crash handler writes, and the
    // one the inspector can read objects out of.
    new DiagnosticsClient(Environment.ProcessId)
        .WriteDump(DumpType.WithHeap, Path.Combine(dir, name));

    throw;
}`;
}

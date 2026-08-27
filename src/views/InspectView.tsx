import { useCallback, useEffect, useRef, useState } from "react";
import {
  CaptureHeader,
  ObjectTree,
  type ExpandTarget,
} from "../components/ObjectTree";
import { OutputConsole, type ConsoleHandle } from "../components/OutputConsole";
import * as api from "../ipc/api";
import type {
  AttachableProcess,
  DumpFile,
  ElidedReason,
  InspectGraph,
  InspectStatus,
  InspectTarget,
  RootSpec,
  Workspace,
} from "../ipc/types";
import {
  byConfigThenAttribution,
  byNameThenPid,
  couldHaveMoved,
  formatBytes,
  formatCaptured,
  isAttachWarnSuppressed,
  preferApplicationProcess,
  readsAsTargetGone,
  selectorValue,
  setupSnippet,
  shouldConfirmAttach,
  spliceInto,
  suppressAttachWarn,
} from "./inspectLogic";
import { AttachConfirm } from "../components/AttachConfirm";

/**
 * The Objects tab: pick something to read — a crash dump on disk or a process
 * that is still running — then browse the real objects inside it.
 *
 * Three rules from the core carry all the way up into this view.
 *
 * The first is that nothing is shown that was not read. Where the inspector
 * cannot run, the reason it gave is printed verbatim rather than the tab
 * rendering empty; where dump capture is off, the view says what turning it on
 * would do *and* what a dump contains, because a dump is a copy of the
 * process's entire memory and that is not a detail to bury.
 *
 * The second is that a fresh read is a different moment. Expanding past a cap
 * is not a local operation — it re-runs the inspector — so for a live process
 * the spliced branch need not agree with the tree it lands in, and the view
 * says so in a band the user cannot miss. A dump is exempt, and the exemption
 * is the point: the bytes on disk do not move, the sidecar mints a new
 * `snapshotId` on every invocation regardless, and a band that fired on every
 * expand of a file that cannot change would be a warning carrying no
 * information — trained away long before the live case that needs it arrives.
 *
 * The third only matters now that a live target exists: a dump is frozen and a
 * running process is not. Every live capture is one instant of a process that
 * kept going, so the whole tree is labelled as such rather than the label being
 * attached only to branches re-read later. And because attaching is not free —
 * it clones the target's memory — what it costs is stated before the button is
 * pressed, not discovered afterwards.
 *
 * The fourth belongs to the picker. The runtime's own diagnostics registry now
 * says which .NET processes exist, so the application a `dotnet run`
 * configuration started can be named rather than confused with the CLI that
 * started it. The list is shown in full — every attachable process, grouped by
 * whether code-basics started it — because the alternative is this view
 * quietly deciding which of a user's processes they were allowed to mean.
 */

/** How many instances a type root asks for. Shown, never assumed. */
const DEFAULT_TYPE_LIMIT = 50;

/** Which of the two things this view can read is selected. */
type TargetKind = "dump" | "live";

type RootChoice =
  | "crashException"
  | "exceptions"
  | "type"
  | "statics"
  | "address";

/**
 * Which roots each kind of target can actually answer.
 *
 * Gated rather than merely sorted. `crashException` is the exception that
 * *killed* a process, so on one that is still alive there is nothing for it to
 * return; offering it would be offering a button whose honest result is always
 * empty, which reads as "no exception" rather than "wrong question".
 * `statics` and `address` are offered on both in principle, but a dump reached
 * from the list has no address the user could have learnt yet, so they are kept
 * to the live case where a previous capture supplies one.
 */
const ROOTS_FOR: Record<TargetKind, RootChoice[]> = {
  dump: ["crashException", "exceptions", "type"],
  live: ["exceptions", "type", "statics", "address"],
};

/** What each kind of target opens on: the question it is usually opened to ask. */
const DEFAULT_ROOT: Record<TargetKind, RootChoice> = {
  dump: "crashException",
  live: "exceptions",
};

const ROOT_LABEL: Record<RootChoice, string> = {
  crashException: "Crash exception",
  exceptions: "All exceptions",
  type: "Instances of a type…",
  statics: "Statics of a type…",
  address: "Address…",
};

/**
 * A capture asked for from somewhere else in the app.
 *
 * Declared structurally here rather than imported: `App` owns this state and
 * the name `InspectRequest` is already taken in `ipc/types.ts` by the very
 * different thing the sidecar reads. TypeScript matches these by shape, so an
 * identically shaped interface declared in `App` satisfies the prop.
 */
export interface ContextualInspectRequest {
  target: InspectTarget;
  root: RootSpec;
  /** Shown above the capture so the user knows what they clicked. */
  reason: string;
}

export interface InspectViewProps {
  workspace: Workspace;
  /** A capture asked for from the Run or Tests tab; null when there is none. */
  pendingRequest?: ContextualInspectRequest | null;
  /** Called once the request has been taken, so it does not fire again. */
  onRequestConsumed?: () => void;
}

export function InspectView({
  workspace,
  pendingRequest,
  onRequestConsumed,
}: InspectViewProps) {
  const [status, setStatus] = useState<InspectStatus | null>(null);
  const [graph, setGraph] = useState<InspectGraph | null>(null);
  const [capturing, setCapturing] = useState(false);
  const [error, setError] = useState<string | null>(null);
  // A live capture waiting on the attach confirmation. Held here so the modal's
  // Attach button can run exactly what was requested (button or cross-tab).
  const [pendingAttach, setPendingAttach] = useState<{
    target: InspectTarget;
    root: RootSpec;
    why: string | null;
  } | null>(null);

  const [targetKind, setTargetKind] = useState<TargetKind>("dump");
  const [selectedDump, setSelectedDump] = useState<string | null>(null);
  const [attachable, setAttachable] = useState<AttachableProcess[]>([]);
  const [attachError, setAttachError] = useState<string | null>(null);
  /** What the enumerator could not do, on a list it still produced. */
  const [attachWarnings, setAttachWarnings] = useState<string[]>([]);
  const [selectedPid, setSelectedPid] = useState<number | null>(null);

  const [rootChoice, setRootChoice] = useState<RootChoice>("crashException");
  const [typeName, setTypeName] = useState("");
  const [typeLimit, setTypeLimit] = useState(DEFAULT_TYPE_LIMIT);
  const [address, setAddress] = useState("");

  const [filter, setFilter] = useState("");
  const [selectedId, setSelectedId] = useState<string | null>(null);
  /** Branches re-read in a later snapshot than the tree they now sit in. */
  const [staleBranches, setStaleBranches] = useState<string[]>([]);
  /** Why the capture on screen was taken, when something else asked for it. */
  const [reason, setReason] = useState<string | null>(null);
  const [showSnippet, setShowSnippet] = useState(false);
  const [copied, setCopied] = useState(false);

  const consoleRef = useRef<ConsoleHandle>(null);
  const treeRef = useRef<HTMLDivElement>(null);
  const rootRef = useRef<HTMLDivElement>(null);
  /** The request object already acted on, so it is consumed exactly once. */
  const consumedRef = useRef<ContextualInspectRequest | null>(null);
  /** Read inside the pending-request effect without re-running it. */
  const capturingRef = useRef(false);
  capturingRef.current = capturing;

  /**
   * Re-read what is on disk.
   *
   * Called on mount, whenever the tab becomes visible, after every capture and
   * from the toolbar, because both ends of the list move while this view stays
   * mounted behind a hidden tab: a run that crashes adds a dump, and a capture
   * prunes them. A list read once either hides the dump the user is waiting
   * for, or offers one that has since been deleted.
   */
  const refreshStatus = useCallback(async () => {
    try {
      const next = await api.inspectStatus();
      setStatus(next);
      // Keep the current choice when it survived; otherwise fall back to the
      // newest, and to nothing when there is nothing left.
      setSelectedDump((current) =>
        current != null && next.dumps.some((dump) => dump.path === current)
          ? current
          : (next.dumps[0]?.path ?? null),
      );
    } catch (e) {
      setError(api.errorMessage(e));
    }
  }, []);

  /**
   * Re-read every .NET process on the machine that can be attached to.
   *
   * This view stays mounted, so a process started after it first rendered
   * would otherwise never appear, and one that has since exited would stay on
   * offer as a pid that attaching to now fails.
   *
   * The read costs a sidecar launch — it is the runtime's own diagnostics
   * registry being enumerated, not a cheap in-process lookup — so it is called
   * on the few moments the list is known to be stale and never on a timer.
   *
   * A failure lands beside the picker rather than in the page-level error band:
   * "there are no processes to attach to" and "the list could not be read" are
   * different statements, and the second must not be shown as the first.
   *
   * There is a third, and it is shown too: a list that came back real but
   * incomplete. The enumerator being unable to read parent pids leaves every
   * process unattributed, so the picker looks like a machine running nothing of
   * this workspace's — with the cause known and, until now, thrown away.
   */
  const refreshAttachable = useCallback(async () => {
    try {
      const { processes: next, warnings } = await api.inspectAttachable();
      setAttachable(next);
      setAttachError(null);
      setAttachWarnings(warnings ?? []);
      // Keep a pid the user chose while it is still alive. Otherwise fall back
      // to an application code-basics started, and to nothing at all when the
      // only candidates are processes it did not start — see
      // `preferApplicationProcess`.
      setSelectedPid((current) =>
        current != null && next.some((process) => process.pid === current)
          ? current
          : (preferApplicationProcess(next)?.pid ?? null),
      );
    } catch (e) {
      setAttachError(api.errorMessage(e));
      // The failure is the whole story now; warnings from an earlier, different
      // reading would be shown as if they were about this one.
      setAttachWarnings([]);
    }
  }, []);

  // Both reads are per-workspace: a different repository has different dumps
  // and a different last capture.
  useEffect(() => {
    let cancelled = false;

    void refreshStatus();
    void refreshAttachable();

    api
      .inspectLast()
      .then((previous) => {
        if (!cancelled && previous) setGraph(previous);
      })
      .catch(() => {
        /* nothing captured yet */
      });

    return () => {
      cancelled = true;
    };
  }, [workspace.root, refreshStatus, refreshAttachable]);

  /**
   * Refresh when the tab becomes visible.
   *
   * This view is mounted permanently behind `hidden` — it owns a console and a
   * running sidecar — so it is never told that it was switched to. An observer
   * on its own root is the honest reading of that: a hidden ancestor is
   * `display: none`, which is not intersecting, and unhiding it fires. A
   * process the user started a second ago must be attachable without reopening
   * the workspace.
   */
  useEffect(() => {
    const element = rootRef.current;
    if (element === null) return;

    const observer = new IntersectionObserver((entries) => {
      if (entries.some((entry) => entry.isIntersecting)) {
        void refreshStatus();
        void refreshAttachable();
      }
    });
    observer.observe(element);
    return () => observer.disconnect();
  }, [refreshStatus, refreshAttachable]);

  // A root that the newly chosen kind of target cannot answer would be a
  // question with a guaranteed empty answer, so fall back to its first.
  useEffect(() => {
    setRootChoice((current) =>
      ROOTS_FOR[targetKind].includes(current) ? current : DEFAULT_ROOT[targetKind],
    );
  }, [targetKind]);

  function currentTarget(): InspectTarget | null {
    if (targetKind === "dump") {
      return selectedDump === null ? null : { kind: "dump", path: selectedDump };
    }
    return selectedPid === null ? null : { kind: "live", pid: selectedPid };
  }

  function currentRoot(): RootSpec | null {
    if (!ROOTS_FOR[targetKind].includes(rootChoice)) return null;

    switch (rootChoice) {
      case "exceptions":
        return { kind: "exceptions" };
      case "crashException":
        return { kind: "crashException" };
      case "address": {
        const at = address.trim();
        return at === "" ? null : { kind: "address", address: at };
      }
      case "statics": {
        const name = typeName.trim();
        return name === "" ? null : { kind: "statics", name };
      }
      case "type": {
        const name = typeName.trim();
        return name === "" ? null : { kind: "type", name, limit: typeLimit };
      }
    }
  }

  /**
   * Run one capture.
   *
   * `why` is the sentence to show above the result when something else in the
   * app asked for this; null for a capture the user composed here, where the
   * toolbar already says what was asked.
   *
   * The previous tree is dropped before the new read starts, and the caption is
   * only attached once a graph actually comes back. Keeping the old tree while
   * `reason` already named the new target would render one process's values
   * under another one's caption — a failed attach would leave a dump's objects
   * on screen labelled "exceptions in Api (pid 5000)", which is a value the user
   * never read from anything.
   */
  const runCapture = useCallback(
    async (target: InspectTarget, root: RootSpec, why: string | null) => {
      setCapturing(true);
      capturingRef.current = true;
      setError(null);
      setStaleBranches([]);
      setSelectedId(null);
      setGraph(null);
      setReason(null);
      consoleRef.current?.clear();

      // Whether the process list is worth the sidecar launch it now costs.
      let processesStale = target.kind === "live";

      try {
        const next = await api.inspectCapture(target, root, null, (event) =>
          consoleRef.current?.handle(event),
        );
        setGraph(next);
        setReason(why);
      } catch (e) {
        const message = api.errorMessage(e);
        setError(message);
        // A target that has gone is exactly the moment the list on screen is
        // wrong and the user needs it right, whatever kind of target it was.
        if (readsAsTargetGone(message)) processesStale = true;
      } finally {
        setCapturing(false);
        capturingRef.current = false;
        // A capture prunes, so the dumps list it was chosen from is now out of
        // date either way.
        void refreshStatus();
        // A live target may have exited while being read — but enumerating
        // processes runs the sidecar again, so it is not paid for after a dump
        // capture that says nothing about what is running.
        if (processesStale) void refreshAttachable();
      }
    },
    [refreshStatus, refreshAttachable],
  );

  /**
   * Start a capture, confirming first when it will attach to a running process.
   *
   * A live target pauses and roughly doubles the memory of the process it reads,
   * so the confirmation is the heads-up at the moment the cost is paid — unless
   * the user has ticked "Don't warn me again". A dump is a file already on disk
   * and runs straight through. Both entry points (the Capture button and a
   * cross-tab request) go through here so the gate cannot be bypassed.
   */
  const beginCapture = useCallback(
    (target: InspectTarget, root: RootSpec, why: string | null) => {
      if (shouldConfirmAttach(target, isAttachWarnSuppressed(localStorage))) {
        setPendingAttach({ target, root, why });
        return;
      }
      void runCapture(target, root, why);
    },
    [runCapture],
  );

  /**
   * Take the capture another tab asked for.
   *
   * Consumed by object identity and reported back immediately, so a re-render
   * cannot replay it. A request that arrives mid-capture is refused out loud
   * rather than queued: two sidecars reading the same target at once would
   * produce two answers for one click.
   */
  useEffect(() => {
    if (!pendingRequest || consumedRef.current === pendingRequest) return;
    consumedRef.current = pendingRequest;
    onRequestConsumed?.();

    if (capturingRef.current) {
      setError(
        "A capture was already running, so this one was not started. Try again once it finishes.",
      );
      return;
    }

    // Point the pickers at what is being read, so the toolbar does not describe
    // a different capture from the one on screen.
    const target = pendingRequest.target;
    setTargetKind(target.kind === "live" ? "live" : "dump");
    if (target.kind === "live") setSelectedPid(target.pid);
    else setSelectedDump(target.path);
    const root = pendingRequest.root;
    setRootChoice(root.kind);
    if (root.kind === "type") {
      setTypeName(root.name);
      setTypeLimit(root.limit);
    } else if (root.kind === "statics") {
      setTypeName(root.name);
    } else if (root.kind === "address") {
      setAddress(root.address);
    }

    beginCapture(pendingRequest.target, pendingRequest.root, pendingRequest.reason);
  }, [pendingRequest, onRequestConsumed, beginCapture]);

  function capture() {
    const target = currentTarget();
    const root = currentRoot();
    if (!target || !root || capturing) return;
    beginCapture(target, root, null);
  }

  /**
   * Read one branch again, rooted at its address.
   *
   * There is no separate expand command by design: this is the same capture
   * with a different root, which is why the result can disagree with what is
   * already on screen and why the disagreement is reported rather than merged
   * away.
   *
   * `target.id` is the node that *owns* the address. For an elided row that is
   * its enclosing object, not the row that was clicked: the capture that comes
   * back is that object, so splicing it into the elided row would render one
   * node's fields under another node's label, and show them twice.
   *
   * `widen` names the cap that produced the elision, so the backend can raise
   * it. Re-reading under the limit that truncated the branch returns the same
   * truncation, which is an expand that expanded nothing reported as a read.
   */
  async function expand(target: ExpandTarget, widen: ElidedReason | null) {
    if (!graph || capturing) return;

    setCapturing(true);
    setError(null);

    try {
      const fresh = await api.inspectCapture(
        graph.target.target,
        { kind: "address", address: target.address },
        widen,
        (event) => consoleRef.current?.handle(event),
      );

      const only = fresh.roots.length === 1 ? fresh.roots[0] : undefined;
      if (only === undefined) {
        // Not the one object that was asked for: showing the new capture whole
        // beats guessing which of its roots belonged under the old node.
        setGraph(fresh);
        setStaleBranches([]);
        setSelectedId(null);
        return;
      }

      setGraph((previous) =>
        previous === null
          ? fresh
          : {
              ...previous,
              roots: spliceInto(previous.roots, target.id, only),
              warnings: [...(previous.warnings ?? []), ...(fresh.warnings ?? [])],
            },
      );
      if (couldHaveMoved(graph, fresh)) {
        setStaleBranches((previous) =>
          previous.includes(target.id) ? previous : [...previous, target.id],
        );
      }
    } catch (e) {
      setError(api.errorMessage(e));
    } finally {
      setCapturing(false);
    }
  }

  /** Select a node and bring it into view; rows carry their id as `title`. */
  function jumpTo(nodeId: string) {
    setSelectedId(nodeId);
    const row = treeRef.current?.querySelector(`[title="${selectorValue(nodeId)}"]`);
    row?.scrollIntoView({ block: "center" });
  }

  async function clear() {
    try {
      await api.inspectClear();
      setGraph(null);
      setSelectedId(null);
      setStaleBranches([]);
      setReason(null);
      consoleRef.current?.clear();
    } catch (e) {
      setError(api.errorMessage(e));
    }
  }

  async function copySnippet() {
    try {
      await navigator.clipboard.writeText(setupSnippet(workspace.root));
      setCopied(true);
      window.setTimeout(() => setCopied(false), 2000);
    } catch (e) {
      // The snippet is on screen either way, so this is a failed convenience,
      // not a failed action — say which.
      setError(`Could not copy to the clipboard: ${api.errorMessage(e)}`);
    }
  }

  const dumps: DumpFile[] = status?.dumps ?? [];
  const target = currentTarget();
  const rootReady = currentRoot() !== null;
  const capturedTarget = graph?.target.target;
  const liveGraph = capturedTarget?.kind === "live";

  /**
   * The picked process, and the one a live capture was actually taken from.
   *
   * Both are looked up so the launcher caveat can be stated twice: before the
   * snapshot is paid for, and again over the values it produced. A `dotnet run`
   * pid is the .NET CLI, so an empty tree from it means "the launcher holds
   * none of your types", which is indistinguishable from "your object is not
   * there" unless it is said out loud.
   */
  const pickedProcess =
    attachable.find((process) => process.pid === selectedPid) ?? null;
  const capturedProcess =
    capturedTarget?.kind === "live"
      ? (attachable.find((process) => process.pid === capturedTarget.pid) ?? null)
      : null;

  /**
   * Two groups, never one list.
   *
   * The processes code-basics started are the ones a user came here to read,
   * and they are the only ones it can say anything about beyond a pid. The
   * rest of the machine's .NET processes are attachable and occasionally
   * exactly what someone wants, so they are offered — but underneath a heading
   * that says they are not this workspace's, because a row that looks like the
   * others while carrying none of the same evidence is the kind of thing that
   * gets attached to by accident.
   */
  const ourProcesses = attachable
    .filter((process) => process.attribution !== "unrelated")
    .sort(byConfigThenAttribution);
  const otherProcesses = attachable
    .filter((process) => process.attribution === "unrelated")
    .sort(byNameThenPid);

  /** One row of the picker. Everything known about the process is on it. */
  const processRow = (process: AttachableProcess) => {
    const chosen = process.pid === selectedPid;
    return (
      <label
        key={process.pid}
        style={{
          display: "flex",
          alignItems: "flex-start",
          gap: 8,
          padding: "6px 8px",
          borderTop: "1px solid var(--border)",
          cursor: capturing ? "default" : "pointer",
          background: chosen ? "rgba(90, 120, 220, 0.16)" : "transparent",
          opacity: capturing ? 0.6 : 1,
        }}
      >
        <input
          type="radio"
          name="inspect-process"
          checked={chosen}
          disabled={capturing}
          onChange={() => setSelectedPid(process.pid)}
          style={{ marginTop: 2 }}
        />
        <span style={{ minWidth: 0, flex: 1 }}>
          <span style={{ display: "flex", gap: 6, flexWrap: "wrap" }}>
            {/* The configuration name and the process name differ for a
                `dotnet run` config — "Api" launched, "Api.exe" running — and
                seeing both is what makes the row checkable against what the
                user pressed Run on. */}
            {process.configName != null && (
              <strong style={{ fontSize: 12 }}>{process.configName}</strong>
            )}
            <span className="mono" style={{ fontSize: 12 }}>
              {process.name}
            </span>
            <span className="muted mono" style={{ fontSize: 11 }}>
              pid {process.pid}
            </span>
            {process.attribution === "descendant" && (
              <span className="muted" style={{ fontSize: 11 }}>
                started by this configuration&apos;s launcher
              </span>
            )}
          </span>
          {/* Often the only way to tell two same-named processes apart. */}
          {process.path != null && (
            <span
              className="faint mono"
              style={{
                display: "block",
                fontSize: 10,
                overflow: "hidden",
                textOverflow: "ellipsis",
                whiteSpace: "nowrap",
              }}
              title={process.path}
            >
              {process.path}
            </span>
          )}
          {/* Beside the pid it qualifies, not in a band at the top that the
              eye has already left by the time a row is chosen. */}
          {process.launcherCaveat != null && (
            <span style={{ display: "block", fontSize: 11, marginTop: 2 }}>
              <span className="ov-warn">⚠</span> {process.launcherCaveat}
            </span>
          )}
        </span>
      </label>
    );
  };

  return (
    <div className="main" ref={rootRef}>
      <div className="toolbar">
        <select
          value={targetKind}
          onChange={(e) => setTargetKind(e.target.value as TargetKind)}
          disabled={capturing}
          title="What to read: a crash dump written to disk, or a process that is still running."
        >
          <option value="dump">Crash dump</option>
          <option value="live">Running process</option>
        </select>

        {targetKind === "dump" ? (
          <select
            value={selectedDump ?? ""}
            onChange={(e) => setSelectedDump(e.target.value || null)}
            disabled={capturing || dumps.length === 0}
            style={{ maxWidth: 340 }}
            title={selectedDump ?? "No dumps have been captured in this workspace"}
          >
            {dumps.length === 0 && <option value="">No dumps</option>}
            {dumps.map((dump) => (
              <option key={dump.path} value={dump.path}>
                {dump.executable} · pid {dump.pid} · {formatCaptured(dump.capturedAt)} ·{" "}
                {formatBytes(dump.bytes)}
              </option>
            ))}
          </select>
        ) : (
          /* Not a dropdown: the list below carries a configuration name, a
             process name, a path and sometimes a caveat per row, and a
             collapsed one-line option would have to drop the parts that make a
             row trustworthy. This says only what is currently selected. */
          <span
            className="mono"
            style={{
              fontSize: 11,
              maxWidth: 340,
              overflow: "hidden",
              textOverflow: "ellipsis",
              whiteSpace: "nowrap",
              color: pickedProcess ? "var(--text)" : "var(--text-dim)",
            }}
            title={pickedProcess?.path ?? undefined}
          >
            {pickedProcess
              ? `${pickedProcess.name} · pid ${pickedProcess.pid}`
              : "No process selected"}
          </span>
        )}

        <select
          value={rootChoice}
          onChange={(e) => setRootChoice(e.target.value as RootChoice)}
          disabled={capturing}
          title="Where the walk starts. There is no option to guess something interesting — a heap holds millions of objects."
        >
          {ROOTS_FOR[targetKind].map((choice) => (
            <option key={choice} value={choice}>
              {ROOT_LABEL[choice]}
            </option>
          ))}
        </select>

        {(rootChoice === "type" || rootChoice === "statics") && (
          <input
            placeholder="Namespace.TypeName"
            value={typeName}
            onChange={(e) => setTypeName(e.target.value)}
            disabled={capturing}
            style={{ width: 220 }}
          />
        )}

        {rootChoice === "type" && (
          <input
            type="number"
            min={1}
            value={typeLimit}
            onChange={(e) => setTypeLimit(Number(e.target.value) || 1)}
            disabled={capturing}
            style={{ width: 70 }}
            title="How many instances to read"
          />
        )}

        {rootChoice === "address" && (
          <input
            placeholder="0x00007ffd…"
            value={address}
            onChange={(e) => setAddress(e.target.value)}
            disabled={capturing}
            style={{ width: 200 }}
            title="An object address from an earlier capture. Addresses do not survive a garbage collection, so one from a previous run will not resolve."
          />
        )}

        <button
          className="primary"
          onClick={capture}
          disabled={
            capturing || !target || !rootReady || status?.available === false
          }
          title={
            !target
              ? targetKind === "dump"
                ? "Select a dump first"
                : "Select a running process first"
              : !rootReady
                ? rootChoice === "address"
                  ? "Enter the address to read"
                  : "Enter the type to look for"
                : targetKind === "dump"
                  ? "Read this dump"
                  : "Copy this process's memory and read it"
          }
        >
          Capture
        </button>
        <button onClick={clear} disabled={capturing || graph === null}>
          Clear
        </button>
        <button
          onClick={() => {
            void refreshStatus();
            void refreshAttachable();
          }}
          disabled={capturing}
          title="Re-read the dumps on disk and the processes still running. Both change while this tab is open without telling it."
        >
          Refresh
        </button>

        {capturing && <span className="spinner" />}

        <span className="spacer" style={{ flex: 1 }} />

        <input
          placeholder="Filter fields"
          value={filter}
          onChange={(e) => setFilter(e.target.value)}
          style={{ width: 180 }}
        />
      </div>

      {status !== null && !status.available && (
        <div className="warning inspect-notice">
          <strong>The object inspector is not available here.</strong>
          <div>
            {status.unavailableReason ??
              "The backend did not say why, so nothing more can be reported honestly."}
          </div>
          <div className="muted">
            The inspector ships as a separate executable. Build it with{" "}
            <code>pnpm sidecar:build</code>, which publishes{" "}
            <code>cb-inspector-win-x64.exe</code> into{" "}
            <code>src-tauri/resources/inspector/</code>.
          </div>
        </div>
      )}

      {targetKind === "dump" &&
        status !== null &&
        status.available &&
        !status.dumpCaptureEnabled && (
          <div className="warning inspect-notice">
            <strong>Crash dump capture is off for this workspace.</strong>
            <div>
              Turning it on sets three environment variables on the processes
              this app runs, so the .NET runtime writes a heap dump into{" "}
              <code>.code-basics/dumps/</code> when one of them crashes. It
              fires on an unhandled crash only — a caught exception and a
              process you stop from the toolbar both leave nothing behind.
              Switch the target above to <em>Running process</em> to inspect
              something that has not crashed.
            </div>
            <div>
              <strong>
                A dump is a copy of the process&apos;s entire memory.
              </strong>{" "}
              Connection strings, tokens, request bodies and anything else that
              was live at the moment of the crash are inside it, and a trivial
              console app already produces around 9 MB. Keep them out of
              anything you share.
            </div>
          </div>
        )}

      {targetKind === "live" && (
        <div
          style={{
            margin: "8px 8px 0",
            border: "1px solid var(--border)",
            borderRadius: 4,
            background: "var(--bg-raised)",
            overflow: "hidden",
          }}
        >
          <div
            style={{ maxHeight: 220, overflowY: "auto" }}
            role="radiogroup"
            aria-label="Process to attach to"
          >
            {ourProcesses.length > 0 && (
              <>
                <div className="group-label">Started by code-basics</div>
                {ourProcesses.map(processRow)}
              </>
            )}
            {otherProcesses.length > 0 && (
              <>
                <div className="group-label">
                  Other .NET processes on this machine
                </div>
                {otherProcesses.map(processRow)}
              </>
            )}
            {/* Nothing running is the ordinary state of a machine, so it is
                said plainly. A failure to read the list is a different thing
                and is reported as one, below. */}
            {attachable.length === 0 && attachError === null && (
              <div className="muted" style={{ fontSize: 12, padding: "10px 8px" }}>
                No .NET process on this machine is currently attachable. Start a
                configuration from the Run tab, or press Refresh once something
                is up — the list is read at the moment you ask for it.
              </div>
            )}
            {attachError !== null && (
              <div style={{ fontSize: 12, padding: "10px 8px" }}>
                <span className="ov-warn">⚠</span> The list of .NET processes
                could not be read, so nothing here is a complete answer:{" "}
                {attachError}
              </div>
            )}
            {/* A list that came back, but without something it needed. The
                common one is that no process's parent could be read, which
                silently turns every row into "not this workspace's" — the
                reason has to be readable next to the rows it explains. */}
            {attachWarnings.map((warning) => (
              <div
                key={warning}
                style={{
                  fontSize: 11,
                  padding: "8px",
                  borderTop: "1px solid var(--border)",
                }}
              >
                <span className="ov-warn">⚠</span> {warning}
              </div>
            ))}
          </div>
          {attachable.length > 0 && (
            <div
              className="faint"
              style={{
                fontSize: 11,
                padding: "6px 8px",
                borderTop: "1px solid var(--border)",
              }}
            >
              A point-in-time list, read when this tab opened, became visible or
              was refreshed. Processes started since then appear on Refresh.
            </div>
          )}
        </div>
      )}

      {/* The attach cost (a brief pause and a memory spike, and that the app
          keeps serving) is no longer a permanent banner here: it is a
          confirmation shown by `AttachConfirm` at the instant a live capture is
          about to attach, where the warning is relevant and can be dismissed
          for good. */}

      {targetKind === "live" && rootChoice === "exceptions" && (
        <div className="warning inspect-notice">
          <strong>Finding a caught exception is best-effort.</strong>
          <div>
            This root scans the heap for live <code>System.Exception</code>{" "}
            objects, so it finds one only if nothing has collected it yet — a
            caught exception that went out of scope may already be gone, and
            finding nothing is not evidence that nothing was thrown.
          </div>
          <div>
            <button onClick={() => setShowSnippet((shown) => !shown)}>
              {showSnippet ? "Hide setup snippet" : "Show setup snippet"}
            </button>{" "}
            <span className="muted">
              For a capture that is certain, write a dump from the{" "}
              <code>catch</code> block itself. This is shown only — nothing is
              written into your project.
            </span>
          </div>
          {showSnippet && (
            <>
              {/* Scrolls rather than wraps: a wrapped C# line reads as a
                  different line of code from the one that would be pasted. */}
              <pre
                className="mono"
                style={{ overflowX: "auto", whiteSpace: "pre", margin: "8px 0" }}
              >
                {setupSnippet(workspace.root)}
              </pre>
              <div>
                <button onClick={() => void copySnippet()}>
                  {copied ? "Copied" : "Copy setup snippet"}
                </button>
              </div>
            </>
          )}
        </div>
      )}

      {(status?.caveats ?? []).length > 0 && (
        <div className="warning inspect-notice">
          {(status?.caveats ?? []).map((caveat) => (
            <div key={caveat}>
              <span className="ov-warn">⚠</span> {caveat}
            </div>
          ))}
        </div>
      )}

      {error && <div className="error">{error}</div>}

      {staleBranches.length > 0 && (
        <div className="warning inspect-stale">
          <strong>
            {staleBranches.length === 1
              ? "One branch was re-read at a different moment."
              : `${staleBranches.length} branches were re-read at different moments.`}
          </strong>{" "}
          Expanding runs the inspector again, and the target may have moved
          between the two reads, so these branches need not agree with the rest
          of the tree:
          <ul>
            {staleBranches.map((id) => (
              <li key={id} className="mono">
                {id}
              </li>
            ))}
          </ul>
        </div>
      )}

      <div className="content split">
        <div className="top" ref={treeRef}>
          {graph ? (
            <>
              <CaptureHeader graph={graph} />
              {reason !== null && (
                <div className="capture-header">
                  <div className="capture-row">
                    <span className="muted">Captured because:</span> {reason}
                  </div>
                </div>
              )}
              {/* Only for a live target: the bytes of a dump do not move, and a
                  warning that fires where it is false is one nobody reads. */}
              {capturedProcess?.launcherCaveat != null && (
                <div className="warning inspect-stale">
                  <strong>
                    This was read from pid {capturedProcess.pid}, which is not{" "}
                    {capturedProcess.configName ?? capturedProcess.name} itself.
                  </strong>{" "}
                  {capturedProcess.launcherCaveat}
                </div>
              )}
              {liveGraph && (
                <div className="warning inspect-stale">
                  <strong>This is one moment of a running process.</strong> It
                  kept running while it was read and has gone on running since,
                  so every value below is what was in memory at{" "}
                  {graph.capturedAt} and may already have changed. Capture again
                  to see where it is now.
                </div>
              )}
              <ObjectTree
                roots={graph.roots}
                filter={filter}
                selectedId={selectedId}
                onSelect={(node) => setSelectedId(node.id)}
                onExpand={expand}
                onJumpTo={jumpTo}
              />
            </>
          ) : (
            <div className="empty">
              {targetKind === "live"
                ? attachable.length === 0
                  ? "No .NET process is attachable at the moment. Start a configuration from the Run tab, then come back here."
                  : "Select a process above and capture to see what is in its memory right now."
                : dumps.length === 0
                  ? "No crash dumps in this workspace yet. A dump appears here after a run crashes with capture enabled."
                  : "Select a dump and capture to see what was in memory when it crashed."}
            </div>
          )}
        </div>

        <div className="bottom">
          <OutputConsole ref={consoleRef} />
        </div>
      </div>

      {pendingAttach && (
        <AttachConfirm
          onCancel={() => setPendingAttach(null)}
          onConfirm={(dontWarnAgain) => {
            if (dontWarnAgain) suppressAttachWarn(localStorage);
            const { target, root, why } = pendingAttach;
            setPendingAttach(null);
            void runCapture(target, root, why);
          }}
        />
      )}
    </div>
  );
}

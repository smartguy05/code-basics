// Decision logic for the Run tab's Debug control: whether a configuration can
// be launched under a debug adapter at all, and how one `DebugEvent` moves the
// console session it belongs to. Pure so it can be unit-tested in the node
// environment — the view itself is a rendering shell.

import type { DebugEvent, ProcessEvent, RunConfig } from "../ipc/types";

/**
 * The status dot on one console tab.
 *
 * Lives here rather than in `RunView.tsx` so {@link debugEffects} can name it
 * without a logic module importing a `.tsx` file (which would pull React into
 * the node-environment tests).
 */
export type SessionStatus = "running" | "ok" | "fail" | "stopped";

/**
 * The ecosystems `cb_core::dap::registry::Debuggee::for_ecosystem` answers for.
 * Anything else has no adapter, and there is deliberately no fallback to an
 * ordinary run: a Debug button that quietly does not debug is worse than a
 * disabled one that says why.
 */
const DEBUGGABLE = ["dotnet", "node"];

export type DebugAvailability = { available: true } | { available: false; reason: string };

const no = (reason: string): DebugAvailability => ({ available: false, reason });

/**
 * Whether Debug can be offered for `config`, and why not when it cannot.
 *
 * Mirrors what `start_debug` enforces, so the button is disabled where the
 * command would refuse rather than failing after the click. A compound is the
 * case worth stating: the backend preflights **every** member before launching
 * any adapter, so one unsupported member makes the whole compound
 * undebuggable — reporting that here names the member, which the command's
 * error can only do after a build has already run.
 */
export function debugAvailability(
  config: RunConfig | null,
  configs: Pick<RunConfig, "id" | "name" | "kind" | "ecosystem" | "compound">[],
): DebugAvailability {
  if (!config) return no("Select a configuration to debug");
  if (config.kind !== "app") return no("Only application configurations can be debugged");

  const members = config.compound ?? [];
  if (members.length > 0) {
    const byId = new Map(configs.map((c) => [c.id, c]));
    for (const id of members) {
      const member = byId.get(id);
      if (!member) return no(`Compound member \`${id}\` no longer exists`);
      if (member.kind !== "app") return no(`${member.name} is not an application configuration`);
      if (!DEBUGGABLE.includes(member.ecosystem)) {
        return no(`${member.name} is a ${member.ecosystem} configuration and cannot be debugged`);
      }
    }
    return { available: true };
  }

  // An `ecosystem: "compound"` configuration with no members launches nothing;
  // saying so beats resolving it to "compound has no debug adapter".
  if (config.ecosystem === "compound") return no("This compound launches nothing");
  if (!DEBUGGABLE.includes(config.ecosystem)) {
    return no(`${config.ecosystem} configurations cannot be debugged`);
  }
  return { available: true };
}

/** One thing the view must do in response to a debug event. */
export type DebugEffect =
  | { kind: "process"; event: ProcessEvent }
  | { kind: "status"; status: SessionStatus };

/**
 * How one `DebugEvent` moves a console session.
 *
 * The console speaks `ProcessEvent`, so the six `DebugState` variants are
 * mapped onto it here — and the mapping is where the states could quietly
 * collapse into one another, which is exactly what `DebugState` exists to
 * prevent. So: `notInstalled` keeps everything that was looked for (the failure
 * a user can act on), `failed` keeps its detail, and `notRunning` produces
 * nothing at all rather than a synthetic exit.
 *
 * `workspaceRoot` is the `started` banner's cwd: an adapter's real working
 * directory belongs to the debuggee it launches, not to this event.
 */
export function debugEffects(event: DebugEvent, workspaceRoot: string): DebugEffect[] {
  if (event.type === "output") {
    return [{ kind: "process", event: { type: "output", stream: event.stream, text: event.text } }];
  }
  switch (event.state.kind) {
    case "starting":
      return [
        {
          kind: "process",
          event: {
            type: "started",
            pid: null,
            program: "debug adapter",
            args: [],
            cwd: workspaceRoot,
          },
        },
        { kind: "status", status: "running" },
      ];
    case "running":
    case "paused":
      // The debuggee is alive. A pause is not a failure and not an exit — it is
      // the debugger doing its job — so both read as "up" on the tab.
      return [{ kind: "status", status: "ok" }];
    case "exited": {
      const { code } = event.state;
      return [
        {
          kind: "process",
          // A null code is what a stop, or a replacement launch, produces —
          // the adapter never reported one. Treating that as a failure would
          // paint every deliberate Stop red.
          event: {
            type: "exited",
            code,
            success: code == null || code === 0,
            durationMs: 0,
            cancelled: false,
          },
        },
      ];
    }
    case "notInstalled":
      return [
        {
          kind: "process",
          event: {
            type: "failed",
            message: `${event.state.hint}\nLooked for:\n${event.state.lookedFor.join("\n")}`,
          },
        },
      ];
    case "failed":
      return [{ kind: "process", event: { type: "failed", message: event.state.detail } }];
    case "notRunning":
      return [];
  }
}

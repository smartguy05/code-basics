// Decision logic for the SQL MCP server installer panel.
//
// Pure, so every rule below is testable in vitest's node environment; the panel
// is a rendering shell that asks these questions and shows `PlanPreview`.
//
// Nothing here invents a caveat, a path or a write. The plan — including its
// warnings — is composed by `cb_core::mcp::install::plan` and rendered verbatim;
// this module only decides what can be *asked for*, and how to describe an
// answer that came back.

import type { InstallPlan, InstallScope, ProviderId } from "../ipc/types";

/**
 * The agents that have an MCP configuration.
 *
 * `ProviderId` also carries `user`, which is the pseudo-provider standing for
 * hand-written hooks. It is not an agent and has no server map, so it is
 * excluded here at the type level rather than refused at runtime.
 */
export type McpProvider = Extract<ProviderId, "claudeCode" | "codex">;

/** Whether the panel is about adding the server or taking it away. */
export type McpMode = "install" | "remove";

/** The agents offered, in the order the picker lists them. */
export const MCP_PROVIDERS: McpProvider[] = ["claudeCode", "codex"];

/** What each agent is called on screen. */
export function providerLabel(provider: McpProvider): string {
  return provider === "claudeCode" ? "Claude Code" : "Codex";
}

/** One scope radio: whether it can be chosen, and what it means. */
export interface ScopeOption {
  scope: InstallScope;
  label: string;
  /** Always present — a refused scope must say why, never just grey out. */
  detail: string;
  available: boolean;
}

/**
 * The scopes this agent actually supports.
 *
 * Codex reads MCP servers only from `$CODEX_HOME/config.toml` and has no
 * per-repository equivalent, so its project row is present and **unavailable
 * with the reason** rather than absent. Hiding it would leave someone hunting
 * for a per-repository option that does not exist; the backend refuses the same
 * combination, and this is that refusal shown before it is reached.
 */
export function scopeOptions(provider: McpProvider): ScopeOption[] {
  return [
    {
      scope: "project",
      label: "This repository",
      detail:
        provider === "claudeCode"
          ? "Writes .mcp.json at the repository root, which is committed and shared."
          : "Codex has no per-repository MCP configuration — install at user scope instead.",
      available: provider === "claudeCode",
    },
    {
      scope: "user",
      label: "Every repository",
      detail:
        provider === "claudeCode"
          ? "Writes ~/.claude.json, which applies everywhere you use Claude Code."
          : "Writes $CODEX_HOME/config.toml, which applies everywhere you use Codex.",
      available: true,
    },
  ];
}

/**
 * The scope to preselect: the one the agent supports, never a refused one.
 *
 * Project for Claude Code because that is the narrower grant of the two, and the
 * narrower grant is the right default for a switch that gives an agent read
 * access to a database.
 */
export function defaultScope(provider: McpProvider): InstallScope {
  return provider === "claudeCode" ? "project" : "user";
}

/** Whether `scope` may be chosen for `provider` at all. */
export function scopeAvailable(provider: McpProvider, scope: InstallScope): boolean {
  return scopeOptions(provider).some((o) => o.scope === scope && o.available);
}

/**
 * How the current installation state reads.
 *
 * `null` is *not installed*, and is said plainly. The three states are kept
 * apart deliberately: "installed for this repository" and "installed for every
 * repository" are different grants, and collapsing them into "installed" would
 * hide which one somebody actually made.
 */
export function statusText(provider: McpProvider, status: InstallScope | null): string {
  const name = providerLabel(provider);
  if (status === null) return `Not installed for ${name}.`;
  return status === "project"
    ? `Installed for ${name} in this repository.`
    : `Installed for ${name} in every repository.`;
}

/** The confirm button's text, which must say which direction it goes. */
export function confirmLabel(mode: McpMode): string {
  return mode === "install" ? "Write these files" : "Remove the server";
}

/**
 * What a returned plan means for the panel.
 *
 * The case this exists for: an uninstall plan with **zero writes**. There was
 * nothing of ours in that file, so there is nothing to confirm — and that is a
 * rendered sentence, not a disabled Remove button. A disabled button is
 * indistinguishable from one that is busy, or broken, or waiting on something;
 * "there is no entry to remove" is an answer.
 *
 * A zero-write *install* plan cannot happen (an install always writes the
 * entry), so it is reported as the anomaly it would be rather than silently
 * shown as success.
 */
export type PlanOutcome =
  | { kind: "confirm"; plan: InstallPlan }
  | { kind: "nothing"; message: string };

/**
 * `serverLabel` names which server the "nothing to remove" sentence is about.
 *
 * Optional, and defaulted to the SQL server rather than made required, so the
 * existing call site keeps the exact words it had. This module is otherwise
 * entirely generic — provider labels, scope options, the confirm wording, the
 * written note — which is why the browser MCP server reuses it whole instead of
 * getting a second copy; this one sentence was the only thing in it that named
 * a particular server.
 */
export function planOutcome(
  plan: InstallPlan,
  mode: McpMode,
  serverLabel = "SQL MCP server",
): PlanOutcome {
  if (plan.writes.length > 0) return { kind: "confirm", plan };
  return {
    kind: "nothing",
    message:
      mode === "remove"
        ? `Nothing to remove — this configuration holds no entry for the ${serverLabel}.`
        : "The installer produced no changes, which it should never do. Nothing has been written.",
  };
}

/**
 * What to tell the user after a write actually landed.
 *
 * The panel previously had no success surface at all: `confirm()` cleared the
 * preview and refreshed the status line, and that was it. A preview
 * disappearing looks exactly like a cancel, so a successful install read as
 * "nothing happened" — and the natural response to that is to install again.
 *
 * Two things this says that the status line cannot, because both are about
 * *this* write rather than the current state:
 *
 * - **Which files were touched.** The status line says "installed for Claude
 *   Code in this repository"; it does not say `.mcp.json` was created at the
 *   root, which is the thing the user may want to commit or inspect.
 * - **That the file being right is not the same as the server running.** A
 *   project-scope `.mcp.json` is correct-but-inert until Claude Code prompts
 *   for approval, and any agent already running has read its configuration
 *   long since. Both are the "silent no-op" class of surprise this feature
 *   documents elsewhere, so they are stated here rather than discovered.
 */
export function writtenNote(
  mode: McpMode,
  provider: McpProvider,
  scope: InstallScope,
  paths: string[],
): string {
  const verb = mode === "install" ? "Installed" : "Removed";
  const preposition = mode === "install" ? "into" : "from";
  const written = paths.length ? ` ${preposition} ${paths.join(", ")}` : "";
  const parts = [`${verb} for ${providerLabel(provider)}${written}.`];

  if (mode === "install" && scope === "project") {
    parts.push(
      `${providerLabel(provider)} asks you to approve a project configuration the first time it sees one — until you accept, the server is configured but not loaded.`,
    );
  }
  parts.push(`An agent that is already running will not see this until it restarts.`);
  return parts.join(" ");
}

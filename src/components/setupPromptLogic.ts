// Decision logic for the first-open setup prompt. Pure so it can be unit-tested
// in the node environment (no DOM); the modal component is a rendering shell.

import type { InstallScope, ProviderStatus } from "../ipc/types";

/**
 * Should the first-open prompt offer to set the hooks up? True when either the
 * quality gate is not installed, or a detected agent has no intent capture.
 *
 * - Gate missing (`gateScope === null`) always warrants the prompt — it works in
 *   any repo with the right tooling and does not depend on an agent.
 * - Intent capture only warrants it when an agent is actually detected; there is
 *   nothing to install for an agent that is not present.
 * - Intent capture counts only at **project** scope. It is a team-shared hook
 *   that writes into the repository, so a global user-scope install does not
 *   set it up for a given project — the project still needs its own. Without
 *   this, a developer with the hooks installed globally would never be prompted
 *   in any repository.
 */
export function needsSetup(
  providers: ProviderStatus[],
  gateScope: InstallScope | null,
): boolean {
  const gateInstalled = gateScope !== null;
  const detected = providers.filter((p) => p.detected);
  const intentInstalled = detected.some((p) => p.capture === "project");
  return !gateInstalled || (detected.length > 0 && !intentInstalled);
}

/** localStorage key under which a per-workspace "don't ask again" is remembered. */
export function dismissKey(root: string): string {
  return `setupPromptDismissed:${root}`;
}

/** Has the user chosen "Don't ask again" for this workspace on this machine? */
export function isDismissed(storage: Storage, root: string): boolean {
  try {
    return storage.getItem(dismissKey(root)) === "1";
  } catch {
    return false;
  }
}

/** Remember "Don't ask again" for this workspace. */
export function setDismissed(storage: Storage, root: string): void {
  try {
    storage.setItem(dismissKey(root), "1");
  } catch {
    // A storage that refuses writes (private mode, quota) just means we ask
    // again next open — no worse than before, and not worth surfacing.
  }
}

/** The final show decision: needs setup AND not dismissed for this workspace. */
export function shouldPrompt(
  providers: ProviderStatus[],
  gateScope: InstallScope | null,
  storage: Storage,
  root: string,
): boolean {
  return needsSetup(providers, gateScope) && !isDismissed(storage, root);
}

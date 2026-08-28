// Decision logic for the bulk stop controls (Run tab "Stop All", Tests tab
// "Stop Tests"). Pure so it can be unit-tested in the node environment.

import type { RunConfig, RunKind } from "../ipc/types";

/**
 * The config ids to cancel for a bulk stop of one kind.
 *
 * `runningIds` comes from the supervisor's live map (`running_ids`) and holds
 * plain config ids for app/test runs plus `<id>:build` keys for builds. A build
 * key never equals a config id, so builds are excluded automatically; an id is
 * kept only when it names a configuration of the requested `kind`. This is what
 * lets "Stop All" stop application runs while leaving tests and builds running,
 * and "Stop Tests" stop only test runs.
 */
export function runningConfigIdsOfKind(
  configs: Pick<RunConfig, "id" | "kind">[],
  runningIds: string[],
  kind: RunKind,
): string[] {
  const kindById = new Map(configs.map((c) => [c.id, c.kind]));
  return runningIds.filter((id) => kindById.get(id) === kind);
}

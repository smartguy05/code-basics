// Pure decisions for the Redis panel: how a connection is labelled, how scan
// pages accumulate, how a TTL reads, and how a discovered candidate becomes a
// save payload. Extracted so they are testable in the node environment;
// `RedisPanel.tsx` is a rendering shell.

import type {
  RedisCandidate,
  RedisConnectionView,
  RedisKeyInfo,
  RedisScanPage,
} from "../ipc/types";

/**
 * The label to show for a saved connection.
 *
 * A user-named connection shows exactly what they typed. A discovered one shows
 * its own name (the source already composed a filename-based label), so nothing
 * derives a second identity for it.
 */
export function connectionLabel(view: RedisConnectionView): string {
  return view.name.trim() || fallbackLabel(view);
}

function fallbackLabel(view: RedisConnectionView): string {
  const d = view.secret.kind === "literal" ? view.secret.display : null;
  if (d && d.host) return `${d.host}:${d.port ?? 6379}`;
  return view.id;
}

/** A human TTL, e.g. "no expiry" / "1m 30s" / "500ms". */
export function ttlLabel(ttlMs: number | null): string {
  if (ttlMs === null) return "no expiry";
  if (ttlMs < 1000) return `${ttlMs}ms`;
  const totalSeconds = Math.floor(ttlMs / 1000);
  const h = Math.floor(totalSeconds / 3600);
  const m = Math.floor((totalSeconds % 3600) / 60);
  const s = totalSeconds % 60;
  const parts: string[] = [];
  if (h) parts.push(`${h}h`);
  if (m) parts.push(`${m}m`);
  if (s || parts.length === 0) parts.push(`${s}s`);
  return parts.join(" ");
}

/**
 * Append a scan page's keys to what is already shown, dropping duplicates.
 *
 * SCAN can return the same key on two pages (it guarantees full coverage, not
 * uniqueness), so a naive concat would show a key twice. The first occurrence
 * wins and order is preserved — the order the server returned them.
 */
export function mergeScan(existing: RedisKeyInfo[], page: RedisScanPage): RedisKeyInfo[] {
  const seen = new Set(existing.map((k) => k.key));
  const merged = [...existing];
  for (const key of page.keys) {
    if (!seen.has(key.key)) {
      seen.add(key.key);
      merged.push(key);
    }
  }
  return merged;
}

/** Whether a discovered candidate can be connected to now. */
export function isConnectable(candidate: RedisCandidate): boolean {
  return candidate.state.kind === "ready";
}

/** Why a candidate cannot be used, or `null` when it can. */
export function candidateBlockedReason(candidate: RedisCandidate): string | null {
  return candidate.state.kind === "unresolved" ? candidate.state.reason : null;
}

/**
 * Build the payload that saves a discovered candidate as a profile. The consent
 * flags are omitted on purpose — the backend `upsert` ignores them for a new
 * profile, so a save can never grant exposure or writes.
 */
export function discoveredToSaved(
  candidate: RedisCandidate,
  workspaceRoot: string | null,
  nowMs: number,
): Record<string, unknown> {
  return {
    id: candidate.id,
    name: candidate.name,
    secret: candidate.source,
    workspaceRoot,
    userNamed: false,
    createdAtMs: nowMs,
    lastUsedMs: null,
  };
}

/** A fresh id for a user-typed literal connection. */
export function newLiteralId(): string {
  return `literal:${Date.now()}:${Math.random().toString(36).slice(2, 8)}`;
}

/** Workspaces the user has opened before, so reopening is one click. */
export const RECENTS_KEY = "code-basics.recentWorkspaces";

/** The slice of `Storage` the recents list needs (localStorage in the app). */
export type RecentsStorage = Pick<Storage, "getItem" | "setItem">;

export function loadRecents(storage: RecentsStorage): string[] {
  try {
    const raw = storage.getItem(RECENTS_KEY);
    return raw ? (JSON.parse(raw) as string[]) : [];
  } catch {
    return [];
  }
}

export function rememberRecent(storage: RecentsStorage, path: string) {
  const recents = [path, ...loadRecents(storage).filter((p) => p !== path)].slice(0, 8);
  storage.setItem(RECENTS_KEY, JSON.stringify(recents));
}

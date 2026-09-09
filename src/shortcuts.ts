import { useEffect, useState } from "react";
import { COMMANDS, SHORTCUT_STORAGE_KEY, chordEquals, chordFromEvent, effectiveBinding, eventIsInsideSettings, eventIsTyping, pickCommandTarget, readShortcutOverrides, shortcutHint, type ShortcutOverrides } from "./shortcutLogic";

export const SHORTCUTS_CHANGE_EVENT = "code-basics:shortcuts";
type Handler = () => void | boolean | Promise<void>;
const handlers = new Map<string, Handler[]>();

export function loadShortcutOverrides(): ShortcutOverrides {
  return readShortcutOverrides(localStorage.getItem(SHORTCUT_STORAGE_KEY));
}
export function saveShortcutOverrides(value: ShortcutOverrides): void {
  localStorage.setItem(SHORTCUT_STORAGE_KEY, JSON.stringify(value));
  window.dispatchEvent(new CustomEvent(SHORTCUTS_CHANGE_EVENT));
}
export function registerCommand(id: string, handler: Handler): () => void {
  handlers.set(id, [...(handlers.get(id) ?? []), handler]);
  return () => {
    const remaining = (handlers.get(id) ?? []).filter((candidate) => candidate !== handler);
    if (remaining.length) handlers.set(id, remaining); else handlers.delete(id);
  };
}
export function executeCommand(id: string): boolean {
  const registered = handlers.get(id) ?? [];
  for (let index = registered.length - 1; index >= 0; index -= 1) {
    if (registered[index]!() !== false) return true;
  }
  // Every match, not the first: several codebases can be mounted at once and a
  // background one is merely `hidden`, so `querySelector` would answer with a
  // button the user cannot see. `pickCommandTarget` owns the rule and is tested.
  const found = Array.from(document.querySelectorAll<HTMLElement>(`[data-command="${CSS.escape(id)}"]`));
  const choice = pickCommandTarget(
    found.map((element) => ({
      rendered: element.getClientRects().length > 0,
      disabled: element.hasAttribute("disabled") || element.getAttribute("aria-disabled") === "true",
    })),
  );
  if (choice.kind !== "target") return false;
  found[choice.index]!.click();
  return true;
}
export function dispatchShortcut(event: KeyboardEvent): boolean {
  if (eventIsInsideSettings(event)) return false;
  const pressed = chordFromEvent(event);
  const overrides = loadShortcutOverrides();
  const typing = eventIsTyping(event);
  const command = COMMANDS.find((candidate) => {
    const binding = effectiveBinding(candidate, overrides);
    return binding && chordEquals(binding, pressed) && (!typing || candidate.allowInText);
  });
  if (!command) return false;
  // Attempted, but the ANSWER is "a binding matched", not "it did something".
  // The caller uses this to call `preventDefault`, and the two questions differ
  // exactly when it matters: F5 is bound to Run, and if the Run button happens
  // to be disabled — nothing selected, already running — returning false let
  // the key through to the WebView, which reloaded the whole application and
  // discarded the user's session. A key the user has bound is spoken for
  // whether or not the command could act on it.
  executeCommand(command.id);
  return true;
}

/**
 * The keys currently bound to `commandId`, re-read when the user rebinds.
 *
 * A hook rather than a bare call because a tooltip computed once would go
 * stale the moment someone changed the binding in Settings — which is the
 * exact failure that made hardcoding them wrong in the first place.
 */
export function useShortcutHint(commandId: string): string | null {
  const [hint, setHint] = useState(() => shortcutHint(commandId, loadShortcutOverrides()));
  useEffect(() => {
    const reread = () => setHint(shortcutHint(commandId, loadShortcutOverrides()));
    reread();
    window.addEventListener(SHORTCUTS_CHANGE_EVENT, reread);
    return () => window.removeEventListener(SHORTCUTS_CHANGE_EVENT, reread);
  }, [commandId]);
  return hint;
}

import { COMMANDS, SHORTCUT_STORAGE_KEY, chordEquals, chordFromEvent, effectiveBinding, eventIsInsideSettings, eventIsTyping, readShortcutOverrides, type ShortcutOverrides } from "./shortcutLogic";

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
  const target = document.querySelector<HTMLElement>(`[data-command="${CSS.escape(id)}"]`);
  if (!target || target.hasAttribute("disabled") || target.getAttribute("aria-disabled") === "true") return false;
  target.click(); return true;
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
  return command ? executeCommand(command.id) : false;
}

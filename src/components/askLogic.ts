/**
 * The "Ask the codebase" modal's decisions, with no React and no DOM in sight.
 *
 * The feature itself is small: Ctrl+/ opens a box, the user types a question and
 * picks an agent, and the app opens a real PTY terminal running `claude` or
 * `codex` interactively with that question already asked. Everything in this
 * file is the part of that which can be wrong — which shortcut a keydown is,
 * whether the button may be pressed, *why* it may not, and what the terminal tab
 * is called — extracted so it is testable, because the vitest suite runs in a
 * node environment with no jsdom and nothing here may touch a `KeyboardEvent`,
 * an element or a terminal session.
 *
 * Nothing here spawns anything. The agent list, the PATH lookup that produced
 * it, and the command line are `cb-core`'s (`crates/core/src/review.rs`, and the
 * PTY commands in `src-tauri/src/commands/terminal.rs`); this module only reads
 * what the backend reported and refuses to have a second opinion about it.
 */

import type { ShortcutEvent } from "./searchLogic";

export type { ShortcutEvent };

/**
 * Whether a keydown is the Ask shortcut: Ctrl+/ , or Cmd+/ on a mac keyboard.
 *
 * **Ctrl+/ is already taken, and this recogniser cannot see that.**
 * `FileEditor.tsx` binds `Mod-/` to `toggleComment` explicitly and *ahead of*
 * `defaultKeymap`, with `preventDefault`, because the WebView otherwise swallows
 * the chord before CodeMirror is reached — CLAUDE.md records that as
 * load-bearing, and toggling a comment is the thing a developer does with that
 * chord a hundred times a day. A global listener that opened this modal whenever
 * Ctrl+/ was pressed would take that away, which is exactly the trade
 * `searchLogic`'s table refused for Ctrl+F (the console's find bar) and Ctrl+A
 * (select-all in whatever has focus).
 *
 * So the resolution is not in the binding but in the **caller**: this function is
 * pure and knows nothing about focus, and the window-level listener that calls it
 * must abstain — return without opening anything, letting the event reach the
 * focused surface untouched — whenever focus is in something that already means
 * something by this chord. *Which* surfaces those are is a decision, and it lives
 * in {@link shouldAbstainForFocus} below; only the `document.activeElement`
 * lookup that describes the focused element stays at the call site, because that
 * part is pure DOM and there is no DOM in the node environment these tests run
 * in.
 *
 * The modifier rules are the abstaining kind. Alt held is a different chord and
 * is never this one. Shift held is not this one either — on several layouts
 * Ctrl+Shift+/ is how `?` is typed, and claiming it would eat a keystroke the
 * user meant for a text box. Ctrl *and* Cmd together is nobody's Ask shortcut:
 * it is a stuck modifier or a chord being assembled, and firing on it would be
 * this file guessing. Cmd is accepted alone only because `/` with Cmd is the mac
 * spelling of the same intent; unlike `searchLogic`'s table, which documents that
 * it deliberately does not invent a mac variant, this one is asked for.
 */
export function recogniseAskShortcut(event: ShortcutEvent): boolean {
  if (event.key !== "/") return false;
  if (event.altKey || event.shiftKey) return false;
  // Exactly one of Ctrl / Cmd, never both and never neither.
  return event.ctrlKey !== event.metaKey;
}

/**
 * The ancestor selectors {@link shouldAbstainForFocus} recognises, and the whole
 * reason each is here.
 *
 * * `.cm-editor` — CodeMirror. `FileEditor.tsx` binds `Mod-/` to `toggleComment`
 *   explicitly and *ahead of* `defaultKeymap`, with `preventDefault`, because the
 *   WebView otherwise swallows the chord; CLAUDE.md records that as load-bearing.
 * * `.xterm` — a terminal. Over a PTY, Ctrl+/ **is** Ctrl+_ , which is readline's
 *   `undo` — a real, default binding in bash and zsh, and one this feature breaks
 *   on its own output window: the user is talking to the agent in a
 *   `TerminalPanel` that Ask itself opened. xterm puts focus on a hidden
 *   `.xterm-helper-textarea`, which matched none of the old checks, so the
 *   keystroke was eaten silently — the exact failure this list exists to stop.
 *
 * These are *ancestor* selectors, matched with `closest()` at the call site,
 * because focus inside either surface lands on an inner element (a
 * `.cm-content`, a helper textarea) and never on the surface itself.
 */
export const TEXT_ENTRY_ANCESTORS: readonly string[] = [".cm-editor", ".xterm"];

/**
 * Element tags that are text entry on their own, whatever they sit inside.
 *
 * Deliberately the whole tag, not just `input[type=text]`: this module cannot
 * see an input's type, and the cost of being wrong is asymmetric. Abstaining in
 * a checkbox costs one unused chord; claiming the chord in a text box the user
 * is typing in is `searchLogic`'s stated reason for refusing Ctrl+F and Ctrl+A
 * outright.
 */
export const TEXT_ENTRY_TAGS: readonly string[] = ["input", "textarea"];

/**
 * A focused element, reduced to the three facts this decision needs.
 *
 * A plain descriptor rather than an `Element`, for the reason at the top of the
 * file: vitest runs in the node environment with no DOM, so the caller does the
 * `document.activeElement` lookup and the `closest()` calls and passes the
 * answers in. `null` means nothing is focused.
 */
export interface FocusedSurface {
  /** `Element.tagName`, in whatever case the DOM gave it. */
  tagName: string;
  /** Whether the element, or an ancestor, is `contenteditable`. */
  contentEditable: boolean;
  /**
   * Which of {@link TEXT_ENTRY_ANCESTORS} the element sits inside. Only those
   * selectors are ever reported; anything else here is ignored rather than
   * treated as a reason to abstain, so a caller cannot widen this rule by
   * accident.
   */
  ancestors: readonly string[];
}

/**
 * Whether the Ask shortcut must **stand down** because focus is in a surface
 * that already means something by Ctrl+/ .
 *
 * The rule, stated once: *a global chord never wins against a focused text-entry
 * surface.* `askLogic`'s own note on {@link recogniseAskShortcut} argues that
 * eating a keystroke meant for a text box is unacceptable — Ctrl+Shift+/ is
 * refused there purely because it is how `?` is typed on some layouts — and
 * `searchLogic`'s binding table refuses Ctrl+F and Ctrl+A for the same reason.
 * This is that argument applied to where the caret actually is, and the list it
 * checks ({@link TEXT_ENTRY_ANCESTORS}, {@link TEXT_ENTRY_TAGS},
 * `contenteditable`) is deliberately wider than the CodeMirror-only check that
 * shipped first, which let the chord through xterm's hidden textarea and killed
 * readline's undo inside the terminal Ask had just opened.
 *
 * Abstaining is a **whole** abstain at the call site: no `preventDefault`, no
 * `stopPropagation`, the event continues untouched. Opening the modal *and*
 * letting the key through would be the worst of both.
 *
 * Nothing focused is **not** an abstain. That is the ordinary case — the user
 * looking at a tab with no caret anywhere — and is precisely when the shortcut
 * should open the box, so it answers `false` rather than defensively refusing.
 */
export function shouldAbstainForFocus(focused: FocusedSurface | null): boolean {
  if (focused === null) return false;
  if (focused.contentEditable) return true;
  if (TEXT_ENTRY_TAGS.includes(focused.tagName.toLowerCase())) return true;
  return focused.ancestors.some((selector) => TEXT_ENTRY_ANCESTORS.includes(selector));
}

/**
 * Whether there is a question to ask.
 *
 * Whitespace is not a question: the agent would be started with an empty prompt
 * and would sit at its own input waiting, which looks exactly like the app having
 * failed to send anything. Trimming is the whole rule — no minimum length, since
 * "why?" in a file the user is staring at is a perfectly good question and this
 * module has no way to judge one.
 */
export function canAsk(question: string): boolean {
  return question.trim() !== "";
}

/**
 * The program each agent id is spawned as, mirroring `ReviewAgent::program` in
 * `crates/core/src/review.rs`.
 *
 * The keys are the **kebab-case** `ReviewAgent::id()` spelling that
 * `ReviewAgentInfo.id` carries — `"claude-code"`, not the camelCase
 * `"claudeCode"` that `ProviderId` uses for intent providers. The two unions look
 * alike, mean different things and are not interchangeable; crossing them here
 * would produce a picker that silently matches nothing.
 */
const PROGRAM_BY_AGENT: Readonly<Record<string, string>> = {
  "claude-code": "claude",
  codex: "codex",
};

/**
 * The program an agent id would be spawned as, or **null** for an id this build
 * does not know.
 *
 * Null rather than a guess (the id itself, say): the whole value of naming the
 * program in {@link launchBlockedReason} is that the user can go and check for it
 * on PATH, and a name this module made up would send them looking for something
 * that was never looked for.
 */
export function askProgram(agentId: string): string | null {
  return PROGRAM_BY_AGENT[agentId] ?? null;
}

/**
 * The agent fields this module needs. Structural rather than an import of
 * `ReviewAgentInfo`, for `searchLogic`'s reason: this file depends on nothing, so
 * vitest can load it without the IPC layer or `@tauri-apps/api` behind it. A real
 * `ReviewAgentInfo` satisfies it as it stands, and the test passes one, which is
 * what proves the two shapes still agree.
 */
export interface AskAgent {
  id: string;
  label: string;
}

/**
 * Why the Ask button cannot be pressed, or **null** when it can.
 *
 * A **reason string rather than a boolean**, because a disabled button that does
 * not say why is a dead end: the three ways this can be blocked need three
 * different actions from the user, and collapsing them into one greyed-out
 * control tells them none of it. Kept distinct, in the order they are decided:
 *
 * * **No agents at all** — `agents` is the installed set (`review_agents` only
 *   returns agents whose CLI resolved on PATH), so an empty list means nothing is
 *   installed. Decided *first*, ahead of the unchosen case: with no agents there
 *   is nothing to choose, and "choose an agent" in front of an empty picker reads
 *   as the app blaming the user for its own emptiness.
 * * **Nothing chosen** — a picker with entries and no selection. `null`,
 *   `undefined` and a blank id are all the same fact.
 * * **Chosen but absent from the list** — that agent is not installed. The
 *   message names the program that was looked for (`claude` / `codex`), because
 *   "not installed" without it leaves the user with nowhere to go. An id this
 *   build has no program for ({@link askProgram} abstains) gets a *different*
 *   sentence that claims nothing about PATH — a stale remembered id from an older
 *   build is a bug in this app, not a missing install, and saying "`claudeCode`
 *   was not found on PATH" would be a fact that was never checked.
 */
export function launchBlockedReason(
  agents: readonly AskAgent[],
  chosenId: string | null | undefined,
): string | null {
  if (agents.length === 0) {
    return "No coding agent was found — install Claude Code (`claude`) or Codex (`codex`) and make sure it is on your PATH.";
  }

  const chosen = chosenId?.trim() ?? "";
  if (chosen === "") return "Choose an agent to ask.";

  const found = agents.find((a) => a.id === chosen);
  if (found) return null;

  const program = askProgram(chosen);
  if (program === null) {
    return `The agent "${chosen}" is not one this build knows about — pick one from the list.`;
  }
  const label = LABEL_BY_AGENT[chosen] ?? chosen;
  return `${label} is not installed — \`${program}\` was not found on your PATH.`;
}

/**
 * Human labels for the known ids, used only when the agent is *absent* from the
 * list and so has no `label` of its own to borrow. Mirrors `ReviewAgent::label`.
 */
const LABEL_BY_AGENT: Readonly<Record<string, string>> = {
  "claude-code": "Claude Code",
  codex: "Codex",
};

/**
 * How many characters a terminal tab label may be.
 *
 * A terminal tab sits in a strip beside its siblings, so the budget is small; 40
 * is about a clause of a question, which is enough to tell two asks apart.
 */
export const ASK_TITLE_MAX = 40;

/**
 * A short label for the terminal a question is asked in.
 *
 * Whitespace is collapsed first: a question pasted out of a file arrives with
 * newlines and runs of indentation in it, and either would wreck a single-line
 * tab. What is left is cut to {@link ASK_TITLE_MAX} with an ellipsis *replacing*
 * the last character rather than being added after it, so the label never grows
 * past the cap it was measured against, and a trailing space before the ellipsis
 * is trimmed so the cut does not look like a rendering fault.
 *
 * The cut is by **code point** (`Array.from`), never by slicing the raw string:
 * an emoji is two UTF-16 units and a raw `slice` can halve a surrogate pair,
 * which renders as a replacement glyph — `searchLogic.highlightSpans` avoids the
 * same trap for the same reason.
 *
 * A question that is empty or all whitespace answers `"Ask"` rather than an empty
 * string. This is only ever reached for a question {@link canAsk} accepted, so
 * that is defensive; an unlabelled tab, though, is indistinguishable from a
 * broken one, and a fallback costs nothing.
 */
export function terminalTitle(question: string): string {
  const collapsed = question.replace(/\s+/g, " ").trim();
  if (collapsed === "") return "Ask";

  const chars = Array.from(collapsed);
  if (chars.length <= ASK_TITLE_MAX) return collapsed;

  const cut = chars
    .slice(0, ASK_TITLE_MAX - 1)
    .join("")
    .trimEnd();
  // `collapsed` is trimmed and non-empty, so its first character is not a space
  // and `cut` cannot be trimmed away to nothing.
  return `${cut}…`;
}

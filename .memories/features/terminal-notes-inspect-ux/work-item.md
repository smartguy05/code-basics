# Work item: terminal/notes/inspect UX improvements

Five improvements requested by the user:

1. **Inspect (Objects) attach warning** — replace the always-on live-target banner with a
   confirmation dialog shown right before a live attach; live targets only; "Don't warn me
   again" checkbox persisted per machine (localStorage).
2. **Minimized panel overlap** — Notes pill takes the base bottom-right slot; terminal
   pills shift up one slot (`pillBottom(index) = 16 + (index+1)*48`).
3. **Headless spawning** — set `CREATE_NO_WINDOW` at every Windows spawn site so running
   projects no longer pop OS console windows. ConPTY terminals unaffected.
4. **Custom terminal title** — double-click terminal header to rename; blank rejected;
   in-memory on `TerminalDescriptor`.
5. **Custom pill color** — preset-swatch popover for terminal pills (on descriptor,
   in-memory) and the single Notes pill (persisted in localStorage).

Plan file: C:\Users\AnthonyJames\.claude\plans\right-now-when-i-glimmering-hamster.md

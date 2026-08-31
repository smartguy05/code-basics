# Output filtering, folder-tab signalling, and three bug fixes

Six asks batched into one work item.

## ACs

1. **Severity filter for launched-app output** — a threshold dropdown in the Apps
   panel toolbar (All / Info+ / Warn+ / Errors) that hides lines below it, per tab.
   Classification is text-pattern first, with `stderr` as the fallback for a line
   carrying no level marker.
2. **Folder-tab signalling** — a background workspace tab signals what happened in it:
   | Source | Appearance |
   |---|---|
   | build/rebuild/clean exit != 0 | red outline, pulses until clicked |
   | minimized terminal bell | amber outline, pulses until clicked |
   | build/rebuild/clean exit 0 | green outline, pulses until clicked |
   | minimized terminal exits | green, pulses exactly twice, then stops |
   Higher-priority signals are never masked by lower ones. Active tab never flashes.
3. **Bug** — collapse the Run output panel with a file open, close every file, and the
   panel is hidden with no control to restore it. Closing the last file must un-minimize.
4. **Bug** — the Apps panel scrollbar renders but cannot be dragged.
5. **Successful build closes its output tab** (a failed one keeps it).
6. **Bug** — saving `secrets.json` containing comments fails with
   `secrets are not valid JSON: ...`.

## Decisions taken with the user

- Severity: pattern match **plus** stderr fallback (not stderr alone).
- Filter UX: threshold dropdown that **hides** lines (not highlight, not per-level chips).
- Auto-close applies to the **Run tab's** build output panel, not the Apps panel.
- Terminal completion pulses green **twice then stops**; everything else persists
  until the tab is clicked.
- A terminal exit signals only when it was **minimized** and its tab is **not active**.
- Build success closes the tab and pulses green **only if that tab is not active**.

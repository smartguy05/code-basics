import { useEffect, useState } from "react";
import * as api from "../ipc/api";
import { LSP_RESTART_CAVEAT, lspPollDelay, summariseLspStatus } from "./lspStatusLogic";
import type { LspStatus } from "../ipc/types";

/**
 * A quiet indicator for what the language servers are doing, when there is
 * something to say.
 *
 * A rendering shell: every decision — whether to appear at all, which state is
 * worst, what each row says — is in `lspStatusLogic.ts`, which has tests. What is
 * left here is the polling loop and the markup.
 *
 * The loop follows `SearchEverywhere`'s precedent: a state in flight earns a fast
 * read, a settled one a slow one, and `pollKey` re-arms it when the editor's file
 * set changes (which is what starts a server in the first place — a `didOpen`, not
 * this component). It keeps watching slowly rather than stopping, because a server
 * that was ready and then died would otherwise leave this surface silent — the
 * summary of an all-ready status is nothing at all — until the file set changed.
 * `lspStatusLogic.lspPollDelay` owns both decisions, including when to stop.
 *
 * The key itself is `lspStatusLogic.activeLspPollKey`: the *active* codebase's
 * open files, root included. `lsp_status` answers for the active workspace slot,
 * so switching codebases changes the answer, and a key made of file names alone
 * would not notice two codebases holding the same file open.
 *
 * `watching` is a *second* prop and not something derived from the key, because
 * the two answer different questions — which codebase, versus whether anything
 * there could have started a server. See `lspStatusLogic.lspWatching`.
 */
export function LspStatusIndicator({
  pollKey,
  watching,
}: {
  pollKey: string;
  /** Whether the active codebase has an editor open; drives when polling stops. */
  watching: boolean;
}) {
  const [status, setStatus] = useState<LspStatus | null>(null);
  const [open, setOpen] = useState(false);
  const [restarting, setRestarting] = useState(false);

  const restart = async () => {
    setRestarting(true);
    try {
      // The command returns the fresh session's status; the poll loop then
      // follows it from starting → loading → ready.
      setStatus(await api.lspRestart());
    } catch {
      // A failed restart leaves the old status in place; nothing better to show.
    } finally {
      setRestarting(false);
    }
  };

  useEffect(() => {
    let cancelled = false;
    let timer: ReturnType<typeof setTimeout> | undefined;
    let attempt = 0;

    // Drop the previous key's answer before asking for this one. There is one
    // indicator for the whole app now, so without this a codebase switch shows
    // the *previous* codebase's servers under the new one's name until the
    // first read lands — a wrong answer where the rule everywhere else in this
    // file is to show none.
    setStatus(null);

    const read = () => {
      api
        .lspStatus()
        .then((next) => {
          if (cancelled) return;
          attempt += 1;
          setStatus(next);
          const delay = lspPollDelay(next, attempt, watching);
          if (delay !== null) timer = setTimeout(read, delay);
        })
        .catch(() => {
          // No workspace open, or the command failed. Nothing to say beats
          // saying something wrong, and the next file the user opens re-arms
          // this effect.
          if (!cancelled) setStatus(null);
        });
    };

    read();
    return () => {
      cancelled = true;
      if (timer) clearTimeout(timer);
    };
  }, [pollKey, watching]);

  const summary = summariseLspStatus(status);
  if (!summary) return null;

  return (
    // `lsp-status` is a placement hook, not a look: this indicator is mounted in
    // the app's bottom status bar, where `.dropdown-menu`'s downward `top` would
    // open the panel below the window edge. `styles.css` flips it upward for
    // this class *inside `.statusbar`* only, so mounting it anywhere else keeps
    // the ordinary downward menu.
    <div className="dropdown lsp-status">
      <button
        onClick={() => setOpen((was) => !was)}
        title={summary.title}
        aria-expanded={open}
      >
        {summary.tone === "busy" ? (
          <span className="spinner" />
        ) : (
          <span className={summary.tone === "error" ? "tab-status fail" : "tab-status stopped"}>
            {summary.tone === "error" ? "✕" : "▲"}
          </span>
        )}{" "}
        {summary.label}
      </button>

      {open && (
        <>
          <div className="dropdown-backdrop" onClick={() => setOpen(false)} />
          {/* Anchored to its right edge: this button sits at the end of its row,
              and `.dropdown-menu`'s own `left: 0` would run a wide row of paths
              off the window. Kept inline rather than moved into the `lsp-status`
              rules because it holds wherever the indicator is mounted, while the
              upward flip is true only in the status bar. */}
          <div
            className="dropdown-menu"
            style={{ left: "auto", right: 0, maxWidth: 460 }}
          >
            {summary.lines.map((line) => (
              <div
                key={line.id}
                className="dropdown-item"
                style={{
                  display: "block",
                  cursor: "default",
                  whiteSpace: "normal",
                }}
              >
                <div>{line.headline}</div>
                {/* First, and above `detail`: for a promoted server `detail` is
                    just the program path, and this is the sentence that explains
                    why every count in the file says it may be low. */}
                {line.caveat && <div className="muted">{line.caveat}</div>}
                {/* Shown whenever it exists, including on a state a reader
                    might think needs no explanation: the exit code and the
                    version line are both `detail`. */}
                {line.detail && <div className="muted">{line.detail}</div>}
                {line.hint && <div className="muted">{line.hint}</div>}
                {line.lookedFor.length > 0 && (
                  <div className="faint mono" style={{ fontSize: 11 }}>
                    {/* Where we looked, all of it — "not found" is only
                        actionable if the user can see the paths that were
                        tried, so the list is not trimmed to the first. */}
                    Looked in:
                    {line.lookedFor.map((where) => (
                      <div key={where}>{where}</div>
                    ))}
                  </div>
                )}
              </div>
            ))}

            {/* One action for the whole session: restart recovers a server that
                crashed, and the caveat says what it cannot fix. */}
            {summary.restartable && (
              <div
                className="dropdown-item"
                style={{ display: "block", cursor: "default", whiteSpace: "normal" }}
              >
                <button disabled={restarting} onClick={() => void restart()}>
                  {restarting ? "Restarting…" : "Restart language server"}
                </button>
                <div className="muted" style={{ marginTop: 4 }}>
                  {LSP_RESTART_CAVEAT}
                </div>
              </div>
            )}
          </div>
        </>
      )}
    </div>
  );
}

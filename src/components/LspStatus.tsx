import { useEffect, useState } from "react";
import * as api from "../ipc/api";
import { lspPollDelay, summariseLspStatus } from "./lspStatusLogic";
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
 */
export function LspStatusIndicator({ pollKey }: { pollKey: string }) {
  const [status, setStatus] = useState<LspStatus | null>(null);
  const [open, setOpen] = useState(false);

  useEffect(() => {
    let cancelled = false;
    let timer: ReturnType<typeof setTimeout> | undefined;
    let attempt = 0;

    const read = () => {
      api
        .lspStatus()
        .then((next) => {
          if (cancelled) return;
          attempt += 1;
          setStatus(next);
          const delay = lspPollDelay(next, attempt, pollKey.length > 0);
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
  }, [pollKey]);

  const summary = summariseLspStatus(status);
  if (!summary) return null;

  return (
    <div className="dropdown">
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
          {/* Anchored to its right edge: this button sits near the end of the
              toolbar, and `.dropdown-menu`'s own `left: 0` would run a wide row
              of paths off the window. Inline because `styles.css` is not this
              round's to edit. */}
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
          </div>
        </>
      )}
    </div>
  );
}

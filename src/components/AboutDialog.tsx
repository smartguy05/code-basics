import { getTauriVersion, getVersion } from "@tauri-apps/api/app";
import { useEffect, useState } from "react";
import * as api from "../ipc/api";
import type { AboutInfo } from "../ipc/types";
import {
  UNKNOWN,
  formatBuildDate,
  platformLine,
  treeNotice,
  treeState,
  versionDrift,
  versionLine,
} from "./aboutLogic";

/**
 * Help → About: which build of code-basics is this?
 *
 * It exists to be *pasted into a bug report*, which is the whole design brief:
 * a version alone cannot distinguish two builds of the same release, so the
 * commit — marked when the tree was dirty — the build instant and the host
 * platform are all here too. Anything that could not be established says
 * `unknown` in as many words; nothing is left blank for the reader to fill in
 * with an assumption.
 *
 * A rendering shell with no decisions of its own: every string is composed in
 * `aboutLogic.ts`, where a test pins it.
 *
 * Reuses the shared `modal-backdrop` / `modal` / `modal-body` markup and needs
 * no CSS of its own. Two sources feed it — `@tauri-apps/api/app` for the bundle
 * and Tauri versions (already permitted by `core:default`, so no capability
 * change) and the `about_info` command for what that API cannot answer.
 */
export function AboutDialog({ onClose }: { onClose: () => void }) {
  /** `null` until the read lands: *not read yet* is not the same as *unknown*. */
  const [info, setInfo] = useState<AboutInfo | null>(null);
  const [bundleVersion, setBundleVersion] = useState<string | null>(null);
  const [tauriVersion, setTauriVersion] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    // Read in one go and report a failure rather than rendering a half-filled
    // dialog: a version report with a silently missing row is the thing this
    // dialog exists to prevent.
    void Promise.all([api.aboutInfo(), getVersion(), getTauriVersion()])
      .then(([nextInfo, nextBundle, nextTauri]) => {
        setInfo(nextInfo);
        setBundleVersion(nextBundle);
        setTauriVersion(nextTauri);
      })
      .catch((e) => setError(api.errorMessage(e)));
  }, []);

  // Escape closes, matching `FeaturesPicker`. `MenuBar` itself is mouse-only,
  // but a modal that traps the reader with no keyboard exit is a different
  // problem from a menu bar without arrow keys.
  useEffect(() => {
    const onKey = (event: KeyboardEvent) => {
      if (event.key === "Escape") onClose();
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [onClose]);

  const state = info ? treeState(info.gitSha) : null;
  const notice = state ? treeNotice(state) : null;
  const drift = info && bundleVersion ? versionDrift(bundleVersion, info.appVersion) : null;

  return (
    <div className="modal-backdrop" onClick={onClose}>
      <div
        className="modal"
        role="dialog"
        aria-modal="true"
        aria-label="About code-basics"
        onClick={(e) => e.stopPropagation()}
      >
        <header>
          <strong>About code-basics</strong>
        </header>

        <div className="modal-body">
          {error ? (
            <div className="error" style={{ fontSize: 12 }}>
              {error}
            </div>
          ) : !info || bundleVersion === null || tauriVersion === null ? (
            // Deliberately not a table of "unknown" rows: nothing has been
            // read yet, and showing the abstention word before anyone asked
            // would be a claim of its own.
            <p className="faint" style={{ marginTop: 0, fontSize: 13 }}>
              Reading build information…
            </p>
          ) : (
            <>
              <p style={{ marginTop: 0, fontSize: 15 }}>
                <strong>code-basics {versionLine(bundleVersion, info.gitSha)}</strong>
              </p>

              <dl style={{ display: "grid", gridTemplateColumns: "auto 1fr", gap: "4px 12px", margin: 0, fontSize: 12 }}>
                <dt className="faint">Commit</dt>
                {/* The sha is rendered verbatim, suffix and all. `unknown` is a
                    reading, so it is shown as one rather than hidden. */}
                <dd style={{ margin: 0, fontFamily: "var(--mono, monospace)" }}>{info.gitSha}</dd>

                <dt className="faint">Built</dt>
                <dd style={{ margin: 0 }}>{formatBuildDate(info.buildDate)}</dd>

                <dt className="faint">Platform</dt>
                <dd style={{ margin: 0 }}>{platformLine(info.os, info.arch)}</dd>

                <dt className="faint">Tauri</dt>
                <dd style={{ margin: 0 }}>{tauriVersion || UNKNOWN}</dd>
              </dl>

              {notice && (
                <p className="faint" style={{ fontSize: 11, marginBottom: 0 }}>
                  {notice}
                </p>
              )}

              {drift && (
                <div className="error" style={{ fontSize: 11, marginTop: 6 }}>
                  {drift}
                </div>
              )}
            </>
          )}
        </div>

        <footer>
          <button className="primary" onClick={onClose}>
            Close
          </button>
        </footer>
      </div>
    </div>
  );
}

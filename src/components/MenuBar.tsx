import { getCurrentWindow } from "@tauri-apps/api/window";
import { useCallback, useEffect, useState } from "react";
import * as api from "../ipc/api";
import type { EnhancementInfo, PromptInfo, PromptRuns } from "../ipc/types";
import {
  actionTitle,
  confirmAddMessage,
  confirmRerunMessage,
  emptyMessage,
  emptyPromptsMessage,
  promptClickAction,
  runBadge,
  statusBadge,
} from "./enhancementsLogic";

/**
 * The application menu bar: **File** and **Enhancements**.
 *
 * A small HTML menu bar rather than a native OS menu, to stay consistent with
 * the app's custom titlebar and to render the dynamic, file-driven Enhancements
 * lists directly. `File` holds the workspace actions (which also remain as
 * titlebar buttons); `Enhancements` holds two fly-out submenus:
 *
 * - **Add Instructions** — add/remove a marker-bounded section in `CLAUDE.md` /
 *   `AGENTS.md` (all logic in `cb_core::enhancements`).
 * - **Run Agent** — run a prompt's body as an agent in the panel; a run-once
 *   prompt records its success per repo and confirms before re-running.
 *
 * The menu bar sits above the dropdown backdrop (see `styles.css`) so hovering
 * one top-level menu while another is open switches between them.
 */
type TopMenu = "file" | "enhancements";
type Submenu = "instructions" | "prompts";

export function MenuBar({
  onOpen,
  onRescan,
  onRunAgent,
  onOpenReview,
}: {
  onOpen: () => void;
  onRescan: () => void;
  /** Open the agent panel to run a prompt (by id) as an agent. */
  onRunAgent: (promptId: string) => void;
  /** Open the agent panel as an adversarial review (mirrors the Changes tab). */
  onOpenReview: () => void;
}) {
  const [menu, setMenu] = useState<TopMenu | null>(null);
  const [sub, setSub] = useState<Submenu | null>(null);
  const [instructions, setInstructions] = useState<EnhancementInfo[]>([]);
  const [prompts, setPrompts] = useState<PromptInfo[]>([]);
  const [runs, setRuns] = useState<PromptRuns>({});
  const [busy, setBusy] = useState<string | null>(null);
  /** Instruction awaiting the user's confirmation before it is written. */
  const [confirmId, setConfirmId] = useState<string | null>(null);
  /** Run-once prompt awaiting confirmation before it is re-run. */
  const [rerunId, setRerunId] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);

  const close = () => {
    setMenu(null);
    setSub(null);
    setConfirmId(null);
    setRerunId(null);
  };

  // Load the Enhancements lists when that menu opens; they are cheap and rarely
  // seen, so there is no reason to hit disk until then. The run-once record is
  // per-workspace, so it comes from the app state alongside the file lists.
  const loadEnhancements = useCallback(async () => {
    try {
      const [nextInstructions, nextPrompts, nextRuns] = await Promise.all([
        api.listEnhancements(),
        api.listPrompts(),
        api.agentRuns(),
      ]);
      setInstructions(nextInstructions);
      setPrompts(nextPrompts);
      setRuns(nextRuns);
      setError(null);
    } catch (e) {
      setError(api.errorMessage(e));
    }
  }, []);

  useEffect(() => {
    if (menu === "enhancements") void loadEnhancements();
  }, [menu, loadEnhancements]);

  const openMenu = (which: TopMenu) => {
    setMenu((was) => (was === which ? null : which));
    setSub(null);
  };

  // While a menu is already open, hovering the other top-level button switches
  // to it — the expected menu-bar feel.
  const hoverMenu = (which: TopMenu) => {
    if (menu !== null && menu !== which) {
      setMenu(which);
      setSub(null);
    }
  };

  const toggleInstruction = async (info: EnhancementInfo) => {
    if (busy) return;
    setBusy(info.id);
    try {
      setInstructions(
        info.installed
          ? await api.removeEnhancement(info.id)
          : await api.addEnhancement(info.id),
      );
      setError(null);
    } catch (e) {
      setError(api.errorMessage(e));
    } finally {
      setBusy(null);
    }
  };

  // Click a prompt to run it as an agent. A run-once prompt that already ran
  // here asks first (inline), matching how an instruction confirms before it
  // writes to shared files.
  const clickPrompt = (prompt: PromptInfo) => {
    if (promptClickAction(prompt, runs) === "confirm-rerun") {
      setRerunId(prompt.id);
      return;
    }
    close();
    onRunAgent(prompt.id);
  };

  const runPrompt = (prompt: PromptInfo) => {
    close();
    onRunAgent(prompt.id);
  };

  const exit = () => {
    void getCurrentWindow().close();
  };

  const emptyInstructions = emptyMessage(instructions.length);
  const emptyPrompts = emptyPromptsMessage(prompts.length);
  const now = Date.now();

  return (
    <div className="menubar">
      {menu && <div className="dropdown-backdrop" onClick={close} />}

      {/* File */}
      <div className="dropdown">
        <button
          className={`menu-button ${menu === "file" ? "open" : ""}`}
          onClick={() => openMenu("file")}
          onMouseEnter={() => hoverMenu("file")}
        >
          File
        </button>
        {menu === "file" && (
          <div className="dropdown-menu" style={{ minWidth: 160 }}>
            <div
              className="dropdown-item"
              onClick={() => {
                close();
                onOpen();
              }}
            >
              Open…
            </div>
            <div
              className="dropdown-item"
              onClick={() => {
                close();
                onRescan();
              }}
            >
              Rescan
            </div>
            <div className="dropdown-separator" />
            <div
              className="dropdown-item"
              onClick={() => {
                close();
                exit();
              }}
            >
              Exit
            </div>
          </div>
        )}
      </div>

      {/* Enhancements */}
      <div className="dropdown">
        <button
          className={`menu-button ${menu === "enhancements" ? "open" : ""}`}
          onClick={() => openMenu("enhancements")}
          onMouseEnter={() => hoverMenu("enhancements")}
        >
          Enhancements
        </button>
        {menu === "enhancements" && (
          <div className="dropdown-menu has-submenus" style={{ minWidth: 180 }}>
            {/* Instructions submenu */}
            <div
              className="dropdown-item dropdown-submenu"
              onMouseEnter={() => setSub("instructions")}
            >
              <span>Add Instructions</span>
              <span className="submenu-caret">▸</span>
              {sub === "instructions" && (
                <div className="dropdown-menu" style={{ minWidth: 240 }}>
                  {emptyInstructions && (
                    <div className="dropdown-item muted">{emptyInstructions}</div>
                  )}
                  {instructions.map((item) => {
                    const badge = statusBadge(item);

                    // Adding writes to shared files, so it is confirmed inline
                    // first. Removing is a revert and stays one click.
                    if (confirmId === item.id) {
                      return (
                        <div key={item.id} className="dropdown-item confirm-row">
                          <span style={{ flex: 1 }}>{confirmAddMessage(item.title)}</span>
                          <button
                            className="confirm-yes"
                            onClick={(e) => {
                              e.stopPropagation();
                              setConfirmId(null);
                              void toggleInstruction(item);
                            }}
                          >
                            Add
                          </button>
                          <button
                            className="confirm-no"
                            onClick={(e) => {
                              e.stopPropagation();
                              setConfirmId(null);
                            }}
                          >
                            Cancel
                          </button>
                        </div>
                      );
                    }

                    return (
                      <div
                        key={item.id}
                        className={`dropdown-item ${item.installed ? "selected" : ""}`}
                        title={actionTitle(item)}
                        onClick={() =>
                          item.installed
                            ? void toggleInstruction(item)
                            : setConfirmId(item.id)
                        }
                      >
                        <span
                          style={{ flex: 1, overflow: "hidden", textOverflow: "ellipsis" }}
                        >
                          {item.title}
                        </span>
                        {busy === item.id && <span className="faint">…</span>}
                        {badge && <span className="badge">{badge}</span>}
                        {item.installed && (
                          <span
                            className="remove"
                            role="button"
                            title="Remove this section from CLAUDE.md and AGENTS.md"
                            onClick={(e) => {
                              e.stopPropagation();
                              void toggleInstruction(item);
                            }}
                          >
                            ✕
                          </span>
                        )}
                      </div>
                    );
                  })}
                </div>
              )}
            </div>

            {/* Run Agent submenu */}
            <div
              className="dropdown-item dropdown-submenu"
              onMouseEnter={() => {
                setSub("prompts");
                setConfirmId(null);
                setRerunId(null);
              }}
            >
              <span>Run Agent</span>
              <span className="submenu-caret">▸</span>
              {sub === "prompts" && (
                <div className="dropdown-menu" style={{ minWidth: 240 }}>
                  {emptyPrompts && (
                    <div className="dropdown-item muted">{emptyPrompts}</div>
                  )}
                  {prompts.map((prompt) => {
                    const badge = runBadge(prompt, runs, now);

                    // A run-once prompt that already ran here confirms inline
                    // before re-running, like an instruction confirms its write.
                    if (rerunId === prompt.id) {
                      return (
                        <div key={prompt.id} className="dropdown-item confirm-row">
                          <span style={{ flex: 1 }}>{confirmRerunMessage(prompt.title)}</span>
                          <button
                            className="confirm-yes"
                            onClick={(e) => {
                              e.stopPropagation();
                              setRerunId(null);
                              runPrompt(prompt);
                            }}
                          >
                            Run
                          </button>
                          <button
                            className="confirm-no"
                            onClick={(e) => {
                              e.stopPropagation();
                              setRerunId(null);
                            }}
                          >
                            Cancel
                          </button>
                        </div>
                      );
                    }

                    return (
                      <div
                        key={prompt.id}
                        className="dropdown-item"
                        title="Run this prompt as an agent"
                        onClick={() => clickPrompt(prompt)}
                      >
                        <span
                          style={{ flex: 1, overflow: "hidden", textOverflow: "ellipsis" }}
                        >
                          {prompt.title}
                        </span>
                        {badge && <span className="badge">{badge}</span>}
                      </div>
                    );
                  })}
                </div>
              )}
            </div>

            <div className="dropdown-separator" />
            {/* Mirrors the Changes-tab Review button, so a review is reachable
                without leaving the current tab. */}
            <div
              className="dropdown-item"
              title="Run an adversarial review of the current changes (Claude Code or Codex)"
              onClick={() => {
                close();
                onOpenReview();
              }}
            >
              Review changes…
            </div>

            {error && (
              <>
                <div className="dropdown-separator" />
                <div className="dropdown-item error" style={{ fontSize: 11 }}>
                  {error}
                </div>
              </>
            )}
          </div>
        )}
      </div>
    </div>
  );
}

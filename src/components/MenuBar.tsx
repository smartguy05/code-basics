import { getCurrentWindow } from "@tauri-apps/api/window";
import { useCallback, useEffect, useState } from "react";
import * as api from "../ipc/api";
import type { EnhancementInfo, PromptInfo } from "../ipc/types";
import {
  actionTitle,
  confirmAddMessage,
  copyFeedback,
  emptyMessage,
  emptyPromptsMessage,
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
 * - **Instructions** — add/remove a marker-bounded section in `CLAUDE.md` /
 *   `AGENTS.md` (all logic in `cb_core::enhancements`).
 * - **Prompts** — copy a prompt's body to the clipboard; nothing is written.
 *
 * The menu bar sits above the dropdown backdrop (see `styles.css`) so hovering
 * one top-level menu while another is open switches between them.
 */
type TopMenu = "file" | "enhancements";
type Submenu = "instructions" | "prompts";

export function MenuBar({
  onOpen,
  onRescan,
}: {
  onOpen: () => void;
  onRescan: () => void;
}) {
  const [menu, setMenu] = useState<TopMenu | null>(null);
  const [sub, setSub] = useState<Submenu | null>(null);
  const [instructions, setInstructions] = useState<EnhancementInfo[]>([]);
  const [prompts, setPrompts] = useState<PromptInfo[]>([]);
  const [busy, setBusy] = useState<string | null>(null);
  const [copied, setCopied] = useState<string | null>(null);
  /** Instruction awaiting the user's confirmation before it is written. */
  const [confirmId, setConfirmId] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);

  const close = () => {
    setMenu(null);
    setSub(null);
    setCopied(null);
    setConfirmId(null);
  };

  // Load the Enhancements lists when that menu opens; they are cheap and rarely
  // seen, so there is no reason to hit disk until then.
  const loadEnhancements = useCallback(async () => {
    try {
      const [nextInstructions, nextPrompts] = await Promise.all([
        api.listEnhancements(),
        api.listPrompts(),
      ]);
      setInstructions(nextInstructions);
      setPrompts(nextPrompts);
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

  const copyPrompt = async (prompt: PromptInfo) => {
    try {
      await navigator.clipboard.writeText(prompt.body);
      setCopied(prompt.id);
      setError(null);
    } catch (e) {
      setError(api.errorMessage(e));
    }
  };

  const exit = () => {
    void getCurrentWindow().close();
  };

  const emptyInstructions = emptyMessage(instructions.length);
  const emptyPrompts = emptyPromptsMessage(prompts.length);

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
              <span>Instructions</span>
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

            {/* Prompts submenu */}
            <div
              className="dropdown-item dropdown-submenu"
              onMouseEnter={() => {
                setSub("prompts");
                setConfirmId(null);
              }}
            >
              <span>Prompts</span>
              <span className="submenu-caret">▸</span>
              {sub === "prompts" && (
                <div className="dropdown-menu" style={{ minWidth: 240 }}>
                  {emptyPrompts && (
                    <div className="dropdown-item muted">{emptyPrompts}</div>
                  )}
                  {prompts.map((prompt) => (
                    <div
                      key={prompt.id}
                      className="dropdown-item"
                      title="Copy this prompt to the clipboard"
                      onClick={() => void copyPrompt(prompt)}
                    >
                      <span
                        style={{ flex: 1, overflow: "hidden", textOverflow: "ellipsis" }}
                      >
                        {prompt.title}
                      </span>
                      {copied === prompt.id && (
                        <span className="badge">{copyFeedback(prompt.title)}</span>
                      )}
                    </div>
                  ))}
                </div>
              )}
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

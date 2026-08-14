import { useState } from "react";
import * as api from "../ipc/api";
import {
  canRejectInMode,
  importFeedback,
  intentDataHint,
  rejectFeedback,
  rejectReasonError,
} from "./intentPanelLogic";
import type {
  ComparisonMode,
  Confidence,
  GroupFile,
  GroupKind,
  InstallPlan,
  InstallScope,
  IntentGroup,
  ProviderId,
  ProviderStatus,
  RejectSummary,
} from "../ipc/types";

/**
 * The Changes tab's intent view: hunks collapsed into the decisions behind
 * them.
 *
 * Twelve hunks are twelve things to read even when they are really four. Each
 * card here is one of those four, and can be staged or reverted as a unit.
 *
 * The design rule throughout is that a card must never look more certain than
 * it is. A group derived from what the agent actually said reads differently
 * from one guessed at from a function name, and a hunk nothing could explain
 * is shown as unexplained rather than folded into a neighbour.
 */

const KIND_LABEL: Record<GroupKind, string> = {
  intent: "Stated",
  sameTurn: "One turn",
  formatting: "Formatting",
  newSymbol: "New",
  modifiedSymbol: "Changed",
  other: "Unexplained",
};

/** Only a stated intent is a real reason; the rest are locations. */
const KIND_TITLE: Record<GroupKind, string> = {
  intent: "The agent said this is what it was doing",
  sameTurn:
    "One turn made these changes, but never said why — the title describes them rather than explaining them",
  formatting: "Whitespace only — no code changed",
  newSymbol: "A symbol that is not in the baseline",
  modifiedSymbol: "An existing symbol whose body changed",
  other: "Nothing could be determined; grouped by file",
};

const CONFIDENCE_MARK: Record<Confidence, string> = {
  high: "●●●",
  medium: "●●○",
  low: "●○○",
};

const PROVIDER_LABEL: Record<ProviderId, string> = {
  claudeCode: "Claude Code",
  codex: "Codex",
};

export interface IntentPanelProps {
  groups: IntentGroup[];
  providers: ProviderStatus[];
  selectedGroup: string | null;
  /** The file open in the diff pane, so the card can mark its row. */
  selectedPath: string | null;
  /** Which view is displayed — rejecting is only possible in a working-tree one. */
  mode: ComparisonMode;
  busy: boolean;
  onSelect: (group: IntentGroup) => void;
  /** Open one file of the group, scoped to the group's hunks in it. */
  onSelectFile: (group: IntentGroup, file: GroupFile) => void;
  onStage: (group: IntentGroup) => void;
  onRevert: (group: IntentGroup) => void;
  onStageFile: (group: IntentGroup, file: GroupFile) => void;
  onRevertFile: (group: IntentGroup, file: GroupFile) => void;
  /**
   * Revert the group — or one file's share of it — and leave `reason` in the
   * code for the agent to read.
   *
   * Resolves with what happened, so the panel can report it inline; `null` when
   * the action failed and the view has already said so.
   */
  onReject: (
    group: IntentGroup,
    reason: string,
    file?: GroupFile,
  ) => Promise<RejectSummary | null>;
  onEnable: (provider: ProviderId, scope: InstallScope) => Promise<void>;
  /** Resolves with how many records the import found, so the panel can say so. */
  onImportHistory: () => Promise<number>;
}

export function IntentPanel({
  groups,
  providers,
  selectedGroup,
  selectedPath,
  mode,
  busy,
  onSelect,
  onSelectFile,
  onStage,
  onRevert,
  onStageFile,
  onRevertFile,
  onReject,
  onEnable,
  onImportHistory,
}: IntentPanelProps) {
  const stated = groups.filter((g) => g.kind === "intent").length;
  const capturing = providers.some((p) => p.capture != null);
  const hint = intentDataHint(groups, providers);

  // Open by default until capture is set up. The panel is the only way to turn
  // it on, and a feature nobody can find is a feature that does not exist.
  const [setupOpen, setSetupOpen] = useState(!capturing);
  const [feedback, setFeedback] = useState<string | null>(null);

  const runImport = async () => {
    setFeedback(null);
    const total = await onImportHistory();
    setFeedback(importFeedback(total));
  };

  const runReject = async (group: IntentGroup, reason: string, file?: GroupFile) => {
    setFeedback(null);
    const summary = await onReject(group, reason, file);
    if (summary) setFeedback(rejectFeedback(summary));
  };

  return (
    <>
      <button
        className="intent-setup"
        onClick={() => setSetupOpen((open) => !open)}
        title="Set up agent intent capture, or import past sessions"
      >
        <span className="twisty">{setupOpen ? "▾" : "▸"}</span>
        <span style={{ flex: 1 }}>
          {capturing ? "Agent intent capture" : "Set up agent intent capture…"}
        </span>
        <span className="badge">{capturing ? "on" : "off"}</span>
      </button>

      {setupOpen && (
        <CaptureSetup
          providers={providers}
          busy={busy}
          onEnable={onEnable}
          onImportHistory={runImport}
        />
      )}

      {feedback && (
        <div className="muted" style={{ padding: "6px 8px", fontSize: 11 }}>
          {feedback}
        </div>
      )}

      {hint.kind === "hint" && (
        <div className="warning" style={{ fontSize: 12 }}>
          <div>{hint.text}</div>

          {hint.caveats.map((caveat) => (
            <div key={caveat} style={{ fontSize: 11, marginTop: 4 }}>
              {caveat}
            </div>
          ))}

          <div className="actions" style={{ display: "flex", gap: 4, marginTop: 6 }}>
            {hint.canEnable && (
              <button
                disabled={busy}
                onClick={() => setSetupOpen(true)}
                title="Show what turning capture on would write, before writing anything"
              >
                Enable capture
              </button>
            )}
            {hint.canImport && (
              <button
                disabled={busy}
                onClick={() => void runImport()}
                title="Read what the agents already recorded — no setup required"
              >
                Import past sessions ({hint.sessions})
              </button>
            )}
          </div>
        </div>
      )}

      <div className="group-label" style={{ display: "flex", alignItems: "center", gap: 4 }}>
        <span style={{ flex: 1 }}>
          {groups.length} group{groups.length === 1 ? "" : "s"}
        </span>
        <span
          className="badge"
          title={
            stated > 0
              ? `${stated} group(s) come from what the agent actually said; the rest are inferred`
              : "Nothing is agent-stated yet — every group here is inferred from the diff"
          }
        >
          {stated} stated
        </span>
      </div>

      {groups.length === 0 && (
        <div className="muted" style={{ padding: 8, fontSize: 12 }}>
          No changes to group.
        </div>
      )}

      {groups.map((group) => (
        <GroupCard
          key={group.id}
          group={group}
          selected={group.id === selectedGroup}
          selectedPath={group.id === selectedGroup ? selectedPath : null}
          mode={mode}
          busy={busy}
          onSelect={onSelect}
          onSelectFile={onSelectFile}
          onStage={onStage}
          onRevert={onRevert}
          onStageFile={onStageFile}
          onRevertFile={onRevertFile}
          onReject={(group, reason, file) => void runReject(group, reason, file)}
        />
      ))}
    </>
  );
}

function GroupCard({
  group,
  selected,
  selectedPath,
  mode,
  busy,
  onSelect,
  onSelectFile,
  onStage,
  onRevert,
  onStageFile,
  onRevertFile,
  onReject,
}: {
  group: IntentGroup;
  selected: boolean;
  selectedPath: string | null;
  mode: ComparisonMode;
  busy: boolean;
  onSelect: (group: IntentGroup) => void;
  onSelectFile: (group: IntentGroup, file: GroupFile) => void;
  onStage: (group: IntentGroup) => void;
  onRevert: (group: IntentGroup) => void;
  onStageFile: (group: IntentGroup, file: GroupFile) => void;
  onRevertFile: (group: IntentGroup, file: GroupFile) => void;
  onReject: (group: IntentGroup, reason: string, file?: GroupFile) => void;
}) {
  const hunks = group.files.reduce((total, file) => total + file.hunks.length, 0);

  // Which reject is being composed: the whole group, or one file of it. `null`
  // means no prompt is open.
  const [rejecting, setRejecting] = useState<{ file: GroupFile | null } | null>(null);
  const [reason, setReason] = useState("");
  const [error, setError] = useState<string | null>(null);

  const canReject = canRejectInMode(mode);
  const rejectTitle = canReject
    ? "Revert this and leave the reason in the code for the agent to fix"
    : "Rejecting works on the working tree — switch out of the staged view";

  const openReject = (file: GroupFile | null) => {
    setRejecting({ file });
    setReason("");
    setError(null);
  };

  const confirmReject = () => {
    const problem = rejectReasonError(reason);
    if (problem) {
      setError(problem);
      return;
    }
    const file = rejecting?.file ?? undefined;
    setRejecting(null);
    onReject(group, reason, file);
  };

  return (
    <div
      className={`intent-card ${selected ? "selected" : ""}`}
      onClick={() => onSelect(group)}
      title={KIND_TITLE[group.kind]}
    >
      <div className="headline">
        <span className={`kind ${group.kind}`}>{KIND_LABEL[group.kind]}</span>
        <span className="label">{group.label}</span>
        <span className="confidence" title={`${group.confidence} confidence`}>
          {CONFIDENCE_MARK[group.confidence]}
        </span>
      </div>

      <div className="meta">
        {group.files.length} file{group.files.length === 1 ? "" : "s"} · {hunks} hunk
        {hunks === 1 ? "" : "s"} · {group.lineCount} line
        {group.lineCount === 1 ? "" : "s"}
      </div>

      {selected && (
        <>
          <div className="group-files">
            {group.files.map((file) => (
              <div
                key={file.path}
                className={`group-file ${file.path === selectedPath ? "selected" : ""}`}
                onClick={(e) => {
                  e.stopPropagation();
                  onSelectFile(group, file);
                }}
                title="Show only this group's changes in this file"
              >
                <span className="path">{file.path}</span>
                <span className="faint" style={{ fontSize: 11, whiteSpace: "nowrap" }}>
                  {file.hunks.length} hunk{file.hunks.length === 1 ? "" : "s"} ·{" "}
                  {file.lineIndices.length} line{file.lineIndices.length === 1 ? "" : "s"}
                </span>
                {group.files.length > 1 && (
                  <span className="file-actions">
                    <button
                      disabled={busy}
                      onClick={(e) => {
                        e.stopPropagation();
                        onStageFile(group, file);
                      }}
                      title="Stage only this file's share of the group"
                    >
                      Stage
                    </button>
                    <button
                      disabled={busy}
                      onClick={(e) => {
                        e.stopPropagation();
                        onRevertFile(group, file);
                      }}
                      title="Revert only this file's share of the group"
                    >
                      Revert
                    </button>
                    <button
                      disabled={busy || !canReject}
                      onClick={(e) => {
                        e.stopPropagation();
                        openReject(file);
                      }}
                      title={rejectTitle}
                    >
                      Reject…
                    </button>
                  </span>
                )}
              </div>
            ))}
          </div>
          <div className="actions">
            <button
              disabled={busy}
              onClick={(e) => {
                e.stopPropagation();
                onStage(group);
              }}
              title="Stage every line in this group, across every file"
            >
              Stage group
            </button>
            <button
              disabled={busy}
              onClick={(e) => {
                e.stopPropagation();
                onRevert(group);
              }}
              title="Revert every line in this group, across every file"
            >
              Revert group
            </button>
            <button
              disabled={busy || !canReject}
              onClick={(e) => {
                e.stopPropagation();
                openReject(null);
              }}
              title={rejectTitle}
            >
              Reject group…
            </button>
          </div>

          {rejecting && (
            <div className="reject-prompt" onClick={(e) => e.stopPropagation()}>
              <label style={{ fontSize: 11 }}>
                Why is{" "}
                {rejecting.file ? <code>{rejecting.file.path}</code> : "this"} wrong?
              </label>
              <input
                autoFocus
                value={reason}
                placeholder="e.g. matches a column named limit"
                onChange={(e) => {
                  setReason(e.target.value);
                  setError(null);
                }}
                onKeyDown={(e) => {
                  if (e.key === "Enter") confirmReject();
                  if (e.key === "Escape") setRejecting(null);
                }}
              />
              {error && (
                <div className="error" style={{ fontSize: 11 }}>
                  {error}
                </div>
              )}
              <div className="faint" style={{ fontSize: 11 }}>
                Reverts the change and leaves the reason as a comment. The commit
                is blocked until the comment is gone.
              </div>
              <div className="actions">
                <button disabled={busy} onClick={confirmReject}>
                  Reject
                </button>
                <button disabled={busy} onClick={() => setRejecting(null)}>
                  Cancel
                </button>
              </div>
            </div>
          )}
        </>
      )}
    </div>
  );
}

/**
 * Turning capture on.
 *
 * Every install writes outside this view's control — into a file that is
 * committed and shared with a team, or into an agent's global configuration
 * that may already drive unrelated tooling. So nothing is written until the
 * exact contents have been shown and confirmed.
 */
function CaptureSetup({
  providers,
  busy,
  onEnable,
  onImportHistory,
}: {
  providers: ProviderStatus[];
  busy: boolean;
  onEnable: (provider: ProviderId, scope: InstallScope) => Promise<void>;
  onImportHistory: () => Promise<void>;
}) {
  const [plan, setPlan] = useState<InstallPlan | null>(null);
  const [error, setError] = useState<string | null>(null);

  const preview = async (provider: ProviderId, scope: InstallScope) => {
    try {
      setPlan(await api.intentInstallPlan(provider, scope));
      setError(null);
    } catch (e) {
      setError(api.errorMessage(e));
    }
  };

  const confirm = async () => {
    if (!plan) return;
    try {
      await onEnable(plan.provider, plan.scope);
      setPlan(null);
      setError(null);
    } catch (e) {
      setError(api.errorMessage(e));
    }
  };

  const detected = providers.filter((p) => p.detected);

  const totalSessions = providers.reduce((sum, p) => sum + p.sessions, 0);

  return (
    <div className="intent-panel">
      {detected.length === 0 && (
        <div className="muted" style={{ fontSize: 12 }}>
          No coding agent detected on this machine.
        </div>
      )}

      {detected.map((status) => (
        <div key={status.provider} className="provider">
          <div className="provider-name">
            <strong>{PROVIDER_LABEL[status.provider]}</strong>
            {status.capture ? (
              <span className="badge" title={`Hooks installed at ${status.capture} level`}>
                capturing ({status.capture})
              </span>
            ) : (
              <span className="faint" style={{ fontSize: 11 }}>
                not capturing
              </span>
            )}
          </div>

          {status.sessions > 0 && (
            <div className="faint" style={{ fontSize: 11 }}>
              {status.sessions} past session{status.sessions === 1 ? "" : "s"} for this
              workspace
            </div>
          )}

          {status.caveats?.map((caveat) => (
            <div key={caveat} className="warning" style={{ fontSize: 11, marginTop: 2 }}>
              {caveat}
            </div>
          ))}

          {!status.capture && (
            <div className="actions">
              <button
                disabled={busy}
                onClick={() => void preview(status.provider, "project")}
                title="Write the hook into this repository, shared with anyone who clones it"
              >
                Enable for this repo…
              </button>
              <button
                disabled={busy}
                onClick={() => void preview(status.provider, "user")}
                title="Write the hook into your own configuration, covering every repository"
              >
                Enable for me…
              </button>
            </div>
          )}

          {status.capture && (
            <div className="actions">
              <button
                disabled={busy}
                onClick={() => void preview(status.provider, status.capture!)}
                title="Re-write the hook and the instruction section at the same level — useful after an update changed either"
              >
                Re-apply setup…
              </button>
            </div>
          )}
        </div>
      ))}

      <button
        disabled={busy}
        onClick={() => void onImportHistory()}
        title="Read what the agents already recorded — no setup, works on changes already in your tree"
      >
        Import past sessions{totalSessions > 0 ? ` (${totalSessions})` : ""}
      </button>

      {error && <div className="error" style={{ fontSize: 11 }}>{error}</div>}

      {plan && (
        <div style={{ marginTop: 8 }}>
          <div style={{ fontSize: 12, marginBottom: 4 }}>
            This will write {plan.writes.length} file
            {plan.writes.length === 1 ? "" : "s"}:
          </div>

          {plan.caveats?.map((caveat) => (
            <div key={caveat} className="warning" style={{ fontSize: 11, marginBottom: 4 }}>
              {caveat}
            </div>
          ))}

          {plan.writes.map((write) => (
            <details key={write.path}>
              <summary style={{ fontSize: 11, cursor: "pointer" }}>
                {write.path}
                {write.mergesExisting && (
                  <span className="badge" style={{ marginLeft: 4 }}>
                    merges into existing
                  </span>
                )}
              </summary>
              <pre>{write.content}</pre>
            </details>
          ))}

          <div className="actions">
            <button disabled={busy} onClick={() => void confirm()}>
              Write these files
            </button>
            <button disabled={busy} onClick={() => setPlan(null)}>
              Cancel
            </button>
          </div>
        </div>
      )}
    </div>
  );
}

import { useEffect, useState } from "react";
import * as api from "../ipc/api";
import {
  canRejectInMode,
  cardRisk,
  cardTitle,
  groupStagedState,
  importFeedback,
  intentDataHint,
  rejectFeedback,
  rejectReasonError,
  scopeCreep,
  scorecardLine,
  stagedState,
  unfulfilledCaption,
  type StagedState,
} from "./intentPanelLogic";
import {
  behavioralBadge,
  behavioralScoreLine,
  deltaLine,
} from "./behavioralPanelLogic";
import type {
  BehavioralReport,
  CardBehavior,
  ComparisonMode,
  Confidence,
  ErosionFlag,
  FileChange,
  GroupFile,
  GroupKind,
  InstallPlan,
  InstallScope,
  IntentGroup,
  ProviderId,
  ProviderStatus,
  RejectSummary,
  Scorecard,
  UnfulfilledClaim,
} from "../ipc/types";

const STAGE_TAG: Record<StagedState, { label: string; className: string; title: string } | null> = {
  staged: { label: "staged", className: "staged", title: "Staged — in the index" },
  partial: {
    label: "partial",
    className: "partial",
    title: "Partially staged — some hunks are in the index, some are not",
  },
  none: null,
};

/** The small staged/partial pill shown on a file row or card. `none` renders nothing. */
function StageTag({ state }: { state: StagedState }) {
  const tag = STAGE_TAG[state];
  if (!tag) return null;
  return (
    <span className={`stage-tag ${tag.className}`} title={tag.title}>
      {tag.label}
    </span>
  );
}

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

/** The small before/after pill on a card, coloured by `behavioralBadge`. */
function BehavioralBadge({ card }: { card: CardBehavior }) {
  const badge = behavioralBadge(card);
  return (
    <span className={`behavioral-badge ${badge.tone}`} title={badge.title}>
      {badge.label}
    </span>
  );
}

/**
 * The quiet risk pill on a card, shown only when {@link cardRisk} elevates it.
 * The reasons become the tooltip so hovering says exactly why it is flagged.
 */
function RiskBadge({ risk }: { risk: { level: "elevated" | "high"; reasons: string[] } }) {
  // Styled inline so the badge is self-contained: high borrows the fail colour,
  // elevated stays muted (the quiet default the abstain rule wants).
  const color = risk.level === "high" ? "var(--fail)" : "var(--text-faint)";
  return (
    <span
      className={`risk-badge ${risk.level}`}
      title={`Worth a closer look:\n${risk.reasons.map((r) => `• ${r}`).join("\n")}`}
      style={{
        fontSize: 10,
        padding: "1px 5px",
        marginLeft: 6,
        borderRadius: 3,
        border: `1px solid ${color}`,
        color,
        whiteSpace: "nowrap",
      }}
    >
      {risk.level === "high" ? "high risk" : "review"}
    </span>
  );
}

export interface IntentPanelProps {
  groups: IntentGroup[];
  /** The per-turn coverage tally, shown above the cards. */
  scorecard: Scorecard;
  /** Declared intents no changed hunk evidences — shown as review items. */
  unfulfilled: UnfulfilledClaim[];
  providers: ProviderStatus[];
  selectedGroup: string | null;
  /** The file open in the diff pane, so the card can mark its row. */
  selectedPath: string | null;
  /**
   * The working-tree status, so each card and file can show whether it is
   * staged — the Intent view has no Staged section of its own.
   */
  statusFiles: FileChange[];
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
  /**
   * The runtime before/after comparison, when one has been run. Its
   * per-card attributions badge the matching cards; its unattributed deltas
   * appear only in the overall section, never pinned to a card.
   */
  behavioral?: BehavioralReport | null;
  /**
   * The erosion flags for the current diff, so a card can show a risk badge
   * when one lands on its own lines. Absent before a scan has run — treated as
   * none, so the badge simply stays quiet rather than guessing.
   */
  erosionFlags?: ErosionFlag[];
}

export function IntentPanel({
  groups,
  scorecard,
  unfulfilled,
  providers,
  selectedGroup,
  selectedPath,
  statusFiles,
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
  behavioral,
  erosionFlags,
}: IntentPanelProps) {
  const capturing = providers.some((p) => p.capture != null);
  const flags = erosionFlags ?? [];
  // Attributions keyed by group id, so a card can find its own behavior in O(1).
  const behaviorByGroup = new Map(
    (behavioral?.attributions ?? []).map((card) => [card.groupId, card]),
  );
  const hint = intentDataHint(groups, providers);
  const unfulfilledHeading = unfulfilledCaption(unfulfilled);
  const scope = scopeCreep(scorecard, groups);

  // Open by default until capture is set up. The panel is the only way to turn
  // it on, and a feature nobody can find is a feature that does not exist.
  const [setupOpen, setSetupOpen] = useState(!capturing);
  const [feedback, setFeedback] = useState<string | null>(null);
  // The unexplained cards are collected under one collapsible section — they
  // sort to the top and can be a wall of noise a reviewer wants to fold away.
  const [unexplainedCollapsed, setUnexplainedCollapsed] = useState(false);

  const unexplained = groups.filter((g) => g.kind === "other");
  const explained = groups.filter((g) => g.kind !== "other");

  const renderCard = (group: IntentGroup) => (
    <GroupCard
      key={group.id}
      group={group}
      behavior={behaviorByGroup.get(group.id) ?? null}
      risk={cardRisk(group, flags)}
      selected={group.id === selectedGroup}
      selectedPath={group.id === selectedGroup ? selectedPath : null}
      statusFiles={statusFiles}
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
  );

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
        <>
          <CaptureSetup
            providers={providers}
            busy={busy}
            onEnable={onEnable}
            onImportHistory={runImport}
          />
          <QualityGateSetup busy={busy} />
        </>
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
        {scorecard.unmatched > 0 && (
          <span
            className="badge warning"
            title="Stated intents with no matching change in this diff"
          >
            {scorecard.unmatched} unmatched
          </span>
        )}
      </div>

      {groups.length > 0 && (
        <div
          className="muted"
          style={{ padding: "2px 8px 6px", fontSize: 11 }}
          title="Claims are declared intents; evidenced means a matching change was found. Hunks and unattributed count the diff itself."
        >
          {scorecardLine(scorecard)}
        </div>
      )}

      {scope && (
        <div
          className={scope.level === "high" ? "warning" : "muted"}
          style={{ padding: "2px 8px 6px", fontSize: 11 }}
          // Informational only: a nudge to glance at scope, never an accusation
          // that anything is out of place. High styling is reserved for when
          // both signals are substantial at once.
          title="A soft scope check derived from the unexplained groups and unattributed lines above — worth a look, not a verdict."
        >
          {scope.message}
        </div>
      )}

      {behavioral && (
        <div className="behavioral-overall">
          <div
            className="muted"
            style={{ padding: "2px 8px 4px", fontSize: 11 }}
            title="The runtime before/after run: outcomes it compared between HEAD and the working tree, and how many differences it could attribute to a card."
          >
            {behavioralScoreLine(behavioral.scorecard)}
          </div>

          {behavioral.unattributed.length > 0 && (
            <div style={{ padding: "2px 8px 4px" }}>
              <div
                className="faint"
                style={{ fontSize: 11, marginBottom: 2 }}
                title="Observable differences no card could be held responsible for — shown here rather than pinned to a card that may not have caused them."
              >
                Unattributed differences
              </div>
              {behavioral.unattributed.map((delta, i) => {
                const line = deltaLine(delta);
                return (
                  <div
                    key={`${line.text}:${i}`}
                    className={`behavioral-delta ${line.tone}`}
                    style={{ fontSize: 11 }}
                  >
                    {line.text}
                  </div>
                );
              })}
            </div>
          )}

          {behavioral.warnings.map((warning) => (
            <div key={warning} className="warning" style={{ fontSize: 11 }}>
              {warning}
            </div>
          ))}
        </div>
      )}

      {unfulfilledHeading && (
        <div className="warning" style={{ fontSize: 12 }}>
          <div style={{ marginBottom: 4 }}>{unfulfilledHeading}</div>
          {unfulfilled.map((claim) => (
            <div
              key={`${claim.turnId}:${claim.label}`}
              style={{ fontSize: 11, marginTop: 2 }}
              // These are informational: there is nothing in the tree to act on,
              // and a stated intent may simply have landed and moved past the
              // matcher's reach rather than being undone.
              title={
                "The agent stated this, but no changed hunk matches it here. " +
                "It may be committed, later overwritten, hand-reverted, or " +
                "reformatted past the matcher — reported, not assumed undone."
              }
            >
              <span className="label">{claim.label}</span>
              {claim.paths.length > 0 && (
                <span className="faint"> — {claim.paths.join(", ")}</span>
              )}
            </div>
          ))}
        </div>
      )}

      {groups.length === 0 && (
        <div className="muted" style={{ padding: 8, fontSize: 12 }}>
          No changes to group.
        </div>
      )}

      {unexplained.length > 0 && (
        <>
          <div
            className="group-label dropdown-section"
            style={{ display: "flex", alignItems: "center", gap: 4 }}
            onClick={() => setUnexplainedCollapsed((open) => !open)}
            title="Changes no stated intent accounts for — review these first"
          >
            <span className="twisty">{unexplainedCollapsed ? "▸" : "▾"}</span>
            <span style={{ flex: 1 }}>Unexplained</span>
            <span className="badge">{unexplained.length}</span>
          </div>
          {!unexplainedCollapsed && unexplained.map(renderCard)}
        </>
      )}

      {explained.map(renderCard)}
    </>
  );
}

function GroupCard({
  group,
  behavior,
  risk,
  selected,
  selectedPath,
  statusFiles,
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
  behavior: CardBehavior | null;
  risk: { level: "elevated" | "high"; reasons: string[] } | null;
  selected: boolean;
  selectedPath: string | null;
  statusFiles: FileChange[];
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
      title={cardTitle(group, KIND_TITLE[group.kind])}
    >
      <div className="headline">
        <span className={`kind ${group.kind}`}>{KIND_LABEL[group.kind]}</span>
        <span className="label">{group.label}</span>
        {behavior && <BehavioralBadge card={behavior} />}
        {risk && <RiskBadge risk={risk} />}
        <StageTag state={groupStagedState(group.files.map((f) => f.path), statusFiles)} />
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
                // The full path titles the whole row: the name is truncated
                // with an ellipsis, and a nested title on the inner span is not
                // reliably shown over the row's own title.
                title={`${file.path}\nClick to show only this group's changes in this file`}
              >
                <span className="path">{file.path}</span>
                <StageTag state={stagedState(file.path, statusFiles)} />
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
        <PlanPreview
          plan={plan}
          busy={busy}
          onConfirm={() => void confirm()}
          onCancel={() => setPlan(null)}
        />
      )}
    </div>
  );
}

/**
 * The confirmation view for an {@link InstallPlan}: what will be written, with
 * the exact final contents of each file, before anything touches disk. Shared
 * by intent capture and the quality gate — both write outside this view's
 * control, into files a team shares.
 */
function PlanPreview({
  plan,
  busy,
  onConfirm,
  onCancel,
}: {
  plan: InstallPlan;
  busy: boolean;
  onConfirm: () => void;
  onCancel: () => void;
}) {
  return (
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
        <button disabled={busy} onClick={onConfirm}>
          Write these files
        </button>
        <button disabled={busy} onClick={onCancel}>
          Cancel
        </button>
      </div>
    </div>
  );
}

/**
 * Turning the quality gate on. Installed the same way capture is — a previewed
 * plan applied to `.claude/settings.json` — but self-contained: it owns its own
 * status, since it is a single Claude Code hook rather than a per-agent matrix.
 *
 * The gate blocks a turn that ends with a failing `pnpm typecheck` / `cargo fmt`
 * on changed files, an unresolved rejection note, and reminds (without blocking)
 * when source changed but no `.memories/` file did.
 */
function QualityGateSetup({ busy }: { busy: boolean }) {
  const [scope, setScope] = useState<InstallScope | null>(null);
  const [plan, setPlan] = useState<InstallPlan | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [loaded, setLoaded] = useState(false);

  useEffect(() => {
    let live = true;
    void api
      .qualityGateStatus()
      .then((s) => {
        if (live) {
          setScope(s);
          setLoaded(true);
        }
      })
      .catch(() => live && setLoaded(true));
    return () => {
      live = false;
    };
  }, []);

  const preview = async (target: InstallScope) => {
    try {
      setPlan(await api.qualityGateInstallPlan(target));
      setError(null);
    } catch (e) {
      setError(api.errorMessage(e));
    }
  };

  const confirm = async () => {
    if (!plan) return;
    try {
      setScope(await api.installQualityGate(plan.scope));
      setPlan(null);
      setError(null);
    } catch (e) {
      setError(api.errorMessage(e));
    }
  };

  if (!loaded) return null;

  return (
    <div className="intent-panel">
      <div className="provider">
        <div className="provider-name">
          <strong>Quality gate</strong>
          {scope ? (
            <span className="badge" title={`Stop hook installed at ${scope} level`}>
              on ({scope})
            </span>
          ) : (
            <span className="faint" style={{ fontSize: 11 }}>
              off
            </span>
          )}
        </div>

        <div className="faint" style={{ fontSize: 11 }}>
          Blocks a turn that ends with a failing typecheck / cargo fmt on changed
          files, or an unresolved rejection note; reminds about `.memories/`.
        </div>

        <div className="actions">
          {!scope ? (
            <>
              <button
                disabled={busy}
                onClick={() => void preview("project")}
                title="Write the Stop hook into this repository, shared with anyone who clones it"
              >
                Enable for this repo…
              </button>
              <button
                disabled={busy}
                onClick={() => void preview("user")}
                title="Write the Stop hook into your own configuration, covering every repository"
              >
                Enable for me…
              </button>
            </>
          ) : (
            <button
              disabled={busy}
              onClick={() => void preview(scope)}
              title="Re-write the hook at the same level — useful after an update changed the command"
            >
              Re-apply…
            </button>
          )}
        </div>
      </div>

      {error && <div className="error" style={{ fontSize: 11 }}>{error}</div>}

      {plan && (
        <PlanPreview
          plan={plan}
          busy={busy}
          onConfirm={() => void confirm()}
          onCancel={() => setPlan(null)}
        />
      )}
    </div>
  );
}

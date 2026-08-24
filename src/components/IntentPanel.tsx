import { useEffect, useRef, useState } from "react";
import * as api from "../ipc/api";
import { PlanPreview } from "./PlanPreview";
import {
  canRejectInMode,
  cardCandidates,
  cardHeadline,
  cardRisk,
  cardTitle,
  groupStagedState,
  importFeedback,
  intentDataHint,
  intentEditPlan,
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
  // Not an installable agent; present so the map is total over ProviderId.
  user: "You",
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
  /** Turn capture off for one provider at the scope it is installed. */
  onDisable: (provider: ProviderId, scope: InstallScope) => Promise<void>;
  /** Resolves with how many records the import found, so the panel can say so. */
  onImportHistory: () => Promise<number>;
  /** Write (or overwrite) the user's own intent for a card. */
  onSetIntent: (group: IntentGroup, label: string) => void;
  /** Remove the user's note from a card. */
  onClearIntent: (group: IntentGroup) => void;
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
  onDisable,
  onImportHistory,
  onSetIntent,
  onClearIntent,
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

  // Collapsed once capture is confirmed on — a working feature does not need its
  // setup pane taking up the top of the panel — and open while it is off, so the
  // one control that turns it on stays visible. This follows capture
  // *transitions* rather than being a one-shot initial value, because providers
  // load asynchronously: the first status lands after mount and flips
  // `capturing` off its empty-list default, and the pane must react to that.
  const [setupOpen, setSetupOpen] = useState(!capturing);
  const lastCapturing = useRef(capturing);
  useEffect(() => {
    if (lastCapturing.current !== capturing) {
      setSetupOpen(!capturing);
      lastCapturing.current = capturing;
    }
  }, [capturing]);

  const [feedback, setFeedback] = useState<string | null>(null);
  // The scorecard, scope check and unfulfilled-claim list answer "did the agent
  // stay on task" — worth having, but not what this view is for. They fold away
  // behind the group count so the cards themselves are what the tab shows.
  const [summaryOpen, setSummaryOpen] = useState(false);
  // The unexplained cards are collected under one collapsible section — they
  // sort to the top and can be a wall of noise a reviewer wants to fold away.
  const [unexplainedCollapsed, setUnexplainedCollapsed] = useState(false);

  const unexplained = groups.filter((g) => g.kind === "other");
  const explained = groups.filter((g) => g.kind !== "other");
  // Whether the folded summary has anything in it — no twisty, no click target
  // when there is nothing to reveal.
  const hasSummaryDetails =
    groups.length > 0 || scope != null || unfulfilledHeading != null;

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
      onSetIntent={onSetIntent}
      onClearIntent={onClearIntent}
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
            onDisable={onDisable}
            onImportHistory={runImport}
          />
          <QualityGateSetup providers={providers} busy={busy} />
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

      <div
        className="group-label intent-summary-head"
        onClick={() => hasSummaryDetails && setSummaryOpen((open) => !open)}
        title={
          hasSummaryDetails
            ? "Coverage: claims evidenced, a soft scope check, and any stated intents with no matching change in this diff"
            : undefined
        }
        style={{
          display: "flex",
          alignItems: "center",
          gap: 4,
          cursor: hasSummaryDetails ? "pointer" : "default",
        }}
      >
        {hasSummaryDetails && <span className="twisty">{summaryOpen ? "▾" : "▸"}</span>}
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

      {summaryOpen && hasSummaryDetails && (
        <div className="intent-summary">
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
              // Informational only: a nudge to glance at scope, never an
              // accusation that anything is out of place. High styling is
              // reserved for when both signals are substantial at once.
              title="A soft scope check derived from the unexplained groups and unattributed lines above — worth a look, not a verdict."
            >
              {scope.message}
            </div>
          )}

          {unfulfilledHeading && (
            <div className="warning" style={{ fontSize: 12 }}>
              <div style={{ marginBottom: 4 }}>{unfulfilledHeading}</div>
              {unfulfilled.map((claim) => (
                <div
                  key={`${claim.turnId}:${claim.label}`}
                  style={{ fontSize: 11, marginTop: 2 }}
                  // These are informational: there is nothing in the tree to act
                  // on, and a stated intent may simply have landed and moved past
                  // the matcher's reach rather than being undone.
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
  onSetIntent,
  onClearIntent,
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
  onSetIntent: (group: IntentGroup, label: string) => void;
  onClearIntent: (group: IntentGroup) => void;
}) {
  const hunks = group.files.reduce((total, file) => total + file.hunks.length, 0);

  // Writing (or overwriting) the user's own intent on this card. `plan` decides
  // the prefill, whether saving overwrites an agent intent (so a confirm is
  // shown), and whether a "Clear note" action is offered.
  const plan = intentEditPlan(group);
  const [editing, setEditing] = useState(false);
  const [label, setLabel] = useState("");
  const [confirming, setConfirming] = useState(false);

  const openEdit = () => {
    setLabel(plan.initial);
    setConfirming(false);
    setEditing(true);
  };
  const saveIntent = () => {
    const trimmed = label.trim();
    if (!trimmed) return;
    // First Save on a card with an agent intent asks to confirm the overwrite.
    if (plan.confirm && !confirming) {
      setConfirming(true);
      return;
    }
    setEditing(false);
    setConfirming(false);
    onSetIntent(group, trimmed);
  };
  const intentButtonLabel = plan.hasUserNote
    ? "Edit note…"
    : plan.confirm
      ? "Override intent…"
      : "Add intent…";

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
        <span className="label">{cardHeadline(group)}</span>
        {behavior && <BehavioralBadge card={behavior} />}
        {risk && <RiskBadge risk={risk} />}
        <StageTag state={groupStagedState(group.files.map((f) => f.path), statusFiles)} />
        <span className="confidence" title={`${group.confidence} confidence`}>
          {CONFIDENCE_MARK[group.confidence]}
        </span>
      </div>

      {cardCandidates(group).length > 0 && (
        <div className="candidates" style={{ fontSize: 11, margin: "2px 0 0" }}>
          <span className="faint">could be any of:</span>
          <ul style={{ margin: "2px 0 0", paddingLeft: 16 }}>
            {cardCandidates(group).map((reason) => (
              <li key={reason}>{reason}</li>
            ))}
          </ul>
        </div>
      )}

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
            <button
              disabled={busy}
              onClick={(e) => {
                e.stopPropagation();
                openEdit();
              }}
              title={
                plan.confirm
                  ? "Write your own intent for this change (overwrites the stated one)"
                  : "Write your own intent for this change — for a manual edit or an agent that recorded none"
              }
            >
              {intentButtonLabel}
            </button>
            {plan.hasUserNote && (
              <button
                disabled={busy}
                onClick={(e) => {
                  e.stopPropagation();
                  onClearIntent(group);
                }}
                title="Remove your note, restoring the reason or title it had before"
              >
                Clear note
              </button>
            )}
          </div>

          {editing && (
            <div className="intent-edit-prompt" onClick={(e) => e.stopPropagation()}>
              <label style={{ fontSize: 11 }}>What is this change for?</label>
              <input
                autoFocus
                value={label}
                placeholder="e.g. cache the quote lookup per request"
                onChange={(e) => {
                  setLabel(e.target.value);
                  setConfirming(false);
                }}
                onKeyDown={(e) => {
                  if (e.key === "Enter") saveIntent();
                  if (e.key === "Escape") {
                    setEditing(false);
                    setConfirming(false);
                  }
                }}
              />
              {confirming && plan.confirm && (
                <div className="warning" style={{ fontSize: 11 }}>
                  {plan.confirm}
                </div>
              )}
              <div className="faint" style={{ fontSize: 11 }}>
                Titles this card as your stated intent. It binds to these exact
                changed lines and wins over any agent reason.
              </div>
              <div className="actions">
                <button disabled={busy || !label.trim()} onClick={saveIntent}>
                  {confirming ? "Overwrite" : "Save"}
                </button>
                <button
                  disabled={busy}
                  onClick={() => {
                    setEditing(false);
                    setConfirming(false);
                  }}
                >
                  Cancel
                </button>
              </div>
            </div>
          )}

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
  onDisable,
  onImportHistory,
}: {
  providers: ProviderStatus[];
  busy: boolean;
  onEnable: (provider: ProviderId, scope: InstallScope) => Promise<void>;
  onDisable: (provider: ProviderId, scope: InstallScope) => Promise<void>;
  onImportHistory: () => Promise<void>;
}) {
  // A pending preview is either an install or an uninstall; the confirm applies
  // whichever kind was previewed. An empty uninstall plan (nothing to remove)
  // is shown as such rather than offered as a write of zero files.
  const [pending, setPending] = useState<{ plan: InstallPlan; kind: "enable" | "disable" } | null>(
    null,
  );
  const [error, setError] = useState<string | null>(null);
  const plan = pending?.plan ?? null;

  const preview = async (provider: ProviderId, scope: InstallScope) => {
    try {
      setPending({ plan: await api.intentInstallPlan(provider, scope), kind: "enable" });
      setError(null);
    } catch (e) {
      setError(api.errorMessage(e));
    }
  };

  const previewDisable = async (provider: ProviderId, scope: InstallScope) => {
    try {
      setPending({ plan: await api.intentUninstallPlan(provider, scope), kind: "disable" });
      setError(null);
    } catch (e) {
      setError(api.errorMessage(e));
    }
  };

  const confirm = async () => {
    if (!pending) return;
    try {
      const { plan, kind } = pending;
      if (kind === "enable") await onEnable(plan.provider, plan.scope);
      else await onDisable(plan.provider, plan.scope);
      setPending(null);
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
              <button
                disabled={busy}
                onClick={() => void previewDisable(status.provider, status.capture!)}
                title="Remove this agent's intent hooks, previewing the exact files first. The shared commit hooks stay while the other agent still uses them, and the instruction note is left in place."
              >
                Disable…
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

      {plan && plan.writes.length === 0 && (
        <div className="muted" style={{ fontSize: 11, padding: "6px 8px" }}>
          Nothing to remove — this agent's hooks are not installed at that scope.
          <div className="actions">
            <button disabled={busy} onClick={() => setPending(null)}>
              OK
            </button>
          </div>
        </div>
      )}

      {plan && plan.writes.length > 0 && (
        <PlanPreview
          plan={plan}
          busy={busy}
          confirmLabel={pending?.kind === "disable" ? "Disable" : undefined}
          onConfirm={() => void confirm()}
          onCancel={() => setPending(null)}
        />
      )}
    </div>
  );
}

/**
 * Turning the quality gate on. Installed the same way capture is — a previewed
 * plan applied to each agent's hook file — and, like the capture block above,
 * listed per detected agent: the same Stop-hook gate can live in Claude Code's
 * `settings.json` or Codex's `hooks.json`.
 *
 * The gate blocks a turn that ends with a failing `pnpm typecheck` / `cargo fmt`
 * on changed files, an unresolved rejection note, and reminds (without blocking)
 * when source changed but no `.memories/` file did.
 */
function QualityGateSetup({
  providers,
  busy,
}: {
  providers: ProviderStatus[];
  busy: boolean;
}) {
  const detected = providers.filter((p) => p.detected && p.provider !== "user");

  return (
    <div className="intent-panel">
      {detected.length === 0 && (
        <div className="muted" style={{ fontSize: 12 }}>
          No coding agent detected on this machine.
        </div>
      )}

      {detected.map((status) => (
        <QualityGateProvider key={status.provider} provider={status.provider} busy={busy} />
      ))}
    </div>
  );
}

/**
 * The quality gate for one agent. It owns its own installed-scope status, since
 * a gate is a single Stop hook per provider rather than a per-agent matrix.
 */
function QualityGateProvider({
  provider,
  busy,
}: {
  provider: ProviderId;
  busy: boolean;
}) {
  const [scope, setScope] = useState<InstallScope | null>(null);
  // A pending preview is an install or an uninstall; confirm applies its kind.
  const [pending, setPending] = useState<{ plan: InstallPlan; kind: "enable" | "disable" } | null>(
    null,
  );
  const [error, setError] = useState<string | null>(null);
  const [loaded, setLoaded] = useState(false);
  const plan = pending?.plan ?? null;

  useEffect(() => {
    let live = true;
    void api
      .qualityGateStatus(provider)
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
  }, [provider]);

  const preview = async (target: InstallScope) => {
    try {
      setPending({ plan: await api.qualityGateInstallPlan(provider, target), kind: "enable" });
      setError(null);
    } catch (e) {
      setError(api.errorMessage(e));
    }
  };

  const previewDisable = async (target: InstallScope) => {
    try {
      setPending({ plan: await api.qualityGateUninstallPlan(provider, target), kind: "disable" });
      setError(null);
    } catch (e) {
      setError(api.errorMessage(e));
    }
  };

  const confirm = async () => {
    if (!pending) return;
    try {
      const { plan, kind } = pending;
      setScope(
        kind === "enable"
          ? await api.installQualityGate(provider, plan.scope)
          : await api.uninstallQualityGate(provider, plan.scope),
      );
      setPending(null);
      setError(null);
    } catch (e) {
      setError(api.errorMessage(e));
    }
  };

  if (!loaded) return null;

  return (
    <div className="provider">
      <div className="provider-name">
        <strong>Quality gate — {PROVIDER_LABEL[provider]}</strong>
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
          <>
            <button
              disabled={busy}
              onClick={() => void preview(scope)}
              title="Re-write the hook at the same level — useful after an update changed the command"
            >
              Re-apply…
            </button>
            <button
              disabled={busy}
              onClick={() => void previewDisable(scope)}
              title="Remove the quality-gate Stop hook, previewing the exact change first. The intent recorder's own Stop hook is left in place."
            >
              Turn off…
            </button>
          </>
        )}
      </div>

      {error && <div className="error" style={{ fontSize: 11 }}>{error}</div>}

      {plan && plan.writes.length === 0 && (
        <div className="muted" style={{ fontSize: 11, padding: "6px 8px" }}>
          Nothing to remove — the gate is not installed at that scope.
          <div className="actions">
            <button disabled={busy} onClick={() => setPending(null)}>
              OK
            </button>
          </div>
        </div>
      )}

      {plan && plan.writes.length > 0 && (
        <PlanPreview
          plan={plan}
          busy={busy}
          confirmLabel={pending?.kind === "disable" ? "Turn off" : undefined}
          onConfirm={() => void confirm()}
          onCancel={() => setPending(null)}
        />
      )}
    </div>
  );
}

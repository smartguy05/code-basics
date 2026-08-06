import { useState } from "react";
import * as api from "../ipc/api";
import type {
  Confidence,
  GroupKind,
  InstallPlan,
  InstallScope,
  IntentGroup,
  ProviderId,
  ProviderStatus,
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
  formatting: "Formatting",
  newSymbol: "New",
  modifiedSymbol: "Changed",
  other: "Unexplained",
};

/** Only a stated intent is a real reason; the rest are locations. */
const KIND_TITLE: Record<GroupKind, string> = {
  intent: "The agent said this is what it was doing",
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
  busy: boolean;
  onSelect: (group: IntentGroup) => void;
  onStage: (group: IntentGroup) => void;
  onRevert: (group: IntentGroup) => void;
  onEnable: (provider: ProviderId, scope: InstallScope) => Promise<void>;
  onImportHistory: () => Promise<void>;
}

export function IntentPanel({
  groups,
  providers,
  selectedGroup,
  busy,
  onSelect,
  onStage,
  onRevert,
  onEnable,
  onImportHistory,
}: IntentPanelProps) {
  const stated = groups.filter((g) => g.kind === "intent").length;
  const capturing = providers.some((p) => p.capture != null);

  // Open by default until capture is set up. The panel is the only way to turn
  // it on, and a feature nobody can find is a feature that does not exist.
  const [setupOpen, setSetupOpen] = useState(!capturing);

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
          onImportHistory={onImportHistory}
        />
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
          busy={busy}
          onSelect={onSelect}
          onStage={onStage}
          onRevert={onRevert}
        />
      ))}
    </>
  );
}

function GroupCard({
  group,
  selected,
  busy,
  onSelect,
  onStage,
  onRevert,
}: {
  group: IntentGroup;
  selected: boolean;
  busy: boolean;
  onSelect: (group: IntentGroup) => void;
  onStage: (group: IntentGroup) => void;
  onRevert: (group: IntentGroup) => void;
}) {
  const hunks = group.files.reduce((total, file) => total + file.hunks.length, 0);

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
          <div className="paths">{group.files.map((file) => file.path).join(", ")}</div>
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
          </div>
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

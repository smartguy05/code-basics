import { describe, expect, it } from "vitest";
import {
  CLOSED_TASKS_PANEL,
  closeTasksPanel,
  openTasksPanel,
  TASKS_LAYOUT_KEY,
  tasksPanelAfterFeatureChange,
  tasksPanelMounted,
  type TasksPanelState,
} from "./tasksPanelLogic";

describe("TASKS_LAYOUT_KEY", () => {
  it("follows the cb.<thing>.layout convention", () => {
    expect(TASKS_LAYOUT_KEY).toBe("cb.tasks.layout");
  });

  it("does not collide with the other panels' keys", () => {
    expect(TASKS_LAYOUT_KEY).not.toBe("cb.sql.layout");
    expect(TASKS_LAYOUT_KEY).not.toBe("cb.notes.layout");
    expect(TASKS_LAYOUT_KEY).not.toBe("cb.agentPanel.layout");
  });
});

describe("openTasksPanel", () => {
  it("opens a closed panel", () => {
    expect(openTasksPanel(CLOSED_TASKS_PANEL).open).toBe(true);
  });

  it("advances the token even when the panel is already open", () => {
    const first = openTasksPanel(CLOSED_TASKS_PANEL);
    const second = openTasksPanel(first);
    expect(second.open).toBe(true);
    expect(second.restoreToken).not.toBe(first.restoreToken);
  });

  it("advances the token on every request, so a restore is always observable", () => {
    let state: TasksPanelState = CLOSED_TASKS_PANEL;
    const seen = new Set<number>();
    for (let i = 0; i < 5; i += 1) {
      state = openTasksPanel(state);
      seen.add(state.restoreToken);
    }
    expect(seen.size).toBe(5);
  });
});

describe("closeTasksPanel", () => {
  it("closes an open panel", () => {
    expect(closeTasksPanel(openTasksPanel(CLOSED_TASKS_PANEL)).open).toBe(false);
  });

  it("keeps the token, so a later open still differs from every value seen", () => {
    const open = openTasksPanel(CLOSED_TASKS_PANEL);
    const closed = closeTasksPanel(open);
    expect(closed.restoreToken).toBe(open.restoreToken);
    expect(openTasksPanel(closed).restoreToken).not.toBe(open.restoreToken);
  });

  it("returns the same object when nothing was open", () => {
    expect(closeTasksPanel(CLOSED_TASKS_PANEL)).toBe(CLOSED_TASKS_PANEL);
  });
});

describe("tasksPanelAfterFeatureChange", () => {
  it("closes an open panel when the feature is switched off", () => {
    const open = openTasksPanel(CLOSED_TASKS_PANEL);
    expect(tasksPanelAfterFeatureChange(open, false).open).toBe(false);
  });

  it("leaves an open panel alone while the feature is on", () => {
    const open = openTasksPanel(CLOSED_TASKS_PANEL);
    expect(tasksPanelAfterFeatureChange(open, true)).toBe(open);
  });

  it("returns the same object when there is nothing to close", () => {
    expect(tasksPanelAfterFeatureChange(CLOSED_TASKS_PANEL, false)).toBe(CLOSED_TASKS_PANEL);
  });

  it("does not reopen the panel when the feature comes back on", () => {
    const disabled = tasksPanelAfterFeatureChange(openTasksPanel(CLOSED_TASKS_PANEL), false);
    expect(tasksPanelAfterFeatureChange(disabled, true).open).toBe(false);
  });
});

describe("tasksPanelMounted", () => {
  it("mounts only when the panel is open and the feature is on", () => {
    expect(tasksPanelMounted(openTasksPanel(CLOSED_TASKS_PANEL), true)).toBe(true);
  });

  it("does not mount a closed panel", () => {
    expect(tasksPanelMounted(CLOSED_TASKS_PANEL, true)).toBe(false);
  });

  it("unmounts — never merely hides — an open panel whose feature is off", () => {
    expect(tasksPanelMounted(openTasksPanel(CLOSED_TASKS_PANEL), false)).toBe(false);
  });
});

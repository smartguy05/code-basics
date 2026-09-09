import { describe, expect, it } from "vitest";
import {
  CLOSED_SQL_PANEL,
  closeSqlPanel,
  openSqlPanel,
  SQL_LAYOUT_KEY,
  sqlPanelAfterFeatureChange,
  sqlPanelMounted,
  type SqlPanelState,
} from "./sqlPanelLogic";

describe("SQL_LAYOUT_KEY", () => {
  it("follows the cb.<thing>.layout convention", () => {
    expect(SQL_LAYOUT_KEY).toBe("cb.sql.layout");
  });

  it("does not collide with the other panels' keys", () => {
    expect(SQL_LAYOUT_KEY).not.toBe("cb.notes.layout");
    expect(SQL_LAYOUT_KEY).not.toBe("cb.agentPanel.layout");
  });
});

describe("openSqlPanel", () => {
  it("opens a closed panel", () => {
    const next = openSqlPanel(CLOSED_SQL_PANEL);
    expect(next.open).toBe(true);
  });

  it("advances the token even when the panel is already open", () => {
    const first = openSqlPanel(CLOSED_SQL_PANEL);
    const second = openSqlPanel(first);
    expect(second.open).toBe(true);
    expect(second.restoreToken).not.toBe(first.restoreToken);
  });

  it("advances the token on every request, so a restore is always observable", () => {
    let state: SqlPanelState = CLOSED_SQL_PANEL;
    const seen = new Set<number>();
    for (let i = 0; i < 5; i += 1) {
      state = openSqlPanel(state);
      seen.add(state.restoreToken);
    }
    expect(seen.size).toBe(5);
  });
});

describe("closeSqlPanel", () => {
  it("closes an open panel", () => {
    expect(closeSqlPanel(openSqlPanel(CLOSED_SQL_PANEL)).open).toBe(false);
  });

  it("keeps the token, so a later open still differs from every value seen", () => {
    const open = openSqlPanel(CLOSED_SQL_PANEL);
    const closed = closeSqlPanel(open);
    expect(closed.restoreToken).toBe(open.restoreToken);
    expect(openSqlPanel(closed).restoreToken).not.toBe(open.restoreToken);
  });

  it("returns the same object when nothing was open", () => {
    expect(closeSqlPanel(CLOSED_SQL_PANEL)).toBe(CLOSED_SQL_PANEL);
  });
});

describe("sqlPanelAfterFeatureChange", () => {
  it("closes an open panel when the feature is switched off", () => {
    const open = openSqlPanel(CLOSED_SQL_PANEL);
    expect(sqlPanelAfterFeatureChange(open, false).open).toBe(false);
  });

  it("leaves an open panel alone while the feature is on", () => {
    const open = openSqlPanel(CLOSED_SQL_PANEL);
    expect(sqlPanelAfterFeatureChange(open, true)).toBe(open);
  });

  it("returns the same object when there is nothing to close", () => {
    expect(sqlPanelAfterFeatureChange(CLOSED_SQL_PANEL, false)).toBe(CLOSED_SQL_PANEL);
  });

  it("does not reopen the panel when the feature comes back on", () => {
    const disabled = sqlPanelAfterFeatureChange(openSqlPanel(CLOSED_SQL_PANEL), false);
    expect(sqlPanelAfterFeatureChange(disabled, true).open).toBe(false);
  });
});

describe("sqlPanelMounted", () => {
  it("mounts only when the panel is open and the feature is on", () => {
    const open = openSqlPanel(CLOSED_SQL_PANEL);
    expect(sqlPanelMounted(open, true)).toBe(true);
  });

  it("does not mount a closed panel", () => {
    expect(sqlPanelMounted(CLOSED_SQL_PANEL, true)).toBe(false);
  });

  it("unmounts — never merely hides — an open panel whose feature is off", () => {
    const open = openSqlPanel(CLOSED_SQL_PANEL);
    expect(sqlPanelMounted(open, false)).toBe(false);
  });
});

import { describe, expect, it } from "vitest";

import type { McpServerToolsInfo } from "../ipc/types";
import {
  enabledCount,
  nextServerValue,
  serverToggleState,
  toolBlockedByServer,
  toolKey,
} from "./mcpToolsLogic";

function server(states: boolean[]): McpServerToolsInfo {
  return {
    id: "sql",
    label: "SQL",
    tools: states.map((enabled, i) => ({
      name: `sql.tool${i}`,
      description: "",
      enabled,
    })),
  };
}

describe("serverToggleState", () => {
  it("is all when every tool is on", () => {
    expect(serverToggleState(server([true, true]))).toBe("all");
  });
  it("is none when every tool is off", () => {
    expect(serverToggleState(server([false, false]))).toBe("none");
  });
  it("is some when mixed", () => {
    expect(serverToggleState(server([true, false]))).toBe("some");
  });
  it("treats an empty server as all", () => {
    expect(serverToggleState(server([]))).toBe("all");
  });
});

describe("enabledCount", () => {
  it("counts the on tools", () => {
    expect(enabledCount(server([true, false, true]))).toBe(2);
  });
});

describe("nextServerValue", () => {
  it("turns a fully-off server on", () => {
    expect(nextServerValue("none")).toBe(true);
  });
  it("turns a mixed server off", () => {
    expect(nextServerValue("some")).toBe(false);
  });
  it("turns a fully-on server off", () => {
    expect(nextServerValue("all")).toBe(false);
  });
});

describe("toolBlockedByServer", () => {
  it("blocks when the server feature is off", () => {
    expect(toolBlockedByServer(false)).toBe(true);
  });
  it("does not block when on", () => {
    expect(toolBlockedByServer(true)).toBe(false);
  });
  it("does not block while the feature list is still loading", () => {
    expect(toolBlockedByServer(null)).toBe(false);
  });
});

describe("toolKey", () => {
  it("joins server id and tool name", () => {
    expect(toolKey("sql", { name: "sql.query", description: "", enabled: true })).toBe(
      "sql/sql.query",
    );
  });
});

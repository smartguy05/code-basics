import { describe, it, expect } from "vitest";
import { runningConfigIdsOfKind } from "./runControlLogic";
import type { RunConfig } from "../ipc/types";

function cfg(id: string, kind: "app" | "test"): Pick<RunConfig, "id" | "kind"> {
  return { id, kind };
}

const configs = [
  cfg("api", "app"),
  cfg("web", "app"),
  cfg("unit", "test"),
  cfg("integration", "test"),
];

describe("runningConfigIdsOfKind", () => {
  it("returns only the running app configs for kind app", () => {
    const running = ["api", "unit"];
    expect(runningConfigIdsOfKind(configs, running, "app")).toEqual(["api"]);
  });

  it("returns only the running test configs for kind test", () => {
    const running = ["api", "unit", "integration"];
    expect(runningConfigIdsOfKind(configs, running, "test")).toEqual(["unit", "integration"]);
  });

  it("excludes build keys, which never match a config id", () => {
    const running = ["api:build", "api", "web:build"];
    expect(runningConfigIdsOfKind(configs, running, "app")).toEqual(["api"]);
  });

  it("ignores running ids that name no known config", () => {
    const running = ["ghost", "api"];
    expect(runningConfigIdsOfKind(configs, running, "app")).toEqual(["api"]);
  });

  it("returns nothing when none of the running ids are of the kind", () => {
    expect(runningConfigIdsOfKind(configs, ["unit"], "app")).toEqual([]);
    expect(runningConfigIdsOfKind(configs, [], "test")).toEqual([]);
  });
});

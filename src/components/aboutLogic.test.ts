import { describe, expect, it } from "vitest";
import {
  UNKNOWN,
  formatBuildDate,
  platformLine,
  treeNotice,
  treeState,
  versionDrift,
  versionLine,
} from "./aboutLogic";

describe("treeState", () => {
  it("reads a bare sha as a clean tree", () => {
    expect(treeState("20a7dbb")).toBe("clean");
  });

  // The whole point of the -dirty marker: an unmarked sha on a modified tree
  // is a wrong answer about which code is running.
  it("reads the dirty marker the build script appends", () => {
    expect(treeState("20a7dbb-dirty")).toBe("dirty");
  });

  it("keeps unverified distinct from both clean and dirty", () => {
    expect(treeState("20a7dbb-unverified")).toBe("unverified");
  });

  it("reads the unknown sentinel as unknown rather than as a clean sha", () => {
    expect(treeState(UNKNOWN)).toBe("unknown");
  });

  // An empty string cannot mean "clean" — nothing was established at all.
  it("treats a blank sha as unknown, not clean", () => {
    expect(treeState("")).toBe("unknown");
    expect(treeState("   ")).toBe("unknown");
  });

  // "dirty" must be the build script's suffix, not any sha ending in it.
  it("does not read a sha merely containing the word dirty as dirty", () => {
    expect(treeState("dirtyab")).toBe("clean");
  });
});

describe("treeNotice", () => {
  it("says nothing for a clean tree — there is nothing to warn about", () => {
    expect(treeNotice("clean")).toBe(null);
  });

  it("warns that a dirty build does not match its commit", () => {
    const notice = treeNotice("dirty");
    expect(notice).not.toBe(null);
    expect(notice).toMatch(/uncommitted/i);
  });

  // Not knowing is its own answer and must not borrow the dirty wording.
  it("states the unverified case separately from the dirty one", () => {
    expect(treeNotice("unverified")).not.toBe(treeNotice("dirty"));
    expect(treeNotice("unverified")).toMatch(/could not/i);
  });

  it("explains why the commit is unknown rather than leaving it bare", () => {
    expect(treeNotice("unknown")).toMatch(/no git|not a git|without git/i);
  });
});

describe("versionLine", () => {
  it("pairs the version with the commit it was built from", () => {
    expect(versionLine("1.3.0", "20a7dbb")).toBe("1.3.0 (20a7dbb)");
  });

  it("keeps the dirty marker where the reader will see it", () => {
    expect(versionLine("1.3.0", "20a7dbb-dirty")).toBe("1.3.0 (20a7dbb-dirty)");
  });

  // "1.3.0 (unknown)" adds nothing: the commit row already says unknown, and
  // a parenthetical that carries no information reads as a defect.
  it("omits the parenthetical entirely when the commit is unknown", () => {
    expect(versionLine("1.3.0", UNKNOWN)).toBe("1.3.0");
    expect(versionLine("1.3.0", "")).toBe("1.3.0");
  });
});

describe("platformLine", () => {
  it("names the target the binary was compiled for", () => {
    expect(platformLine("windows", "x86_64")).toBe("windows (x86_64)");
  });

  // Half an answer is still an answer; inventing the other half is not.
  it("omits an architecture it was not given rather than guessing one", () => {
    expect(platformLine("linux", "")).toBe("linux");
    expect(platformLine("", "aarch64")).toBe(UNKNOWN);
  });
});

describe("formatBuildDate", () => {
  it("renders epoch seconds as an unambiguous UTC timestamp", () => {
    expect(formatBuildDate("1788466955")).toBe("2026-09-03 20:22:35 UTC");
  });

  it("passes the unknown sentinel straight through", () => {
    expect(formatBuildDate(UNKNOWN)).toBe(UNKNOWN);
  });

  // Anything it cannot parse becomes "unknown" — never a fabricated date, and
  // never the Invalid Date a bare `new Date()` would render.
  it("abstains rather than rendering an invalid date", () => {
    expect(formatBuildDate("")).toBe(UNKNOWN);
    expect(formatBuildDate("not-a-number")).toBe(UNKNOWN);
    expect(formatBuildDate("12e9999")).toBe(UNKNOWN);
    expect(formatBuildDate("-1")).toBe(UNKNOWN);
  });
});

describe("versionDrift", () => {
  it("says nothing while the manifests agree", () => {
    expect(versionDrift("1.3.0", "1.3.0")).toBe(null);
  });

  // package.json / tauri.conf.json hold their own copies of a version the Cargo
  // workspace couples everywhere else, so they can be bumped apart. When they
  // are, the dialog is the one place anybody would notice.
  it("names both versions when they disagree", () => {
    const drift = versionDrift("1.3.0", "1.2.0");
    expect(drift).toMatch(/1\.3\.0/);
    expect(drift).toMatch(/1\.2\.0/);
  });

  // A missing half is not a disagreement — warning on it would fire during the
  // in-flight read, before either value is known.
  it("abstains rather than warning when either version is missing", () => {
    expect(versionDrift("", "1.3.0")).toBe(null);
    expect(versionDrift("1.3.0", "")).toBe(null);
    expect(versionDrift("", "")).toBe(null);
  });
});

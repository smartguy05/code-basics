import { describe, expect, it } from "vitest";
import type { Task } from "../ipc/types";
import {
  canAddTask,
  cleanTaskTitle,
  nextSelectedAfterDelete,
  otherOwner,
  ownerLabel,
  taskPrompt,
  toggledStatus,
  UNTITLED,
} from "./tasksLogic";

function task(over: Partial<Task> = {}): Task {
  return {
    id: "t",
    title: "Title",
    body: "Body",
    status: "open",
    owner: "me",
    createdAtMs: 0,
    updatedAtMs: 0,
    ...over,
  };
}

describe("taskPrompt", () => {
  it("joins the title and body with a blank line", () => {
    expect(taskPrompt(task({ title: "Fix login", body: "The button 500s." }))).toBe(
      "Fix login\n\nThe button 500s.",
    );
  });

  it("is the title alone when the body is blank", () => {
    expect(taskPrompt(task({ title: "Fix login", body: "   " }))).toBe("Fix login");
  });

  it("is the body alone when the title is blank", () => {
    expect(taskPrompt(task({ title: "", body: "Details" }))).toBe("Details");
  });

  it("trims each part so a trailing newline is not submitted as the whole prompt", () => {
    expect(taskPrompt(task({ title: "  A  ", body: "  B\n" }))).toBe("A\n\nB");
  });
});

describe("toggledStatus", () => {
  it("completes an open task", () => {
    expect(toggledStatus("open")).toBe("done");
  });
  it("reopens a done task", () => {
    expect(toggledStatus("done")).toBe("open");
  });
});

describe("otherOwner", () => {
  it("flips me to ai", () => {
    expect(otherOwner("me")).toBe("ai");
  });
  it("flips ai to me", () => {
    expect(otherOwner("ai")).toBe("me");
  });
});

describe("ownerLabel", () => {
  it("names the owners", () => {
    expect(ownerLabel("ai")).toBe("AI");
    expect(ownerLabel("me")).toBe("Me");
  });
});

describe("canAddTask", () => {
  it("allows a non-blank title", () => {
    expect(canAddTask("Do the thing")).toBe(true);
  });
  it("refuses a blank or whitespace-only title", () => {
    expect(canAddTask("")).toBe(false);
    expect(canAddTask("   ")).toBe(false);
  });
});

describe("cleanTaskTitle", () => {
  it("trims a title", () => {
    expect(cleanTaskTitle("  hi  ")).toBe("hi");
  });
  it("falls back to Untitled for a blank rename", () => {
    expect(cleanTaskTitle("   ")).toBe(UNTITLED);
  });
});

describe("nextSelectedAfterDelete", () => {
  const tasks = [task({ id: "a" }), task({ id: "b" }), task({ id: "c" })];

  it("keeps the selection when a different task is deleted", () => {
    expect(nextSelectedAfterDelete(tasks, "a", "b")).toBe("b");
  });

  it("selects the task that slid into the deleted index", () => {
    expect(nextSelectedAfterDelete(tasks, "b", "b")).toBe("c");
  });

  it("selects the new last task when the final one was deleted", () => {
    expect(nextSelectedAfterDelete(tasks, "c", "c")).toBe("b");
  });

  it("has no selection once the list is empty", () => {
    expect(nextSelectedAfterDelete([task({ id: "a" })], "a", "a")).toBeUndefined();
  });
});

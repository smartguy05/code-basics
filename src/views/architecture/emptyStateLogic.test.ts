import { describe, expect, it } from "vitest";
import { emptyGraphKind } from "./emptyStateLogic";

describe("emptyGraphKind", () => {
  it("is null while there is something to draw", () => {
    expect(emptyGraphKind(3, [])).toBeNull();
    expect(emptyGraphKind(1, ["something was refused"])).toBeNull();
  });

  it("is a plain absence when nothing was found and nothing was refused", () => {
    expect(emptyGraphKind(0, [])).toBe("nothingFound");
  });

  it("is a refusal when the deriver found things and drew none of them", () => {
    // The shape a synthetic workspace produces: `Api.csproj` plus a
    // `builder.Services.AddHttpClient("orders")` with no literal base address.
    //
    //   COMPONENT nodes=0 edges=0 warnings=1
    //     C-WARN: Api: the AddHttpClient registration at Api/Program.cs:2 was
    //             not attributed to a service because no literal base address
    //             is written there
    //
    // Saying "no components were found" there is false: something was found,
    // and refused, and the reason is the only information the view has.
    expect(
      emptyGraphKind(0, [
        "Api: the AddHttpClient registration at Api/Program.cs:2 was not attributed to a service",
      ]),
    ).toBe("allRefused");
  });

  it("does not count a warning nobody can read", () => {
    // Same rule `warningSummary` applies: a blank string cannot be shown, so
    // promising a reason and then listing nothing would be the worse answer.
    expect(emptyGraphKind(0, ["", "   "])).toBe("nothingFound");
  });

  it("takes a graph that was never loaded as nothing to say", () => {
    expect(emptyGraphKind(null, ["a warning"])).toBeNull();
  });
});

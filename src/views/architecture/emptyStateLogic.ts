/**
 * Which of the two empty answers a derived graph is giving.
 *
 * An empty graph is a real answer — `arch_component_graph` returns nothing
 * when no HIGH-strength signal exists and never falls back to the project map
 * to avoid it — but it is **two** answers wearing one shape, and they are not
 * the same news:
 *
 * * *Nothing was found.* A repository of class libraries and tools genuinely
 *   has no components. The prose that says so is correct and complete.
 * * *Everything found was refused.* The signal gate discarded every candidate
 *   and counted each refusal into `ArchGraph.warnings`. Executed on a
 *   synthetic workspace — an `Api.csproj` plus a `Program.cs` holding
 *   `builder.Services.AddHttpClient("orders")`:
 *
 *       COMPONENT nodes=0 edges=0 warnings=1
 *         C-WARN: Api: the AddHttpClient registration at Api/Program.cs:2 was
 *                 not attributed to a service because no literal base address
 *                 is written there
 *
 *   Telling that user "no components were found in this workspace" is simply
 *   false, and it throws away the one thing the derivation had to say. This is
 *   the case where the warnings are not commentary on a picture — they *are*
 *   the answer, and the picture that would normally carry them is not drawn.
 *
 * Blank warnings do not count, for the same reason `warningSummary` does not
 * count them: a warning nobody can read cannot be shown, so promising a reason
 * and then listing nothing would be worse than the plain absence.
 */
export type EmptyGraphKind = "nothingFound" | "allRefused";

/**
 * Classify an empty graph, or `null` when there is a picture to draw (or no
 * graph at all — a stored diagram has no node count, and nothing here applies
 * to it).
 */
export function emptyGraphKind(
  nodeCount: number | null,
  warnings: readonly string[],
): EmptyGraphKind | null {
  if (nodeCount === null || nodeCount > 0) return null;
  return warnings.some((warning) => warning.trim() !== "") ? "allRefused" : "nothingFound";
}

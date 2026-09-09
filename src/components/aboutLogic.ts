/**
 * The string composition behind the About dialog.
 *
 * It lives here rather than inside `AboutDialog.tsx` for the reason every
 * `*Logic.ts` module does: a component in this codebase is an untested
 * rendering shell, and these are decisions — specifically, decisions about what
 * the app is allowed to *claim* about the build it is running. A version report
 * that overstates its certainty is the failure being designed against, so every
 * function here abstains where the backend abstained, and none of them invents
 * a value to fill a gap.
 */

/**
 * The literal both `build.rs` and `commands/about.rs` emit for a fact they
 * could not establish, and the literal shown to the user.
 *
 * Kept as a constant rather than inlined because it is a *wire* value: it must
 * stay the same word on both sides, and it is deliberately rendered verbatim —
 * a blank field would leave the reader to assume, which is exactly what naming
 * the gap prevents.
 */
export const UNKNOWN = "unknown";

/**
 * What the sha suffix says about the working tree when the build was made.
 *
 * Four answers, not two, because they are four different facts and collapsing
 * any pair of them would report something untrue: `unverified` is *we know the
 * commit but not whether it was modified*, and `unknown` is *we do not even
 * know the commit* — neither may masquerade as `clean`.
 */
export type TreeState = "clean" | "dirty" | "unverified" | "unknown";

/** Classify the `gitSha` the backend reported. */
export function treeState(gitSha: string): TreeState {
  const sha = gitSha.trim();
  // A blank sha establishes nothing, so it cannot mean "clean". The backend
  // should never send one, and reading it as a pristine build would be the
  // worst possible reading of an empty field.
  if (sha === "" || sha === UNKNOWN) return "unknown";
  if (sha.endsWith("-dirty")) return "dirty";
  if (sha.endsWith("-unverified")) return "unverified";
  return "clean";
}

/**
 * The line to show beneath the commit, or `null` when there is nothing worth
 * saying. A clean build needs no notice — the commit alone is the whole truth —
 * and adding one to every build would train the reader to ignore it.
 */
export function treeNotice(state: TreeState): string | null {
  switch (state) {
    case "clean":
      return null;
    case "dirty":
      return "Built from a working tree with uncommitted changes — this build is not exactly that commit.";
    case "unverified":
      return "Could not check whether the working tree was modified, so this build may not be exactly that commit.";
    case "unknown":
      return "Built without git information available (no git repository, or no git on PATH).";
  }
}

/**
 * The headline: the version, and the commit it came from.
 *
 * The commit is folded in here — dirty marker included — because this is the
 * one line someone pastes into a bug report, and a marker sitting three rows
 * lower is a marker that gets left behind.
 */
export function versionLine(appVersion: string, gitSha: string): string {
  // "1.3.0 (unknown)" carries no information the commit row does not already
  // state, and a parenthetical that says nothing reads as a defect.
  if (treeState(gitSha) === "unknown") return appVersion;
  return `${appVersion} (${gitSha.trim()})`;
}

/**
 * The target this binary was compiled for.
 *
 * Half an answer is still worth showing; the missing half is left out rather
 * than filled in, since `std::env::consts` never guesses and neither may this.
 */
export function platformLine(os: string, arch: string): string {
  const platform = os.trim();
  const machine = arch.trim();
  if (platform === "") return UNKNOWN;
  return machine === "" ? platform : `${platform} (${machine})`;
}

/**
 * Render the build stamp — UNIX epoch seconds, as `build.rs` documents — as an
 * unambiguous UTC timestamp.
 *
 * UTC and a fixed layout rather than the viewer's locale: this value exists to
 * be quoted in a bug report and compared against someone else's, and a
 * locale-formatted local time makes two reports of the same build look
 * different.
 *
 * Anything unparseable becomes {@link UNKNOWN}. That guard is the point of the
 * function — `new Date(NaN).toISOString()` throws, and a bare `new Date(raw)`
 * would happily render `Invalid Date` as though it were a reading.
 */
export function formatBuildDate(raw: string): string {
  const trimmed = raw.trim();
  if (trimmed === "" || trimmed === UNKNOWN) return UNKNOWN;
  // A strict digits-only test, not `Number()`: that accepts `" 1e9 "`, hex and
  // signs, and a negative or fractional epoch here would mean the stamp is not
  // what `build.rs` promised rather than that the build predates 1970.
  if (!/^\d+$/.test(trimmed)) return UNKNOWN;
  const seconds = Number(trimmed);
  if (!Number.isSafeInteger(seconds)) return UNKNOWN;
  const at = new Date(seconds * 1000);
  if (Number.isNaN(at.getTime())) return UNKNOWN;
  // `2026-09-03T20:22:35.000Z` → `2026-09-03 20:22:35 UTC`: the same instant,
  // spelled so a non-engineer reading a bug report can also see what it says.
  return `${at.toISOString().slice(0, 10)} ${at.toISOString().slice(11, 19)} UTC`;
}

/**
 * Non-null when the two version sources disagree, naming both.
 *
 * There are two copies of this number that the Cargo workspace does *not*
 * couple — `package.json` and `tauri.conf.json` (which is what `getVersion()`
 * reads) — against `cb-app`'s own crate version. A release that bumps one and
 * forgets the other produces a build whose reported version depends on which
 * side you ask, and this dialog is the only place anybody would ever see both.
 *
 * A missing value is not a disagreement: while the reads are still in flight,
 * or if one side ever answers with nothing, staying quiet is the only honest
 * option — a false drift warning would send someone chasing a release bug that
 * does not exist.
 */
export function versionDrift(appVersion: string, crateVersion: string): string | null {
  const app = appVersion.trim();
  const crate = crateVersion.trim();
  if (app === "" || crate === "") return null;
  if (app === crate) return null;
  return `Version mismatch: the bundle reports ${app} and the cb-app crate reports ${crate}.`;
}

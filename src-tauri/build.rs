//! Build script. Runs `tauri_build`, and stamps in the build provenance the
//! About dialog reports.
//!
//! **Nothing here may fail the build.** A missing `.git`, a build machine with
//! no `git` on `PATH`, a source tarball, a clock before the epoch — all are
//! ordinary situations, and none of them is a reason to refuse to compile an
//! application. So every path ends in a value, and the value for *we could not
//! establish this* is the literal `unknown`, which the dialog shows verbatim.
//! Abstaining out loud is the rule this repository holds to everywhere; a blank
//! field, or worse a fabricated one, is the failure mode being avoided.

use std::path::Path;
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

/// What every field reports when it could not be established. Must match
/// `UNKNOWN` in `src/commands/about.rs`, which is the fallback used when this
/// script did not run at all.
const UNKNOWN: &str = "unknown";

fn main() {
    watch_git_refs();

    println!("cargo::rustc-env=CB_GIT_SHA={}", git_sha());
    println!("cargo::rustc-env=CB_BUILD_DATE={}", build_date());

    tauri_build::build()
}

/// Ask cargo to re-run this script whenever the commit could have moved.
///
/// `.git/HEAD` alone is **not** enough, and assuming it was is the mistake this
/// function exists to correct: committing on a branch rewrites
/// `.git/refs/heads/<branch>` (or `packed-refs`) and leaves `HEAD` — a file
/// containing the text `ref: refs/heads/<branch>` — untouched. Measured on this
/// repository, `.git/HEAD` was two days older than the commit it pointed at. So
/// a build, a commit, and a rebuild reported the *previous* commit, with
/// whatever dirty marker had been true at the time.
///
/// So: `HEAD` (checkouts and detached moves), the ref `HEAD` names (commits,
/// resets, amends, and anything else that rewrites the branch), `packed-refs`
/// (the same ref after a `git gc`), and `.git/index` (staging, which is what
/// most `-dirty` transitions actually are).
///
/// **What this deliberately does not watch is the working tree.** Watching it
/// would relink the entire shell on every keystroke, which is not a trade worth
/// making for a provenance string. The consequence is real and must not be
/// glossed: an *unstaged* edit made after a build does not re-run this script,
/// so the stamp describes the tree as of the last time it ran, not necessarily
/// this instant. That is why the dialog shows the build date beside the commit
/// — the pair is checkable — and why a reader is never asked to infer freshness
/// from the sha alone.
fn watch_git_refs() {
    println!("cargo::rerun-if-changed=../.git/HEAD");
    println!("cargo::rerun-if-changed=../.git/index");
    println!("cargo::rerun-if-changed=../.git/packed-refs");

    // Resolve `ref: refs/heads/x` to the loose ref file, so a commit on this
    // branch re-runs us. A detached HEAD holds a raw sha and names no ref;
    // there is then nothing further to watch, and `HEAD` itself already covers
    // it. Anything unreadable or unexpected is simply not watched — this is a
    // best-effort watch list, and no shape of `.git` may fail the build.
    let Ok(head) = std::fs::read_to_string("../.git/HEAD") else {
        return;
    };
    if let Some(reference) = head.trim().strip_prefix("ref:") {
        let reference = reference.trim();
        // Refuse anything that is not a plain ref path. A `..` here would point
        // the watch outside the repository, and a blank one would watch the
        // whole directory.
        if !reference.is_empty() && !reference.contains("..") {
            println!("cargo::rerun-if-changed=../.git/{reference}");
        }
    }
}

/// The short commit this build came from, marked when the tree was not clean.
///
/// The marker carries the weight here. A sha alone is read as *this is exactly
/// that commit*, so emitting a bare sha from a modified tree is not a missing
/// detail — it is a **wrong** statement about which code is running, and this
/// codebase treats a wrong answer as much worse than no answer. Hence three
/// outcomes and not two: clean, `-dirty`, and `-unverified` for the case where
/// the commit is known but its cleanliness is not.
fn git_sha() -> String {
    // No repository at all (a tarball, or a vendored source drop) is an
    // ordinary build, and probing git for it would only produce a confusing
    // error on stderr during a perfectly good compile.
    if !Path::new("../.git").exists() {
        return UNKNOWN.to_string();
    }

    let Some(sha) = git(&["rev-parse", "--short", "HEAD"]) else {
        return UNKNOWN.to_string();
    };
    if sha.is_empty() {
        return UNKNOWN.to_string();
    }

    match git(&["status", "--porcelain"]) {
        Some(status) if !status.is_empty() => format!("{sha}-dirty"),
        Some(_) => sha,
        // `rev-parse` answered, so git exists and the repository is readable —
        // this is close to impossible. It is still not licence to report the
        // sha bare, which would claim a clean tree nobody verified.
        None => format!("{sha}-unverified"),
    }
}

/// This build's instant, as UNIX epoch **seconds** in decimal.
///
/// Not a formatted date, deliberately. Rendering an instant as a calendar date
/// is real date arithmetic, and a build script is the one place in this tree no
/// test can reach — so the formatting is `aboutLogic.formatBuildDate`, which a
/// test pins, and this side stays too simple to be wrong.
fn build_date() -> String {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|since| since.as_secs().to_string())
        .unwrap_or_else(|_| UNKNOWN.to_string())
}

/// Run `git` and return its trimmed stdout, or `None` for anything that is not
/// a clean success — git absent from `PATH`, a non-zero exit, non-UTF-8 output.
/// The caller decides what not knowing means; this never guesses on its behalf.
fn git(args: &[&str]) -> Option<String> {
    let output = Command::new("git").args(args).output().ok()?;
    if !output.status.success() {
        return None;
    }
    Some(String::from_utf8(output.stdout).ok()?.trim().to_string())
}

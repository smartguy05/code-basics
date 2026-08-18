//! The `record-intent` mode, which is what the installed hooks actually run.
//!
//! An agent hook runs this executable rather than a shipped script: there is
//! then no second artifact to keep in step with the record format, and no
//! interpreter to depend on being present.
//!
//! Everything here is shaped by running inside somebody else's tool:
//!
//! * **It must never fail loudly.** A hook that writes to stderr or exits
//!   non-zero interrupts the agent mid-task over a feature that is only meant
//!   to be taking notes. Every *failure* path returns success.
//! * **It must be quick.** `PostToolUse` fires after every single edit.
//! * **It must usually do nothing.** A user-level hook fires for every
//!   repository on the machine; only the ones that opted in are recorded.
//!
//! # The one deliberate interruption
//!
//! There is exactly one case where this does interrupt the agent, and it is
//! not a failure: a turn that edited files and ended without saying why is
//! asked for an `Intent:` line, by exiting 2 with the request on stderr, which
//! is how a Claude Code `Stop` hook refuses a stop and puts text in front of
//! the model.
//!
//! Every guard on that lives in [`cb_core::intents::hook::ask_for_intent`] —
//! including the one that matters most, which is that a given turn is only ever
//! asked once, so a session can always end. Nothing about it is decided here;
//! this file only turns the answer into an exit code.

use std::io::Read;
use std::path::Path;

use cb_core::git::{why, Repo};
use cb_core::intents::hook::{self, HookEvent};

/// Exit code that makes a Claude Code `Stop` hook block the stop and show the
/// hook's stderr to the model.
const BLOCK_STOP: i32 = 2;

/// Did the command line ask for recording rather than for the application?
pub fn is_record_invocation() -> bool {
    hook::is_record_invocation(&std::env::args().collect::<Vec<_>>())
}

/// Read a hook payload from stdin and record it.
///
/// Reports success for every failure, and for every case where there is
/// nothing to say. The single exception is the intent request: see the module
/// note.
pub fn run() {
    match record() {
        Ok(Some(request)) => {
            // stderr plus exit 2 is the Stop-hook contract for "do not stop,
            // and here is why" — the model reads this text.
            eprintln!("{request}");
            std::process::exit(BLOCK_STOP);
        }
        Ok(None) => {}
        Err(error) => {
            // Diagnosable when someone goes looking, invisible during normal use.
            if std::env::var_os("CODE_BASICS_DEBUG_HOOKS").is_some() {
                eprintln!("code-basics: could not record intent: {error:#}");
            }
        }
    }
}

/// Returns the request to put in front of the agent, when there is one.
fn record() -> anyhow::Result<Option<String>> {
    let args: Vec<String> = std::env::args().collect();

    let Some(invocation) = hook::parse_recorder_args(&args) else {
        // Some other event fired. Nothing to do, and not a problem.
        return Ok(None);
    };

    // A git post-commit hook carries no stdin payload and always names its
    // workspace: persist the durable-why note for the new commit and stop.
    if invocation.event == HookEvent::PostCommit {
        record_why_for_head(invocation.workspace.as_deref())?;
        return Ok(None);
    }

    let mut payload = String::new();
    std::io::stdin().read_to_string(&mut payload)?;
    let payload: serde_json::Value = serde_json::from_str(&payload)?;

    // A user-scope hook names no workspace, so the payload's `cwd` — or the
    // first enabled directory above it — is what decides where this lands.
    let explicit = invocation.workspace.as_deref().map(Path::new);
    let cwd = payload
        .get("cwd")
        .and_then(serde_json::Value::as_str)
        .map(Path::new);

    let Some(root) = hook::resolve_enabled_root(explicit, cwd) else {
        return Ok(None);
    };

    // The directory is created when the user enables capture, so its absence
    // means this repository never asked to be recorded.
    if !hook::is_enabled(&root) {
        return Ok(None);
    }

    hook::ingest(&root, invocation.provider, invocation.event, &payload)?;

    // Deliberately after `ingest`: the label this turn *did* produce is written
    // first, so a turn that gets asked and then complies has both, and one that
    // never complies still keeps whatever it had.
    Ok(hook::ask_for_intent(
        &root,
        invocation.provider,
        invocation.event,
        &payload,
    ))
}

/// Persist the durable-why note for the workspace's HEAD commit.
///
/// Called from the `post-commit` hook. Silent for a workspace that never
/// enabled capture, and — like everything else here — never fails loudly.
fn record_why_for_head(workspace: Option<&str>) -> anyhow::Result<()> {
    let Some(root) = workspace.map(Path::new) else {
        return Ok(());
    };
    if !hook::is_enabled(root) {
        return Ok(());
    }

    let repo = Repo::open(root)?;
    let head = repo.history(1)?;
    let Some(commit) = head.first() else {
        return Ok(());
    };
    why::record_note(&repo, root, &commit.id)
}

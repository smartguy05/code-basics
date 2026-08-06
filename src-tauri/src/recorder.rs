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
//!   to be taking notes. Every failure path returns success.
//! * **It must be quick.** `PostToolUse` fires after every single edit.
//! * **It must usually do nothing.** A user-level hook fires for every
//!   repository on the machine; only the ones that opted in are recorded.

use std::io::Read;

use cb_core::intents::hook::{self, HookEvent};
use cb_core::intents::ProviderId;

/// The flag that identifies our own hook entries in a shared config file.
const MARKER: &str = "--code-basics-intent";

/// Did the command line ask for recording rather than for the application?
pub fn is_record_invocation() -> bool {
    std::env::args().any(|arg| arg == "record-intent" || arg == MARKER)
}

/// Read a hook payload from stdin and record it.
///
/// Always reports success: see the module note.
pub fn run() {
    if let Err(error) = record() {
        // Diagnosable when someone goes looking, invisible during normal use.
        if std::env::var_os("CODE_BASICS_DEBUG_HOOKS").is_some() {
            eprintln!("code-basics: could not record intent: {error:#}");
        }
    }
}

fn record() -> anyhow::Result<()> {
    let args: Vec<String> = std::env::args().collect();

    let Some(event) = flag(&args, "--event").and_then(|e| HookEvent::parse(&e)) else {
        // Some other event fired. Nothing to do, and not a problem.
        return Ok(());
    };
    let provider = match flag(&args, "--provider").as_deref() {
        Some("codex") => ProviderId::Codex,
        _ => ProviderId::ClaudeCode,
    };

    let mut payload = String::new();
    std::io::stdin().read_to_string(&mut payload)?;
    let payload: serde_json::Value = serde_json::from_str(&payload)?;

    let Some(root) = hook::resolve_root(flag(&args, "--workspace").as_deref(), &payload) else {
        return Ok(());
    };

    // The directory is created when the user enables capture, so its absence
    // means this repository never asked to be recorded.
    if !hook::is_enabled(&root) {
        return Ok(());
    }

    hook::ingest(&root, provider, event, &payload)?;
    Ok(())
}

/// Read `--name value` from the command line.
fn flag(args: &[String], name: &str) -> Option<String> {
    let position = args.iter().position(|a| a == name)?;
    args.get(position + 1).cloned()
}

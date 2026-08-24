//! The pure decision seam for serverful HTTP replay.
//!
//! The command in `src-tauri` does the untestable I/O — bring a server up on
//! each side, poll readiness, fire requests, tear it down — but every *decision*
//! around that I/O lives here so it can be tested headlessly:
//!
//! * [`plan_replay`] flattens parsed `.http` scenarios into an ordered request
//!   list and picks the readiness probe.
//! * [`choose_launch_config`] decides which `App` configuration to bring up.
//! * [`pair_and_diff`] takes what each side actually produced and turns it into
//!   [`HttpDelta`] values — abstaining, never guessing, exactly as the rest of
//!   the behavioral stack does.
//!
//! # A wrong delta is worse than no delta
//!
//! If either side never became ready the two runs are simply *not comparable*,
//! so [`pair_and_diff`] emits warnings and **no** deltas rather than a fabricated
//! difference. A request that errored or is missing on one side is likewise a
//! warning, not a delta. Only two `Ok` responses that genuinely differ become a
//! delta.

use std::collections::BTreeMap;

use super::http::{diff_http, RecordedResponse};
use super::httpfile::{HttpRequestSpec, HttpScenario, Readiness};
use super::HttpDelta;
use crate::model::{RunConfig, RunKind};

/// What one side of the comparison actually produced.
///
/// `ready` is whether that side's server ever satisfied the readiness probe;
/// `responses` maps a request key to the response recorded for it (or the error
/// that request hit). Keys match the `request_key` half of the list
/// [`plan_replay`] built.
pub struct SideResult {
    pub ready: Result<(), String>,
    pub responses: BTreeMap<String, Result<RecordedResponse, String>>,
}

impl SideResult {
    /// A side that never got off the ground — a build failure, a missing config
    /// — carrying only the reason. No responses.
    pub fn unready(reason: String) -> Self {
        SideResult {
            ready: Err(reason),
            responses: BTreeMap::new(),
        }
    }
}

/// The ordered work HTTP replay has to do, derived purely from parsed scenarios.
pub struct ReplayPlan {
    /// `(request_key, display_name)` in order — the key pairs the two sides,
    /// the display name titles any resulting delta or warning.
    pub keys: Vec<(String, String)>,
    /// `(request_key, spec)` in the same order — what to actually send.
    pub requests: Vec<(String, HttpRequestSpec)>,
    /// The readiness probe to gate on: the first scenario that declares one.
    pub readiness: Option<Readiness>,
}

/// Flatten parsed scenarios into an ordered replay plan.
///
/// The request key is `"{scenario.path}#{request.name}"` so two scenarios can
/// carry a request of the same name without colliding. Readiness is the first
/// `@readiness` any scenario declares; a plan with `readiness == None` cannot be
/// safely replayed and the caller abstains.
pub fn plan_replay(scenarios: &[HttpScenario]) -> ReplayPlan {
    let mut keys = Vec::new();
    let mut requests = Vec::new();
    // Two requests with the same `@name` in one file would collide on the key
    // and — since responses are stored in a map keyed by it — silently drop one.
    // Disambiguate any repeat with an occurrence suffix so both are replayed and
    // diffed; the first occurrence keeps the plain key.
    let mut seen: std::collections::HashMap<String, usize> = std::collections::HashMap::new();
    for s in scenarios {
        for req in &s.requests {
            let base_key = format!("{}#{}", s.path, req.name);
            let count = seen.entry(base_key.clone()).or_insert(0);
            let key = if *count == 0 {
                base_key.clone()
            } else {
                format!("{base_key}#{count}")
            };
            *count += 1;
            let display = if s.path.is_empty() {
                req.name.clone()
            } else {
                base_key
            };
            keys.push((key.clone(), display));
            requests.push((key, req.clone()));
        }
    }
    let readiness = scenarios.iter().find_map(|s| s.readiness.clone());
    ReplayPlan {
        keys,
        requests,
        readiness,
    }
}

/// Which application configuration to launch for the replay.
pub enum LaunchChoice {
    /// Bring up the `App` config with this id.
    Use(String),
    /// Do not replay, for this reason.
    Abstain(String),
}

/// Decide the launch config: the passed config if it is an `App`, else the sole
/// `App` in the workspace, else abstain (zero or ambiguous).
///
/// A serverful replay needs exactly one server. If the config the user picked is
/// already an application launch, use it. Otherwise fall back to the workspace's
/// only `App` config — but never *guess* between several, and never invent one
/// that is not there.
pub fn choose_launch_config(passed: &RunConfig, all: &[RunConfig]) -> LaunchChoice {
    if matches!(passed.kind, RunKind::App) {
        return LaunchChoice::Use(passed.id.clone());
    }
    let apps: Vec<&RunConfig> = all
        .iter()
        .filter(|c| matches!(c.kind, RunKind::App))
        .collect();
    match apps.as_slice() {
        [] => LaunchChoice::Abstain(format!(
            "HTTP replay skipped: `{}` is not an application launch and the workspace declares no \
             App configuration to replay against",
            passed.name
        )),
        [only] => LaunchChoice::Use(only.id.clone()),
        many => LaunchChoice::Abstain(format!(
            "HTTP replay skipped: `{}` is not an application launch and {} App configurations exist, \
             so which server to replay against is ambiguous",
            passed.name,
            many.len()
        )),
    }
}

/// Pair each side's responses and turn genuine differences into deltas.
///
/// The readiness gate comes first: if *either* side never became ready the runs
/// are not comparable, so the result is the readiness warning(s) and **no**
/// deltas. Otherwise, for each request key: two `Ok` responses are diffed and
/// kept only if they actually differ ([`HttpDelta::is_change`]); a response
/// missing or errored on either side is a warning naming the request, never a
/// delta.
pub fn pair_and_diff(
    keys: &[(String, String)],
    base: &SideResult,
    work: &SideResult,
    ignore: &[&str],
) -> (Vec<HttpDelta>, Vec<String>) {
    let mut warnings = Vec::new();

    let mut unready = false;
    if let Err(e) = &base.ready {
        warnings.push(format!("baseline server not ready: {e}"));
        unready = true;
    }
    if let Err(e) = &work.ready {
        warnings.push(format!("working-tree server not ready: {e}"));
        unready = true;
    }
    if unready {
        // The two sides are not comparable; refuse rather than fabricate.
        return (Vec::new(), warnings);
    }

    let mut deltas = Vec::new();
    for (key, name) in keys {
        match (base.responses.get(key), work.responses.get(key)) {
            (Some(Ok(b)), Some(Ok(w))) => {
                let d = diff_http(name, b, w, ignore);
                if d.is_change() {
                    deltas.push(d);
                }
            }
            (b, w) => {
                let mut parts = Vec::new();
                match b {
                    None => parts.push("no baseline response".to_string()),
                    Some(Err(e)) => parts.push(format!("baseline: {e}")),
                    Some(Ok(_)) => {}
                }
                match w {
                    None => parts.push("no working-tree response".to_string()),
                    Some(Err(e)) => parts.push(format!("working tree: {e}")),
                    Some(Ok(_)) => {}
                }
                warnings.push(format!(
                    "request `{name}` not comparable: {}",
                    parts.join("; ")
                ));
            }
        }
    }
    (deltas, warnings)
}

#[cfg(test)]
#[path = "scenario_tests.rs"]
mod tests;

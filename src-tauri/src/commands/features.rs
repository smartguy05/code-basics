//! Optional-feature commands.
//!
//! The bridge and nothing else: which features exist, what they default to, how
//! a corrupt store is tolerated and how an installer seed is adopted all live in
//! [`cb_core::features`]. Like [`crate::commands::notes`], these take no
//! `AppState` — the store is user-global, not per-workspace, so there is no
//! workspace to resolve.

use cb_core::features::{self, FeatureId, FeatureInfo};

/// Every known feature with its current state.
///
/// This is also the launch-time seed point: the first call adopts an installer's
/// `features.json` if one is there and the user has no store yet. Doing it here
/// rather than in a `setup` hook keeps it on the one path that actually needs the
/// answer, and [`features::ensure_seeded`] is idempotent, so a second call is a
/// plain read.
#[tauri::command]
pub async fn list_features() -> Result<Vec<FeatureInfo>, String> {
    Ok(current()?.list())
}

/// Turn one feature on or off, and report the resulting set.
///
/// Returns the whole list rather than nothing so the caller re-renders from what
/// was actually persisted, instead of from what it hoped it wrote.
#[tauri::command]
pub async fn set_feature(id: String, enabled: bool) -> Result<Vec<FeatureInfo>, String> {
    let feature = FeatureId::from_id(&id)?;
    let mut file = current()?;
    file.set(feature, enabled);
    features::save(&features::features_path(), &file).map_err(|e| format!("{e:#}"))?;
    Ok(file.list())
}

/// The features for this launch, seeding from the installer on first run.
fn current() -> Result<features::FeaturesFile, String> {
    let seed = seed_path();
    features::ensure_seeded(&features::features_path(), seed.as_deref())
        .map_err(|e| format!("{e:#}"))
}

/// Where this platform's installer would have left a seed, if it can leave one.
///
/// Extracted so the fallible `current_exe` step is one place: a build with no
/// resolvable executable path (which does happen — a deleted or replaced binary)
/// has no seed rather than failing the call, since a seed is an optional
/// convenience and the defaults are a complete answer without it.
fn seed_path() -> Option<std::path::PathBuf> {
    let exe = std::env::current_exe().ok()?;
    let dir = exe.parent()?;
    features::seed_path(dir)
}

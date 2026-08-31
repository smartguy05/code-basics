//! Reading and writing `code-basics/features.json`, and the installer seed.
//!
//! The filesystem seam for [`super`]. Both halves of the seed story live here: the
//! per-platform location an installer drops its seed at ([`seed_path_for`]) and
//! the rule for folding that seed into a user store ([`merge_seed`]).

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};

use super::FeaturesFile;

/// The file name inside `code-basics/`.
pub const FEATURES_FILE: &str = "features.json";

/// The file name an installer writes its seed to.
pub const SEED_FILE: &str = "features.json";

/// Where the features file lives: `<config>/code-basics/features.json`.
///
/// `CB_FEATURES_PATH` overrides the whole path, matching `CB_NOTES_PATH`.
/// Otherwise the base is `%APPDATA%`, then `$XDG_CONFIG_HOME`, then `~/.config`,
/// then the current directory — the same resolution order as [`crate::notes`].
pub fn features_path() -> PathBuf {
    if let Some(path) = std::env::var_os("CB_FEATURES_PATH") {
        return PathBuf::from(path);
    }

    let base = std::env::var_os("APPDATA")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("XDG_CONFIG_HOME").map(PathBuf::from))
        .or_else(|| home_dir().map(|h| h.join(".config")))
        .unwrap_or_else(|| PathBuf::from("."));

    base.join("code-basics").join(FEATURES_FILE)
}

fn home_dir() -> Option<PathBuf> {
    std::env::var_os("USERPROFILE")
        .or_else(|| std::env::var_os("HOME"))
        .map(PathBuf::from)
        .filter(|p| !p.as_os_str().is_empty())
}

/// The platform whose seed convention applies.
///
/// An argument rather than a `cfg!`, so the mapping is provable on any host —
/// this is exactly the kind of thing that is wrong on the platform nobody runs
/// the tests on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Platform {
    Windows,
    Linux,
    Other,
}

impl Platform {
    /// The platform this build is running on.
    pub fn current() -> Self {
        if cfg!(windows) {
            Platform::Windows
        } else if cfg!(target_os = "linux") {
            Platform::Linux
        } else {
            Platform::Other
        }
    }
}

/// Where an installer left its seed file, if that platform's installer can leave
/// one at all.
///
/// - **Windows** — beside the executable, because the NSIS page writes
///   `$INSTDIR\features.json` and `$INSTDIR` is where the exe lands.
/// - **Linux** — `/usr/share/code-basics/features.json`, shipped by the `.deb`.
///   Not beside the exe: `/usr/bin` is not a place to write app data, and an
///   AppImage's mount point is read-only and different on every launch.
/// - **Anything else** — `None`. macOS has no feature-selection installer here,
///   and inventing a path it would never contain buys nothing.
pub fn seed_path_for(exe_dir: &Path, platform: Platform) -> Option<PathBuf> {
    match platform {
        Platform::Windows => Some(exe_dir.join(SEED_FILE)),
        Platform::Linux => Some(
            PathBuf::from("/usr/share")
                .join("code-basics")
                .join(SEED_FILE),
        ),
        Platform::Other => None,
    }
}

/// The seed path for this build's platform, given where the executable lives.
pub fn seed_path(exe_dir: &Path) -> Option<PathBuf> {
    seed_path_for(exe_dir, Platform::current())
}

/// Fold an installer seed into whatever the user already has.
///
/// **The seed applies only when there is no user file at all.** Once someone has
/// a store, it is theirs: a repair install, a reinstall or an upgrade must never
/// silently switch a feature back on that they turned off. That makes the seed
/// exactly what it claims to be — an initial value — rather than a periodic
/// override arriving from outside the app.
pub fn merge_seed(user: Option<FeaturesFile>, seed: FeaturesFile) -> FeaturesFile {
    // Deliberately all-or-nothing rather than a key-by-key merge. A per-key merge
    // would have to decide what an *absent* key in the user's file means, and it
    // means "no opinion, use the default" — which is indistinguishable from "the
    // seed should fill this in". Taking the user's file whole keeps the two
    // apart, at the cost of a seed never adding a newly-shipped feature to an
    // existing store. That is the right trade: a new feature already arrives
    // enabled through `FeatureId::default_enabled`.
    user.unwrap_or(seed)
}

/// Resolve the features for this launch, adopting an installer seed the first
/// time and never again.
///
/// Returns what the app should act on. When there is no user store yet and a
/// readable seed exists, the seed is **written through** to the user store, so
/// exactly one launch ever reads the installer's file — after that the store
/// stands on its own and an uninstall taking the seed with it changes nothing.
///
/// A seed that is absent, unreadable or corrupt is **ignored**, not fatal: a
/// launch must never fail over a preferences file, and a dev checkout has no
/// installer at all. In that case nothing is written — an ordinary launch has no
/// business creating a file it has nothing to say in.
pub fn ensure_seeded(path: &Path, seed: Option<&Path>) -> Result<FeaturesFile> {
    if let Some(existing) = load_existing(path) {
        return Ok(existing);
    }

    let Some(seed) = seed.and_then(read_seed) else {
        return Ok(FeaturesFile::default());
    };

    let adopted = merge_seed(None, seed);
    save(path, &adopted)?;
    Ok(adopted)
}

/// A seed file, if it is there and is actually parseable. Unlike [`load`] this
/// distinguishes "nothing to adopt" from "adopt the defaults", because adopting
/// writes the result through and a corrupt seed must not be written anywhere.
fn read_seed(path: &Path) -> Option<FeaturesFile> {
    let text = std::fs::read_to_string(path).ok()?;
    serde_json::from_str(&text).ok()
}

/// Read the features at `path`. A missing or unparseable file yields the default
/// [`FeaturesFile`] rather than an error — a corrupt preferences file must not
/// stop the app starting.
pub fn load(path: &Path) -> FeaturesFile {
    let Ok(text) = std::fs::read_to_string(path) else {
        return FeaturesFile::default();
    };
    serde_json::from_str(&text).unwrap_or_default()
}

/// Read the features at `path` only if the file is actually there, so a caller
/// can tell "no store yet" from "a store that happens to be empty" — which is the
/// distinction [`merge_seed`] turns on.
pub fn load_existing(path: &Path) -> Option<FeaturesFile> {
    let text = std::fs::read_to_string(path).ok()?;
    Some(serde_json::from_str(&text).unwrap_or_default())
}

/// A sibling of `path` with `suffix` appended to its file name.
fn sibling(path: &Path, suffix: &str) -> PathBuf {
    let mut name = path
        .file_name()
        .map(|n| n.to_os_string())
        .unwrap_or_default();
    name.push(suffix);
    path.with_file_name(name)
}

/// Write the features to `path`, creating the parent directory if absent.
///
/// Atomic, for the same reason [`crate::notes::save`] is: the JSON goes to a
/// sibling `.tmp` and is renamed over the target, so a crash mid-write cannot
/// leave a truncated file that the tolerant [`load`] would then read back as
/// "no choices made" and quietly clobber.
pub fn save(path: &Path, features: &FeaturesFile) -> Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("creating {}", parent.display()))?;
    }

    let json = serde_json::to_string_pretty(features).context("failed to serialise features")?;
    let tmp = sibling(path, ".tmp");
    std::fs::write(&tmp, format!("{json}\n"))
        .with_context(|| format!("writing {}", tmp.display()))?;
    std::fs::rename(&tmp, path).with_context(|| format!("replacing {}", path.display()))
}

#[cfg(test)]
#[path = "store_tests.rs"]
mod tests;

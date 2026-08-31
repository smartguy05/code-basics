//! Optional features: which of the app's non-core capabilities are switched on.
//!
//! The app ships **one binary containing every feature**, so nothing here removes
//! code — an installer checkbox and an in-app toggle both do the same thing, which
//! is write a preference this module reads at startup. That is the whole reason
//! the store exists: a Windows installer page and a Linux `.deb` (which cannot ask
//! the question at all) and the in-app picker must all feed one answer, and the
//! answer has to survive an upgrade.
//!
//! The store is **user-global, not per-workspace**, like [`crate::notes`] and
//! [`crate::launcher::store`]: "which features I want" belongs to the person, not
//! to a repository, and writing it into a checked-in `.code-basics/config.json`
//! would impose one developer's choices on their whole team.
//!
//! Same abstain-and-tolerate rule as the other user-global stores: a missing or
//! corrupt file loads as the defaults rather than erroring, because a bad file
//! must never stop the app starting — and an id this build does not recognise is
//! **kept**, not dropped, so a downgrade followed by an upgrade cannot silently
//! lose a choice the user made.

pub mod store;

pub use store::{
    ensure_seeded, features_path, load, load_existing, merge_seed, save, seed_path, seed_path_for,
    Platform, FEATURES_FILE, SEED_FILE,
};

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use specta::Type;

/// An optional feature the user can switch on or off.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FeatureId {
    /// The SQL console tab.
    SqlConsole,
    /// The "ask the codebase" question box and its agent terminal.
    AskCodebase,
}

impl FeatureId {
    /// Every feature this build knows about, in the order the picker lists them.
    pub const ALL: [FeatureId; 2] = [FeatureId::SqlConsole, FeatureId::AskCodebase];

    /// Stable id used across IPC, in the store file, and by both installers.
    pub fn id(self) -> &'static str {
        match self {
            FeatureId::SqlConsole => "sqlConsole",
            FeatureId::AskCodebase => "askCodebase",
        }
    }

    /// Human label for the picker and the installer page.
    pub fn label(self) -> &'static str {
        match self {
            FeatureId::SqlConsole => "SQL console",
            FeatureId::AskCodebase => "Ask the codebase",
        }
    }

    /// One line saying what the feature is, shown under the checkbox.
    pub fn description(self) -> &'static str {
        match self {
            FeatureId::SqlConsole => "Connect to a database and run queries.",
            FeatureId::AskCodebase => {
                "Ctrl+/ asks a coding agent about this codebase in a live terminal."
            }
        }
    }

    /// What this feature is when nothing has said otherwise.
    ///
    /// **On.** This is an existing app gaining capability, and a great many
    /// launches never see an installer at all — a `cargo run`, a dev checkout, an
    /// AppImage. Defaulting off would make those look broken. The installer's job
    /// is to let someone turn a feature *off*, not to be the only thing that can
    /// turn it on.
    pub fn default_enabled(self) -> bool {
        true
    }

    /// Look an id up, or `Err` naming the unknown one.
    pub fn from_id(id: &str) -> Result<Self, String> {
        Self::ALL
            .into_iter()
            .find(|f| f.id() == id)
            .ok_or_else(|| format!("unknown feature {id:?}"))
    }
}

/// The whole features file: a schema version and the explicit choices.
///
/// `enabled` holds only ids something has actually decided about. An id that is
/// absent is not "off" — it falls back to [`FeatureId::default_enabled`], which is
/// what lets a new feature ship enabled to people whose store predates it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct FeaturesFile {
    /// Schema version, so a future format change can migrate rather than fail.
    #[serde(default = "default_version")]
    pub version: u32,
    /// Explicit on/off by feature id. Ids this build does not know are preserved.
    #[serde(default)]
    pub enabled: BTreeMap<String, bool>,
}

fn default_version() -> u32 {
    1
}

impl Default for FeaturesFile {
    fn default() -> Self {
        Self {
            version: default_version(),
            enabled: BTreeMap::new(),
        }
    }
}

impl FeaturesFile {
    /// Whether `feature` is on, falling back to its built-in default when the
    /// file says nothing about it.
    pub fn is_enabled(&self, feature: FeatureId) -> bool {
        self.enabled
            .get(feature.id())
            .copied()
            .unwrap_or_else(|| feature.default_enabled())
    }

    /// Record an explicit choice.
    pub fn set(&mut self, feature: FeatureId, enabled: bool) {
        self.enabled.insert(feature.id().to_string(), enabled);
    }

    /// Every known feature with its current state, for the picker and for IPC.
    pub fn list(&self) -> Vec<FeatureInfo> {
        FeatureId::ALL
            .into_iter()
            .map(|f| FeatureInfo {
                id: f.id().to_string(),
                label: f.label().to_string(),
                description: f.description().to_string(),
                enabled: self.is_enabled(f),
            })
            .collect()
    }
}

/// One row of the features picker, as it crosses IPC.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct FeatureInfo {
    pub id: String,
    pub label: String,
    pub description: String,
    pub enabled: bool,
}

#[cfg(test)]
#[path = "features_tests.rs"]
mod tests;

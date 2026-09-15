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
    /// The read-only SQL MCP server a coding agent can be pointed at.
    McpSqlServer,
    /// The embedded web browser panel.
    WebBrowser,
    /// The per-codebase task list panel and its MCP server.
    Tasks,
    /// The per-workspace editor-context MCP server (active file, selection,
    /// cursor, open and recent files), for a coding agent to read.
    EditorContextMcp,
    /// The per-workspace build & diagnostics MCP server (run a build, read its
    /// errors, warnings and status), for a coding agent to read.
    BuildMcp,
}

impl FeatureId {
    /// Every feature this build knows about, in the order the picker lists them.
    pub const ALL: [FeatureId; 7] = [
        FeatureId::SqlConsole,
        FeatureId::AskCodebase,
        FeatureId::McpSqlServer,
        FeatureId::WebBrowser,
        FeatureId::Tasks,
        FeatureId::EditorContextMcp,
        FeatureId::BuildMcp,
    ];

    /// Stable id used across IPC, in the store file, and by both installers.
    pub fn id(self) -> &'static str {
        match self {
            FeatureId::SqlConsole => "sqlConsole",
            FeatureId::AskCodebase => "askCodebase",
            FeatureId::McpSqlServer => "mcpSqlServer",
            FeatureId::WebBrowser => "webBrowser",
            FeatureId::Tasks => "tasks",
            FeatureId::EditorContextMcp => "editorContextMcp",
            FeatureId::BuildMcp => "buildMcp",
        }
    }

    /// Human label for the picker and the installer page.
    pub fn label(self) -> &'static str {
        match self {
            FeatureId::SqlConsole => "SQL console",
            FeatureId::AskCodebase => "Ask the codebase",
            FeatureId::McpSqlServer => "SQL MCP server",
            FeatureId::WebBrowser => "Web browser",
            FeatureId::Tasks => "Tasks",
            FeatureId::EditorContextMcp => "Editor context MCP",
            FeatureId::BuildMcp => "Build MCP",
        }
    }

    /// One line saying what the feature is, shown under the checkbox.
    pub fn description(self) -> &'static str {
        match self {
            FeatureId::SqlConsole => "Connect to a database and run queries.",
            FeatureId::AskCodebase => {
                "Ctrl+/ asks a coding agent about this codebase in a live terminal."
            }
            FeatureId::McpSqlServer => {
                "Let a coding agent read the databases you expose, over MCP."
            }
            FeatureId::WebBrowser => {
                "A floating browser panel, for checking a deployment without leaving the app."
            }
            FeatureId::Tasks => "A per-codebase task list an agent can read and write over MCP.",
            FeatureId::EditorContextMcp => {
                "Let a coding agent read your live editor state (active file, selection, \
                 cursor, open and recent files) over MCP."
            }
            FeatureId::BuildMcp => {
                "Let a coding agent run this workspace's build and read its errors, warnings \
                 and status over MCP."
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
    ///
    /// This is an exhaustive `match` and not a blanket `true` on purpose. A
    /// blanket answer is one nobody has made for the feature being added: the
    /// next feature whose honest default is *off* — one that costs money, opens
    /// a port, or grants an agent access — would inherit "on" silently. Adding a
    /// variant is now a compile error until somebody decides, and
    /// `every_feature_states_its_own_default` names the decisions in one place.
    ///
    /// All three are on today for the reason above: an existing app gaining
    /// capability, launched most often with no installer to ask the question.
    /// The MCP server included — installing it into an agent is a separate,
    /// previewed, per-connection consent step, so the *feature* being visible
    /// grants nothing.
    pub fn default_enabled(self) -> bool {
        match self {
            FeatureId::SqlConsole => true,
            FeatureId::AskCodebase => true,
            FeatureId::McpSqlServer => true,
            // On, like the other three, for the reason above: an existing app
            // gaining capability, launched most often with no installer to ask
            // the question. It opens no port and grants an agent nothing — the
            // page is only what the user navigates to, and automation consent is
            // a separate per-page click that defaults to withheld.
            FeatureId::WebBrowser => true,
            // On, like the others: an existing app gaining capability. The list
            // itself is a local, gitignored file; pointing an agent's MCP config
            // at it is a separate, previewed install step, so the feature being
            // visible grants nothing on its own.
            FeatureId::Tasks => true,
            // On, like the others: an existing app gaining capability. While on,
            // the frontend pushes live editor state into the backend; but that
            // state only reaches an agent once the editor MCP server is a
            // separate, previewed install step, so the feature being visible
            // grants an agent nothing on its own. Read-only either way.
            FeatureId::EditorContextMcp => true,
            // On, like the others: an existing app gaining capability. While on,
            // an agent that installed the Build MCP entry can run and read this
            // workspace's build; installing that entry is a separate, previewed
            // step, so the feature being visible grants nothing on its own. A
            // build writes compiler output but changes no source file.
            FeatureId::BuildMcp => true,
        }
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

//! Per-tool enable/disable for the four MCP servers.
//!
//! Per-*server* enablement already exists ([`crate::features`]): a feature flag
//! decides whether a server is visible and installed. This module is the finer
//! grain the feature flags do not have — **which individual tools within a server
//! an agent may see and call** — and it is the first runtime filter any server
//! process performs, because until now every server advertised its whole
//! [`crate::mcp::tools::ALL`]-style array unconditionally.
//!
//! The store is **user-global, not per-workspace**, like [`crate::features`] and
//! [`crate::notes`]: "which tools I want exposed" belongs to the person, and the
//! four MCP servers are separate short-lived processes that must all resolve one
//! answer at the same path the app writes.
//!
//! # This store abstains *permissively*, and that is deliberate
//!
//! A missing or corrupt file loads as "every tool enabled", and an id the file
//! says nothing about is enabled. This is the **opposite** of the consent stores
//! ([`crate::sql::store`]'s `expose_to_agents`, the browser's automation
//! consent), which withhold by default — and it is correct here because
//! tool-gating is a *preference*, not a security boundary. A newly-shipped tool
//! arrives enabled to a store that predates it; a broken preferences file never
//! turns into "no tools". The genuine safety gates (per-connection exposure,
//! per-page automation consent, forced read-only) are enforced elsewhere and are
//! unaffected by anything here.
//!
//! # Two enforcement points, one rule
//!
//! Each server applies [`is_enabled`] twice: [`filter_descriptors`] removes a
//! disabled tool from `tools/list` so it is never advertised, and the dispatch
//! refuses a disabled-but-known tool that is called anyway (a client caches the
//! list, so a tool switched off mid-session is still callable until the client
//! reconnects — the dispatch refusal is what makes the switch authoritative on
//! the *next* call).

pub mod store;

pub use store::{load, load_existing, mcp_tools_path, save, MCP_TOOLS_FILE};

use std::collections::BTreeMap;

use rmcp::model::Tool;
use serde::{Deserialize, Serialize};
use specta::Type;

/// One of the four MCP servers whose tools can be gated.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ServerId {
    Sql,
    Tasks,
    Roslyn,
    Browser,
    EditorContext,
}

impl ServerId {
    /// Every server this build knows about, in the order the settings page lists
    /// them.
    pub const ALL: [ServerId; 5] = [
        ServerId::Sql,
        ServerId::Tasks,
        ServerId::Roslyn,
        ServerId::Browser,
        ServerId::EditorContext,
    ];

    /// Stable id used in the store key and across IPC.
    pub fn id(self) -> &'static str {
        match self {
            ServerId::Sql => "sql",
            ServerId::Tasks => "tasks",
            ServerId::Roslyn => "roslyn",
            ServerId::Browser => "browser",
            ServerId::EditorContext => "editor",
        }
    }

    /// Human label for the settings page.
    pub fn label(self) -> &'static str {
        match self {
            ServerId::Sql => "SQL",
            ServerId::Tasks => "Tasks",
            ServerId::Roslyn => "Roslyn",
            ServerId::Browser => "Web browser",
            ServerId::EditorContext => "Editor context",
        }
    }

    /// The tools this server advertises, straight from the one array the server's
    /// listing and dispatch both read — so the settings page can never show a
    /// tool the server does not have, or miss one it does.
    pub fn descriptors(self) -> Vec<Tool> {
        match self {
            ServerId::Sql => crate::mcp::tools::descriptors(),
            ServerId::Tasks => crate::tasks::mcp::tools::descriptors(),
            ServerId::Roslyn => crate::roslyn::tools::descriptors(),
            ServerId::Browser => crate::browser::tools::descriptors(),
            ServerId::EditorContext => crate::editor_context::tools::descriptors(),
        }
    }

    /// Look a server id up, or `Err` naming the unknown one.
    pub fn from_id(id: &str) -> Result<Self, String> {
        Self::ALL
            .into_iter()
            .find(|s| s.id() == id)
            .ok_or_else(|| format!("unknown MCP server {id:?}"))
    }
}

/// The composite store key for one tool of one server, e.g. `"sql/sql.query"`.
///
/// The `/` separator cannot collide with a tool name: tool names use only `.`
/// and `_`. Composite keys keep a same-named tool in two servers independent.
fn key(server: ServerId, tool: &str) -> String {
    format!("{}/{tool}", server.id())
}

/// The whole tool-gate file: a schema version and the explicit off/on choices.
///
/// `enabled` holds only `"server/tool"` keys something has actually decided
/// about. An absent key is **on** — which is what lets a newly-shipped tool
/// arrive enabled to a store that predates it. Keys this build does not recognise
/// are preserved, so a downgrade then upgrade cannot silently lose a choice.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct ToolGateFile {
    #[serde(default = "default_version")]
    pub version: u32,
    #[serde(default)]
    pub enabled: BTreeMap<String, bool>,
}

fn default_version() -> u32 {
    1
}

impl Default for ToolGateFile {
    fn default() -> Self {
        Self {
            version: default_version(),
            enabled: BTreeMap::new(),
        }
    }
}

impl ToolGateFile {
    /// Whether `tool` on `server` is enabled, defaulting to **on** when the file
    /// says nothing about it.
    pub fn is_enabled(&self, server: ServerId, tool: &str) -> bool {
        self.enabled
            .get(&key(server, tool))
            .copied()
            .unwrap_or(true)
    }

    /// Record an explicit choice.
    pub fn set(&mut self, server: ServerId, tool: &str, enabled: bool) {
        self.enabled.insert(key(server, tool), enabled);
    }

    /// Every server with its tools and their current state, for the settings
    /// page and for IPC.
    pub fn list(&self) -> Vec<McpServerToolsInfo> {
        ServerId::ALL
            .into_iter()
            .map(|server| McpServerToolsInfo {
                id: server.id().to_string(),
                label: server.label().to_string(),
                tools: server
                    .descriptors()
                    .into_iter()
                    .map(|tool| {
                        let name = tool.name.to_string();
                        let enabled = self.is_enabled(server, &name);
                        McpToolInfo {
                            description: tool
                                .description
                                .map(|d| d.to_string())
                                .unwrap_or_default(),
                            enabled,
                            name,
                        }
                    })
                    .collect(),
            })
            .collect()
    }
}

/// Drop the disabled tools from a descriptor list, keeping the rest **in order**.
///
/// The one function all four servers call from their `tools/list`, so the
/// advertise-side rule cannot drift between them.
pub fn filter_descriptors(gate: &ToolGateFile, server: ServerId, tools: Vec<Tool>) -> Vec<Tool> {
    tools
        .into_iter()
        .filter(|tool| gate.is_enabled(server, tool.name.as_ref()))
        .collect()
}

/// The machine-matchable code a disabled-tool refusal carries.
pub const DISABLED_CODE: &str = "toolDisabled";

/// What a model is told when it calls a tool the user switched off.
///
/// The one wording all four servers use, so a disabled-tool refusal reads the
/// same whether it comes from a self-answering server (SQL, Tasks) or a shim
/// (Roslyn, Browser). It ends with "do not retry": the tool is off by the user's
/// choice, and a retry cannot change that within this session.
pub fn disabled_tool_sentence(tool: &str) -> String {
    format!(
        "The tool {tool:?} was switched off by the user in code-basics' MCP settings, so it is \
         not available. Do not retry it; if you need it, ask the user to re-enable it there."
    )
}

/// A disabled-tool refusal as a **tool execution error** — for the two
/// self-answering servers (SQL, Tasks), which render refusals as a
/// [`rmcp::model::CallToolResult`]. The shims (Roslyn, Browser) build their own
/// `ToolAnswer` from [`disabled_tool_sentence`] instead, because their answer
/// type differs.
pub fn disabled_tool_result(tool: &str) -> rmcp::model::CallToolResult {
    use rmcp::model::{CallToolResult, ContentBlock};
    let mut result = CallToolResult::error(vec![ContentBlock::text(format!(
        "{DISABLED_CODE}: {}",
        disabled_tool_sentence(tool)
    ))]);
    result.structured_content = Some(serde_json::json!({ "code": DISABLED_CODE }));
    result
}

/// One server's row on the settings page, as it crosses IPC.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct McpServerToolsInfo {
    pub id: String,
    pub label: String,
    pub tools: Vec<McpToolInfo>,
}

/// One tool's row on the settings page.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct McpToolInfo {
    pub name: String,
    pub description: String,
    pub enabled: bool,
}

#[cfg(test)]
#[path = "tool_gate_tests.rs"]
mod tests;

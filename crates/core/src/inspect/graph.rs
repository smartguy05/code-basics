//! Reading what the sidecar wrote, without believing it.
//!
//! The sidecar emits a *flat* list of nodes with parent links, exactly as a
//! test runner emits a flat list of cases ([`crate::testing`]), and
//! [`super::tree`] shapes it afterwards. Keeping the wire format flat means
//! the sidecar never has to build a tree — it walks the heap and appends —
//! and it keeps the interesting decisions on this side, where they are
//! testable without a .NET runtime anywhere near them.
//!
//! # Why the wire types are loose and the model types are strict
//!
//! [`RawNode::kind`] is a plain string and most of its fields are optional.
//! That is deliberate. If the wire format were strongly typed, a sidecar that
//! emitted something unexpected would fail deserialisation and lose the whole
//! capture — including the ninety-nine nodes it got right. Instead the loose
//! shape always parses, and [`classify`] converts each node into a strict
//! [`ObjectValue`], **abstaining to [`ObjectValue::Unavailable`] whenever the
//! pieces do not add up**.
//!
//! That is the important property: a bug in the sidecar can cost the user a
//! value, but it can never invent one. A field rendered as `0` that was never
//! actually read is the failure mode worth engineering against, because the
//! user would believe it and go debug the wrong thing.

use anyhow::{Context, Result};
use serde::Deserialize;

use super::model::{Caps, ElidedReason, ObjectValue, TargetSummary};

/// The document the sidecar writes to `result.json`.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RawResult {
    pub schema_version: u32,
    pub snapshot_id: String,
    pub captured_at: String,
    pub target: TargetSummary,
    pub caps: Caps,
    #[serde(default)]
    pub nodes: Vec<RawNode>,
    #[serde(default)]
    pub warnings: Vec<String>,
    /// Set when the sidecar could not take a capture at all. A result file
    /// that carries this is still a *successful* exchange — the sidecar ran
    /// and explained itself, which is worth far more than a non-zero exit.
    #[serde(default)]
    pub failure: Option<String>,
    /// A stable identifier for *why* it failed, alongside the human sentence.
    ///
    /// Kept separate from `failure` so the decision to retry is never made by
    /// matching on prose: the message is written for a person and may be
    /// reworded at any time, while this is a contract.
    #[serde(default)]
    pub failure_code: Option<String>,
}

/// One node as the sidecar reported it.
///
/// Every field beyond `id` and `kind` is optional so that an unexpected
/// combination degrades to a single unreadable value rather than failing the
/// document.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RawNode {
    pub id: String,
    /// `None` for a root.
    #[serde(default)]
    pub parent: Option<String>,
    #[serde(default)]
    pub label: String,
    #[serde(default)]
    pub type_name: Option<String>,
    /// One of `primitive`, `text`, `null`, `reference`, `cycle`, `elided`,
    /// `unavailable`. Anything else is abstained on.
    pub kind: String,
    #[serde(default)]
    pub text: Option<String>,
    #[serde(default)]
    pub address: Option<String>,
    #[serde(default)]
    pub path: Option<String>,
    #[serde(default)]
    pub reason: Option<String>,
    #[serde(default)]
    pub expandable: bool,
    #[serde(default)]
    pub truncated: bool,
    #[serde(default)]
    pub child_count_total: Option<u32>,
}

/// Parse a result document.
///
/// A malformed document names the file and says what was wrong, in the style
/// of [`crate::testing::parse_file`] — the user should never be left holding
/// "invalid JSON" with no idea which of several artefacts produced it.
pub fn parse(content: &str) -> Result<RawResult> {
    let result: RawResult = serde_json::from_str(content)
        .context("the inspector wrote a result that could not be read")?;

    if result.schema_version != super::model::SCHEMA_VERSION {
        anyhow::bail!(
            "the inspector wrote a version {} result, but this build of code-basics reads \
             version {}. The bundled inspector is out of step with the application; \
             rebuilding it should resolve this.",
            result.schema_version,
            super::model::SCHEMA_VERSION
        );
    }

    Ok(result)
}

/// Turn one raw node into a value, abstaining rather than guessing.
///
/// Each arm requires the pieces that variant genuinely needs. A `reference`
/// with no address cannot be expanded and cannot be compared for a cycle, so
/// it is not a reference — it is something we could not read, and it says so.
pub fn classify(node: &RawNode) -> ObjectValue {
    match node.kind.as_str() {
        "null" => ObjectValue::Null,

        // A dictionary entry: a container grouping a Key and a Value. It has no
        // address, text or reason of its own — everything it carries is its two
        // children — so there is nothing to require here beyond the kind itself.
        "pair" => ObjectValue::Pair,

        "primitive" => match &node.text {
            Some(text) => ObjectValue::Primitive { text: text.clone() },
            None => unreadable("the inspector reported a value but did not include it"),
        },

        "text" => match &node.text {
            Some(text) => ObjectValue::Text {
                text: text.clone(),
                truncated: node.truncated,
            },
            None => unreadable("the inspector reported a string but did not include it"),
        },

        "reference" => match (&node.address, &node.type_name) {
            (Some(address), Some(type_name)) => ObjectValue::Reference {
                address: address.clone(),
                type_name: type_name.clone(),
                expandable: node.expandable,
            },
            // Without an address there is nothing to expand and no identity to
            // match a cycle against, so calling it a reference would promise
            // something the UI cannot deliver.
            (None, _) => unreadable("the inspector reported a reference with no address"),
            (_, None) => unreadable("the inspector reported a reference with no type"),
        },

        "cycle" => match (&node.address, &node.path) {
            (Some(address), Some(path)) => ObjectValue::Cycle {
                address: address.clone(),
                path: path.clone(),
            },
            // A cycle marker whose target is unknown would render as "already
            // shown above" with nowhere to jump to.
            _ => unreadable(
                "the inspector reported a repeated object but not where it first appeared",
            ),
        },

        "elided" => match node.reason.as_deref().and_then(elided_reason) {
            Some(reason) => ObjectValue::Elided { reason },
            None => unreadable("the inspector stopped here but did not say why"),
        },

        "unavailable" => unreadable(
            node.reason
                .as_deref()
                .filter(|r| !r.trim().is_empty())
                .unwrap_or("the inspector could not read this value"),
        ),

        // A kind this build does not know about. Rendering it as anything at
        // all would be a guess.
        other => ObjectValue::Unavailable {
            reason: format!("the inspector reported a value of an unrecognised kind `{other}`"),
        },
    }
}

fn unreadable(reason: &str) -> ObjectValue {
    ObjectValue::Unavailable {
        reason: reason.to_string(),
    }
}

fn elided_reason(raw: &str) -> Option<ElidedReason> {
    match raw {
        "depthLimit" => Some(ElidedReason::DepthLimit),
        "childLimit" => Some(ElidedReason::ChildLimit),
        "nodeLimit" => Some(ElidedReason::NodeLimit),
        _ => None,
    }
}

/// The C# compiler's name for an auto-property's storage.
const BACKING_PREFIX: &str = "<";
const BACKING_SUFFIX: &str = ">k__BackingField";

/// Recover the property name from an auto-property backing field.
///
/// `<Total>k__BackingField` is what the heap actually holds for
/// `public decimal Total { get; set; }`, and showing that verbatim is noise.
/// But the demangling only happens on an *exact* match of the compiler's
/// pattern: every other compiler-generated name (`<>c__DisplayClass0_0`,
/// `<GetAsync>d__7`) is left exactly as it is, because a half-recognised
/// mangled name relabelled as a property would claim the object has a member
/// it does not have.
pub fn display_label(field: &str) -> &str {
    let Some(rest) = field.strip_prefix(BACKING_PREFIX) else {
        return field;
    };
    let Some(name) = rest.strip_suffix(BACKING_SUFFIX) else {
        return field;
    };
    // `<>k__BackingField` has no property name to recover.
    if name.is_empty() {
        return field;
    }
    name
}

#[cfg(test)]
#[path = "graph_tests.rs"]
mod graph_tests;

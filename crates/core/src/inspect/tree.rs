//! Shaping the sidecar's flat node list into the tree the UI renders.
//!
//! The same split as [`crate::testing::tree`]: the producer emits a flat list,
//! and the hierarchy is built once, here, so every capture — dump or live,
//! exception or hand-picked root — produces an identically shaped tree.
//!
//! # Why this is defensive
//!
//! The input describes a graph, and the caller cannot assume it is well
//! formed: a node may name a parent that was never sent, two nodes may claim
//! the same id, and — because the sidecar walks a *cyclic* structure — the
//! parent links themselves may form a loop if it has a bug. None of those may
//! hang the application or silently drop data. Every one of them produces a
//! warning that reaches the user, and the nodes involved are still shown.

use std::collections::{HashMap, HashSet};

use super::graph::{self, RawNode};
use super::model::InspectNode;

/// How many levels of *structure* may nest, regardless of the walk's own depth
/// cap.
///
/// The sidecar's `maxDepth` is normally what limits this, but that is a value
/// in a file we did not write. This is the backstop that keeps a malformed
/// document from exhausting the stack. A chain of nodes is displayed this many
/// levels deep and no deeper.
const MAX_STRUCTURAL_DEPTH: usize = 64;

/// A shaped capture, plus anything the shaping itself noticed.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Built {
    pub roots: Vec<InspectNode>,
    pub warnings: Vec<String>,
}

/// Build the tree.
///
/// Sibling order is the order the sidecar emitted, which is the order the
/// fields appear on the type — re-sorting would lose that, and a declaration
/// order a developer recognises is worth more than an alphabetical one.
pub fn build(nodes: &[RawNode]) -> Built {
    let mut built = Built::default();

    // Index by id, keeping the first of any duplicate. Picking between two
    // nodes claiming the same identity would be a guess, so the second is
    // reported rather than quietly overwriting the first.
    let mut index: HashMap<&str, &RawNode> = HashMap::with_capacity(nodes.len());
    for node in nodes {
        // `insert` would replace, leaving the *last* duplicate as the winner
        // while the warning claims the first was kept.
        if index.contains_key(node.id.as_str()) {
            built.warnings.push(format!(
                "the inspector reported more than one value with the id `{}`; only the first is shown",
                node.id
            ));
        } else {
            index.insert(node.id.as_str(), node);
        }
    }

    // Children in emission order, keyed by parent id.
    let mut children: HashMap<&str, Vec<&RawNode>> = HashMap::new();
    let mut roots: Vec<&RawNode> = Vec::new();

    for node in nodes {
        // Skip the losing half of a duplicate id: it is already reported, and
        // attaching it would show the same value twice.
        if !std::ptr::eq(index[node.id.as_str()], node) {
            continue;
        }

        match node.parent.as_deref() {
            None => roots.push(node),
            Some(parent) if index.contains_key(parent) => {
                children.entry(parent).or_default().push(node);
            }
            // An orphan is promoted to a root rather than dropped: losing a
            // value silently is the one outcome worth avoiding, and the
            // warning says exactly what happened.
            Some(parent) => {
                built.warnings.push(format!(
                    "`{}` was reported as belonging to `{parent}`, which the inspector did not send; \
                     it is shown at the top level instead",
                    node.id
                ));
                roots.push(node);
            }
        }
    }

    let mut path = HashSet::new();
    let mut visited = HashSet::new();
    built.roots = roots
        .into_iter()
        .map(|node| {
            shape(
                node,
                &children,
                &mut path,
                &mut visited,
                0,
                &mut built.warnings,
            )
        })
        .collect();

    // Anything still unvisited is only reachable through a loop in the parent
    // links — every node in such a loop names a parent that exists, so none of
    // them is a root and the walk above never reaches them. Promoting them
    // keeps the values on screen; dropping them would be the silent data loss
    // this module exists to avoid.
    for node in nodes {
        if visited.contains(&node.id) || !std::ptr::eq(index[node.id.as_str()], node) {
            continue;
        }
        built.warnings.push(format!(
            "`{}` is part of a loop in the inspector's own parent links; it is shown at the top level",
            node.id
        ));
        built.roots.push(shape(
            node,
            &children,
            &mut path,
            &mut visited,
            0,
            &mut built.warnings,
        ));
    }

    built
}

fn shape(
    node: &RawNode,
    children: &HashMap<&str, Vec<&RawNode>>,
    // Ids on the current branch, for breaking a parent-link loop.
    path: &mut HashSet<String>,
    // Every id shaped anywhere, so nothing is left unreachable.
    visited: &mut HashSet<String>,
    depth: usize,
    warnings: &mut Vec<String>,
) -> InspectNode {
    let value = graph::classify(node);
    let label = graph::display_label(&node.label).to_string();
    visited.insert(node.id.clone());

    // A parent link that loops back on itself is a sidecar bug, not a cycle in
    // the user's data — real cycles arrive as `ObjectValue::Cycle` leaves. It
    // still must not recurse forever.
    let recursed = !path.insert(node.id.clone());
    // `depth` is 0-based, so a node at `MAX - 1` is the last level displayed
    // and is where the children stop.
    let too_deep = depth + 1 >= MAX_STRUCTURAL_DEPTH;

    if recursed {
        warnings.push(format!(
            "the inspector reported `{}` inside itself; its contents are not shown",
            node.id
        ));
    } else if too_deep {
        warnings.push(format!(
            "`{}` is nested more deeply than code-basics will display; its contents are not shown",
            node.id
        ));
    }

    let shaped: Vec<InspectNode> = if recursed || too_deep {
        Vec::new()
    } else {
        children
            .get(node.id.as_str())
            .map(|kids| {
                kids.iter()
                    .map(|kid| shape(kid, children, path, visited, depth + 1, warnings))
                    .collect()
            })
            .unwrap_or_default()
    };

    if !recursed {
        path.remove(&node.id);
    }

    // `has_more` drives the "showing 100 of 5,412" affordance, so it must only
    // be set when the sidecar actually counted more than it sent.
    let has_more = node
        .child_count_total
        .is_some_and(|total| total as usize > shaped.len());

    InspectNode {
        id: node.id.clone(),
        label,
        type_name: node.type_name.clone(),
        value,
        children: shaped,
        has_more,
        child_count_total: node.child_count_total,
    }
}

#[cfg(test)]
#[path = "tree_tests.rs"]
mod tree_tests;

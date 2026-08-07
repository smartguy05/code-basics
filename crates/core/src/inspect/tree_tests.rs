use super::*;
use crate::inspect::model::ObjectValue;

fn node(id: &str, parent: Option<&str>, label: &str) -> RawNode {
    RawNode {
        id: id.into(),
        parent: parent.map(str::to_string),
        label: label.into(),
        type_name: None,
        kind: "null".into(),
        text: None,
        address: None,
        path: None,
        reason: None,
        expandable: false,
        truncated: false,
        child_count_total: None,
    }
}

fn labels(nodes: &[InspectNode]) -> Vec<&str> {
    nodes.iter().map(|n| n.label.as_str()).collect()
}

fn ids(nodes: &[InspectNode]) -> Vec<&str> {
    nodes.iter().map(|n| n.id.as_str()).collect()
}

#[test]
fn a_flat_list_becomes_the_hierarchy_it_describes() {
    let built = build(&[
        node("root", None, "order"),
        node("root.customer", Some("root"), "customer"),
        node("root.customer.name", Some("root.customer"), "name"),
        node("root.total", Some("root"), "total"),
    ]);

    assert!(built.warnings.is_empty(), "{:?}", built.warnings);
    assert_eq!(ids(&built.roots), ["root"]);

    let root = &built.roots[0];
    assert_eq!(labels(&root.children), ["customer", "total"]);
    assert_eq!(labels(&root.children[0].children), ["name"]);
    assert!(root.children[1].children.is_empty());
}

#[test]
fn siblings_keep_the_order_the_inspector_sent() {
    // Field declaration order is what a developer recognises when scanning a
    // type; sorting alphabetically would throw that away.
    let built = build(&[
        node("r", None, "r"),
        node("r.zebra", Some("r"), "zebra"),
        node("r.apple", Some("r"), "apple"),
        node("r.mango", Some("r"), "mango"),
    ]);

    assert_eq!(
        labels(&built.roots[0].children),
        ["zebra", "apple", "mango"]
    );
}

#[test]
fn several_roots_are_all_kept() {
    // A `Type` or `Exceptions` query returns many unrelated objects.
    let built = build(&[
        node("a", None, "first"),
        node("b", None, "second"),
        node("c", None, "third"),
    ]);

    assert_eq!(ids(&built.roots), ["a", "b", "c"]);
    assert!(built.warnings.is_empty());
}

#[test]
fn values_are_classified_and_backing_fields_relabelled_while_shaping() {
    let mut total = node("r.total", Some("r"), "<Total>k__BackingField");
    total.kind = "primitive".into();
    total.text = Some("12450.00".into());

    let built = build(&[node("r", None, "order"), total]);
    let field = &built.roots[0].children[0];

    assert_eq!(
        field.label, "Total",
        "the auto-property name should be recovered"
    );
    assert_eq!(
        field.value,
        ObjectValue::Primitive {
            text: "12450.00".into()
        }
    );
    // The id is the wire identity and must not be rewritten with it.
    assert_eq!(field.id, "r.total");
}

#[test]
fn empty_input_produces_an_empty_tree_rather_than_an_error() {
    let built = build(&[]);
    assert!(built.roots.is_empty());
    assert!(built.warnings.is_empty());
}

// ---------------------------------------------------------------------------
// Malformed input: nothing is lost, nothing hangs
// ---------------------------------------------------------------------------

#[test]
fn a_node_naming_a_parent_that_was_never_sent_is_promoted_not_dropped() {
    let built = build(&[
        node("r", None, "root"),
        node("stray", Some("ghost"), "stray"),
    ]);

    assert_eq!(ids(&built.roots), ["r", "stray"]);
    assert_eq!(built.warnings.len(), 1);
    assert!(built.warnings[0].contains("ghost"), "{}", built.warnings[0]);
    assert!(
        built.warnings[0].contains("top level"),
        "{}",
        built.warnings[0]
    );
}

#[test]
fn a_duplicate_id_keeps_the_first_and_says_so() {
    // Choosing between two values claiming the same identity would be a guess.
    let mut second = node("r.field", Some("r"), "second");
    second.kind = "primitive".into();
    second.text = Some("later".into());

    let mut first = node("r.field", Some("r"), "first");
    first.kind = "primitive".into();
    first.text = Some("earlier".into());

    let built = build(&[node("r", None, "root"), first, second]);

    assert_eq!(labels(&built.roots[0].children), ["first"]);
    assert_eq!(built.warnings.len(), 1);
    assert!(
        built.warnings[0].contains("r.field"),
        "{}",
        built.warnings[0]
    );
}

/// Two nodes naming each other as parent: neither is a root, so a naive walk
/// would never reach either and both would vanish without a word.
#[test]
fn a_loop_in_the_parent_links_loses_nothing_and_terminates() {
    let built = build(&[
        node("a", Some("b"), "a"),
        node("b", Some("a"), "b"),
        node("r", None, "root"),
    ]);

    let shown = ids(&built.roots);
    assert!(shown.contains(&"r"));
    assert!(
        shown.contains(&"a") || shown.contains(&"b"),
        "a parent-link loop must still surface its values, got {shown:?}"
    );
    assert!(
        built.warnings.iter().any(|w| w.contains("loop")),
        "the loop should be reported: {:?}",
        built.warnings
    );
}

#[test]
fn a_node_that_is_its_own_parent_does_not_recurse_forever() {
    let built = build(&[node("r", None, "root"), node("self", Some("self"), "self")]);

    assert!(built.warnings.iter().any(|w| w.contains("self")));
    // The value is still shown, just without contents.
    assert!(ids(&built.roots).contains(&"self"));
}

#[test]
fn structure_nested_past_the_display_limit_stops_rather_than_overflowing_the_stack() {
    // The sidecar's own depth cap normally prevents this, but that cap lives
    // in a file we did not write.
    let depth = MAX_STRUCTURAL_DEPTH + 20;
    let mut nodes = vec![node("n0", None, "n0")];
    for i in 1..depth {
        let id = format!("n{i}");
        let parent = format!("n{}", i - 1);
        nodes.push(node(&id, Some(&parent), &id));
    }

    let built = build(&nodes);

    let mut current = &built.roots[0];
    let mut reached = 1;
    while let Some(child) = current.children.first() {
        current = child;
        reached += 1;
    }

    assert_eq!(reached, MAX_STRUCTURAL_DEPTH);
    assert!(
        built.warnings.iter().any(|w| w.contains("more deeply")),
        "{:?}",
        built.warnings
    );
}

// ---------------------------------------------------------------------------
// Partial collections
// ---------------------------------------------------------------------------

#[test]
fn has_more_is_set_only_when_more_was_actually_counted() {
    let mut capped = node("r.items", Some("r"), "items");
    capped.child_count_total = Some(5412);

    let mut complete = node("r.tags", Some("r"), "tags");
    complete.child_count_total = Some(1);

    let built = build(&[
        node("r", None, "root"),
        capped,
        node("r.items[0]", Some("r.items"), "[0]"),
        complete,
        node("r.tags[0]", Some("r.tags"), "[0]"),
    ]);

    let items = &built.roots[0].children[0];
    assert!(items.has_more, "1 of 5412 shown should offer more");
    assert_eq!(items.child_count_total, Some(5412));

    let tags = &built.roots[0].children[1];
    assert!(!tags.has_more, "1 of 1 shown is complete");
}

#[test]
fn an_uncounted_collection_does_not_claim_there_is_more() {
    // No count means the sidecar could not determine one. Claiming more exists
    // would offer an expansion that returns nothing.
    let built = build(&[node("r", None, "root"), node("r.child", Some("r"), "child")]);

    assert!(!built.roots[0].has_more);
    assert_eq!(built.roots[0].child_count_total, None);
}

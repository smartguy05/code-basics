//! Every rule this module claims to obey, one test each.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use super::*;
use crate::lsp::model::{Availability, Highlight};
use crate::lsp::protocol::{Location, Position, Range, Symbol};
use crate::symbols::declarations::SymbolKind;

// ---------------------------------------------------------------------------
// Fixtures
// ---------------------------------------------------------------------------

const ROOT: &str = if cfg!(windows) { r"C:\repo" } else { "/repo" };

/// The workspace text, injected. Keys are absolute paths, as the caller sees.
#[derive(Default)]
struct Lines {
    files: HashMap<PathBuf, Vec<String>>,
    /// Every path this provider was asked about, in order.
    asked: Vec<PathBuf>,
}

impl Lines {
    fn with(path: &str, lines: &[&str]) -> Self {
        let mut lines_by_file = Lines::default();
        lines_by_file.add(path, lines);
        lines_by_file
    }

    fn add(&mut self, path: &str, lines: &[&str]) -> &mut Self {
        self.files.insert(
            PathBuf::from(path),
            lines.iter().map(|l| (*l).to_string()).collect(),
        );
        self
    }
}

impl TextProvider for Lines {
    fn line(&mut self, path: &Path, line_zero_based: u32) -> Option<String> {
        self.asked.push(path.to_path_buf());
        self.files
            .get(path)
            .and_then(|lines| lines.get(line_zero_based as usize))
            .cloned()
    }
}

/// A `file:` URI under [`ROOT`], spelled the way Roslyn spells one.
fn uri(relative: &str) -> String {
    if cfg!(windows) {
        format!("file:///C:/repo/{relative}")
    } else {
        format!("file:///repo/{relative}")
    }
}

/// The absolute path of a workspace-relative file, for seeding [`Lines`].
fn absolute(relative: &str) -> String {
    if cfg!(windows) {
        format!(r"C:\repo\{}", relative.replace('/', "\\"))
    } else {
        format!("/repo/{relative}")
    }
}

fn at(uri: &str, line: u32, start: u32, end: u32) -> Location {
    Location {
        uri: uri.to_string(),
        range: Range {
            start: Position {
                line,
                character: start,
            },
            end: Position {
                line,
                character: end,
            },
        },
    }
}

fn root() -> &'static Path {
    Path::new(ROOT)
}

// ---------------------------------------------------------------------------
// Paths
// ---------------------------------------------------------------------------

#[test]
fn a_location_is_resolved_by_path_and_never_by_comparing_uri_strings() {
    // The same file, spelled two ways: rust-analyzer percent-encodes the drive
    // colon and Roslyn does not. A string comparison would call these two
    // different files and report two usages where there is one.
    let encoded = if cfg!(windows) {
        "file:///C%3A/repo/src/a.cs"
    } else {
        "file:///repo/src/a.cs"
    };
    let plain = uri("src/a.cs");
    let mut text = Lines::with(&absolute("src/a.cs"), &["", "    Total();"]);

    let result = usages(
        root(),
        &[at(&plain, 1, 4, 9), at(encoded, 1, 4, 9)],
        &mut text,
        50,
    );

    assert_eq!(result.outcome, Availability::Ready);
    assert_eq!(result.total, Some(1));
    assert_eq!(result.usages.len(), 1);
    assert_eq!(result.usages[0].path, Some(PathBuf::from("src/a.cs")));
    assert_eq!(result.usages[0].label, "src/a.cs");
}

#[test]
fn a_location_outside_the_root_keeps_its_row_with_no_path_and_the_raw_uri() {
    // Not dropped — the count would be wrong. Not joined onto the root — that
    // would open a different file that happens to sit where the guess landed.
    let outside = if cfg!(windows) {
        "file:///C:/elsewhere/Other.cs"
    } else {
        "file:///elsewhere/Other.cs"
    };
    let mut text = Lines::default();

    let result = usages(root(), &[at(outside, 0, 0, 5)], &mut text, 50);

    assert_eq!(result.total, Some(1));
    assert_eq!(result.usages.len(), 1);
    assert_eq!(result.usages[0].path, None);
    assert_eq!(result.usages[0].label, outside);
}

#[test]
fn a_non_file_uri_keeps_its_row_and_is_still_counted() {
    // Roslyn really does answer with these for decompiled and generated code.
    let generated = "source-generated:/Microsoft.Extensions/Foo.g.cs";
    let mut text = Lines::with(&absolute("src/a.cs"), &["    Total();"]);

    let result = usages(
        root(),
        &[at(&uri("src/a.cs"), 0, 4, 9), at(generated, 3, 0, 4)],
        &mut text,
        50,
    );

    assert_eq!(result.total, Some(2));
    let unopenable = result
        .usages
        .iter()
        .find(|u| u.path.is_none())
        .expect("the generated document must still be a row");
    assert_eq!(unopenable.label, generated);
    assert_eq!(unopenable.line, 4, "1-based at the boundary");
    assert_eq!(unopenable.snippet, "");
    assert_eq!(unopenable.highlight, None);
}

// ---------------------------------------------------------------------------
// Duplicates and order
// ---------------------------------------------------------------------------

#[test]
fn identical_locations_are_deduplicated() {
    let mut text = Lines::with(&absolute("src/a.cs"), &["    Total();"]);
    let one = at(&uri("src/a.cs"), 0, 4, 9);

    let result = usages(root(), &[one.clone(), one.clone(), one], &mut text, 50);

    assert_eq!(result.total, Some(1));
    assert_eq!(result.usages.len(), 1);
}

#[test]
fn two_matches_on_one_line_are_two_rows() {
    // Deduplication must key on the whole location, not on the line: `Total +
    // Total` is two use sites.
    let mut text = Lines::with(&absolute("src/a.cs"), &["    Total + Total;"]);

    let result = usages(
        root(),
        &[
            at(&uri("src/a.cs"), 0, 4, 9),
            at(&uri("src/a.cs"), 0, 12, 17),
        ],
        &mut text,
        50,
    );

    assert_eq!(result.total, Some(2));
}

#[test]
fn rows_are_sorted_by_path_then_line_then_character() {
    // An unstable order reshuffles the dropdown under the user's cursor.
    let mut text = Lines::default();
    text.add(&absolute("src/a.cs"), &["a0", "a1", "a2"])
        .add(&absolute("src/b.cs"), &["b0"]);

    let result = usages(
        root(),
        &[
            at(&uri("src/b.cs"), 0, 0, 1),
            at(&uri("src/a.cs"), 2, 0, 1),
            at(&uri("src/a.cs"), 0, 9, 10),
            at(&uri("src/a.cs"), 0, 1, 2),
        ],
        &mut text,
        50,
    );

    let order: Vec<(String, u32)> = result
        .usages
        .iter()
        .map(|u| (u.label.clone(), u.line))
        .collect();
    assert_eq!(
        order,
        vec![
            ("src/a.cs".to_string(), 1),
            ("src/a.cs".to_string(), 1),
            ("src/a.cs".to_string(), 3),
            ("src/b.cs".to_string(), 1),
        ]
    );
    // Same file, same line: the earlier column comes first.
    assert_eq!(result.usages[0].snippet, "a0");
}

#[test]
fn an_unopenable_row_sorts_after_every_openable_one() {
    let mut text = Lines::with(&absolute("src/z.cs"), &["z"]);

    let result = usages(
        root(),
        &[
            at("source-generated:/Foo.g.cs", 0, 0, 1),
            at(&uri("src/z.cs"), 0, 0, 1),
        ],
        &mut text,
        50,
    );

    assert_eq!(result.usages[0].path, Some(PathBuf::from("src/z.cs")));
    assert_eq!(result.usages[1].path, None);
}

// ---------------------------------------------------------------------------
// The cap, and the difference between no count and a count of zero
// ---------------------------------------------------------------------------

#[test]
fn the_cap_truncates_the_list_but_total_still_reports_the_true_count() {
    // A cap that also caps the number is a lie, and the number is the claim the
    // user acts on before changing a method.
    let mut text = Lines::default();
    text.add(&absolute("src/a.cs"), &["0", "1", "2", "3", "4"]);
    let locations: Vec<Location> = (0..5)
        .map(|line| at(&uri("src/a.cs"), line, 0, 1))
        .collect();

    let result = usages(root(), &locations, &mut text, 2);

    assert_eq!(result.total, Some(5));
    assert_eq!(result.usages.len(), 2);
    assert!(result.truncated);
    // The rows kept are the first ones in sorted order, not an arbitrary two.
    assert_eq!(result.usages[0].line, 1);
    assert_eq!(result.usages[1].line, 2);
}

#[test]
fn a_list_that_fits_is_not_marked_truncated() {
    let mut text = Lines::with(&absolute("src/a.cs"), &["0", "1"]);
    let locations = vec![at(&uri("src/a.cs"), 0, 0, 1), at(&uri("src/a.cs"), 1, 0, 1)];

    let result = usages(root(), &locations, &mut text, 2);

    assert_eq!(result.total, Some(2));
    assert!(!result.truncated);
}

#[test]
fn a_genuine_zero_is_some_zero_and_an_unavailable_answer_is_none() {
    // The whole subsystem exists to keep these two apart.
    let mut text = Lines::default();
    let found_nothing = usages(root(), &[], &mut text, 50);
    assert_eq!(found_nothing.outcome, Availability::Ready);
    assert_eq!(found_nothing.total, Some(0));
    assert!(!found_nothing.truncated);

    let could_not_ask = UsageResult::unavailable(Availability::Loading, "still loading");
    assert_eq!(could_not_ask.total, None);
    assert_ne!(found_nothing.total, could_not_ask.total);
}

// ---------------------------------------------------------------------------
// Snippets and the byte-to-UTF-16 conversion
// ---------------------------------------------------------------------------

#[test]
fn a_highlight_crosses_as_utf16_code_units_not_bytes() {
    // The line carries a 2-byte character, a 3-byte one and an astral-plane
    // character that is 4 bytes and **2** UTF-16 units, so bytes, `char`s and
    // code units all disagree. Shipping the byte offsets would underline the
    // wrong characters and nothing would ever notice.
    //
    //   `  let a = "é€𝄞"; Total();`
    //    0 1 indent
    //    utf16: l=2 … `"`=10, é=11, €=12, 𝄞=13..15, `"`=15, `;`=16, ` `=17,
    //    T=18 … end of `Total` = 23.
    let source = r#"  let a = "é€𝄞"; Total();"#;
    let mut text = Lines::with(&absolute("src/a.cs"), &[source]);

    let result = usages(root(), &[at(&uri("src/a.cs"), 0, 18, 23)], &mut text, 50);

    let usage = &result.usages[0];
    assert_eq!(usage.snippet, r#"let a = "é€𝄞"; Total();"#);
    // Two spaces of indentation were trimmed, so the UTF-16 offset moves by two.
    assert_eq!(usage.highlight, Some(Highlight { start: 16, end: 21 }));

    // Prove the numbers are code units and not bytes: the byte offset of the
    // same span is different, and slicing by it is what a missing conversion
    // would produce.
    let byte_start = usage.snippet.find("Total").unwrap();
    assert_ne!(
        byte_start, 16,
        "the byte offset must differ, or this proves nothing"
    );
    let start = usage.highlight.unwrap().start as usize;
    let end = usage.highlight.unwrap().end as usize;
    let units: Vec<u16> = usage
        .snippet
        .encode_utf16()
        .skip(start)
        .take(end - start)
        .collect();
    assert_eq!(String::from_utf16(&units).unwrap(), "Total");
}

#[test]
fn a_line_the_provider_cannot_read_yields_an_empty_snippet_and_no_highlight() {
    // A file that moved, or a server answering about a buffer version we no
    // longer have. The row survives because the count must stay right.
    let mut text = Lines::default();

    let result = usages(root(), &[at(&uri("src/gone.cs"), 7, 0, 4)], &mut text, 50);

    assert_eq!(result.total, Some(1));
    assert_eq!(result.usages[0].snippet, "");
    assert_eq!(result.usages[0].highlight, None);
    assert_eq!(result.usages[0].path, Some(PathBuf::from("src/gone.cs")));
}

#[test]
fn a_range_spanning_lines_underlines_to_the_end_of_its_first_line() {
    // The span demonstrably covers the rest of the line, so underlining to the
    // end states something true; carrying the second line's column into the
    // first line's snippet would not.
    //
    // The astral character is load-bearing: "the end of this line" is computed
    // for `positions::snippet`, which takes **UTF-16** columns, and on an ASCII
    // line `len()`, `chars().count()` and `encode_utf16().count()` are the same
    // number. `𝄞` is one char, two UTF-16 units and four bytes, so the three
    // disagree and only the right one puts the end of the underline at the end of
    // the line.
    let mut text = Lines::with(&absolute("src/a.cs"), &["    𝄞Total(x,", "        y);"]);

    let result = usages(
        root(),
        &[Location {
            uri: uri("src/a.cs"),
            range: Range {
                start: Position {
                    line: 0,
                    // UTF-16: four spaces plus the two units of `𝄞`.
                    character: 6,
                },
                end: Position {
                    line: 1,
                    character: 10,
                },
            },
        }],
        &mut text,
        50,
    );

    assert_eq!(result.usages[0].snippet, "𝄞Total(x,");
    assert_eq!(
        result.usages[0].highlight,
        // UTF-16 offsets into the snippet: past the `𝄞`, and on to its end.
        Some(Highlight { start: 2, end: 10 })
    );
}

#[test]
fn the_text_provider_is_asked_for_the_zero_based_line_the_server_sent() {
    // The 1-based line is a boundary convention, not a file-reading one; asking
    // for the wrong one shows the reader the line above the match.
    let mut text = Lines::with(&absolute("src/a.cs"), &["zero", "one", "two"]);

    let result = usages(root(), &[at(&uri("src/a.cs"), 2, 0, 3)], &mut text, 50);

    assert_eq!(result.usages[0].snippet, "two");
    assert_eq!(result.usages[0].line, 3);
    // And the provider is handed the absolute path, which is the only thing
    // that names a file; a workspace-relative one would resolve against the
    // process's working directory.
    assert_eq!(text.asked, vec![PathBuf::from(absolute("src/a.cs"))]);
}

// ---------------------------------------------------------------------------
// Definitions
// ---------------------------------------------------------------------------

#[test]
fn a_target_repeated_within_one_group_is_listed_once() {
    let mut text = Lines::with(&absolute("src/a.cs"), &["class Order {}"]);
    let one = at(&uri("src/a.cs"), 0, 6, 11);

    let result = targets(root(), &[one.clone(), one.clone()], &[], &[], &mut text);

    assert_eq!(result.outcome, Availability::Ready);
    assert_eq!(result.declarations.len(), 1);
}

#[test]
fn a_target_in_two_groups_stays_in_both() {
    // An interface method is a declaration from one angle and, from another, an
    // implementation. Cross-group deduplication would delete a real answer.
    let mut text = Lines::with(&absolute("src/a.cs"), &["    int Total { get; }"]);
    let one = [at(&uri("src/a.cs"), 0, 8, 13)];

    let result = targets(root(), &one, &one, &one, &mut text);

    assert_eq!(result.declarations.len(), 1);
    assert_eq!(result.implementations.len(), 1);
    assert_eq!(result.type_definitions.len(), 1);
}

#[test]
fn a_target_carries_a_one_based_line_and_a_zero_based_character() {
    // The asymmetry is the contract; a test that does not state it is how the
    // off-by-one ships.
    let mut text = Lines::with(&absolute("src/a.cs"), &["", "    class Order {}"]);

    let result = targets(
        root(),
        &[at(&uri("src/a.cs"), 1, 10, 15)],
        &[],
        &[],
        &mut text,
    );

    let target = &result.declarations[0];
    assert_eq!(target.line, 2, "1-based, matching the gutter");
    assert_eq!(target.character, 10, "0-based UTF-16, matching CodeMirror");
    assert_eq!(target.snippet, "class Order {}");
    assert_eq!(target.label, "src/a.cs");
    assert_eq!(target.container, None);
}

#[test]
fn a_target_outside_the_root_keeps_its_row_with_no_path() {
    let mut text = Lines::default();
    let result = targets(
        root(),
        &[],
        &[at("metadata:/System.Object", 0, 0, 1)],
        &[],
        &mut text,
    );

    assert_eq!(result.implementations.len(), 1);
    assert_eq!(result.implementations[0].path, None);
    assert_eq!(result.implementations[0].label, "metadata:/System.Object");
}

#[test]
fn definitions_with_nothing_anywhere_are_ready_and_empty() {
    let mut text = Lines::default();
    let result = targets(root(), &[], &[], &[], &mut text);

    assert_eq!(result.outcome, Availability::Ready);
    assert!(result.declarations.is_empty());
    assert_eq!(result.message, None);
}

// ---------------------------------------------------------------------------
// Anchors
// ---------------------------------------------------------------------------

fn symbol(name: &str, kind: SymbolKind, line: u32, character: u32, container: &[&str]) -> Symbol {
    Symbol {
        name: name.to_string(),
        detail: None,
        kind,
        range: Range {
            start: Position { line, character: 4 },
            end: Position {
                line: line + 3,
                character: 5,
            },
        },
        selection_range: Range {
            start: Position { line, character },
            end: Position {
                line,
                character: character + name.len() as u32,
            },
        },
        container: container.iter().map(|c| (*c).to_string()).collect(),
        uri: None,
    }
}

#[test]
fn anchors_keep_methods_properties_and_classes_and_drop_local_variables() {
    let symbols = vec![
        symbol("Orders", SymbolKind::Namespace, 0, 10, &[]),
        symbol("Order", SymbolKind::Class, 1, 17, &["Orders"]),
        symbol("Total", SymbolKind::Variable, 2, 15, &["Orders", "Order"]),
        symbol("Compute", SymbolKind::Function, 3, 15, &["Orders", "Order"]),
        // A local, which LSP reports with the very same kind as a property.
        symbol(
            "count",
            SymbolKind::Variable,
            4,
            12,
            &["Orders", "Order", "Compute"],
        ),
        symbol("unknown", SymbolKind::Other, 5, 4, &["Orders", "Order"]),
    ];

    let names: Vec<String> = anchors(&symbols).into_iter().map(|a| a.name).collect();

    assert_eq!(names, vec!["Order", "Total", "Compute"]);
}

#[test]
fn a_top_level_variable_is_not_an_anchor() {
    // Named storage earns a row by being a member of a type. With no container
    // there is nothing to prove it is not a local, and refusing costs a click
    // where admitting costs a wrong row.
    let symbols = vec![symbol("buffer", SymbolKind::Variable, 0, 4, &[])];
    assert!(anchors(&symbols).is_empty());
}

#[test]
fn a_constant_declared_in_a_type_is_an_anchor_and_one_in_a_method_is_not() {
    let symbols = vec![
        symbol("Order", SymbolKind::Class, 0, 6, &[]),
        symbol("Limit", SymbolKind::Constant, 1, 20, &["Order"]),
        symbol("Compute", SymbolKind::Function, 2, 15, &["Order"]),
        symbol("Step", SymbolKind::Constant, 3, 18, &["Order", "Compute"]),
    ];

    let names: Vec<String> = anchors(&symbols).into_iter().map(|a| a.name).collect();
    assert_eq!(names, vec!["Order", "Limit", "Compute"]);
}

#[test]
fn an_anchor_aims_at_the_selection_range_and_draws_on_the_declaration_line() {
    // A `references` request aimed at the declaration's start hits `public`, an
    // attribute or a brace, and answers nothing.
    let mut compute = symbol("Compute", SymbolKind::Function, 9, 24, &[]);
    // Attributes and a doc comment push the identifier below the declaration.
    compute.range.start.line = 6;
    compute.range.start.character = 4;

    let anchor = anchors(&[compute]).remove(0);

    assert_eq!(anchor.line, 7, "the row draws at the declaration, 1-based");
    assert_eq!(
        anchor.selection_line, 10,
        "the identifier's own line, 1-based"
    );
    assert_eq!(anchor.character, 24, "0-based UTF-16, where to aim");
}

#[test]
fn an_anchor_name_drops_the_signature_roslyn_puts_in_it() {
    let symbols = vec![
        symbol(
            "TryGetElements(ClrObject, IReadOnlyList<ClrObject>, int) : bool",
            SymbolKind::Function,
            0,
            4,
            &[],
        ),
        symbol("Total : int", SymbolKind::Class, 1, 4, &[]),
        symbol("Cache<TKey, TValue>", SymbolKind::Class, 2, 4, &[]),
    ];

    let names: Vec<String> = anchors(&symbols).into_iter().map(|a| a.name).collect();
    assert_eq!(
        names,
        vec!["TryGetElements", "Total", "Cache<TKey, TValue>"]
    );
}

#[test]
fn two_same_named_overloads_on_one_line_get_distinct_ids() {
    // Same name, same line, same kind — only the column tells them apart, and a
    // UI keying a widget on a shared id would draw one row for two methods.
    let symbols = vec![
        symbol("Add", SymbolKind::Function, 4, 16, &["Order"]),
        symbol("Add", SymbolKind::Function, 4, 40, &["Order"]),
        symbol("Order", SymbolKind::Class, 0, 6, &[]),
    ];

    let ids: Vec<String> = anchors(&symbols).into_iter().map(|a| a.id).collect();
    // The literal ids, not merely distinct ones. Distinctness alone holds with
    // the selection *column* dropped from the id too — the duplicate-occurrence
    // counter would then separate them as `Order.Add@4` and `Order.Add@4#1` — and
    // that id is a function of position in the answer rather than of the
    // declaration, so two overloads listed in the other order next time swap
    // owners and the inline count is drawn against the wrong method.
    assert_eq!(
        ids,
        vec!["Order.Add@4:16", "Order.Add@4:40", "Order@0:6"],
        "the column is what separates two overloads on one line"
    );
    assert!(
        ids.iter().all(|id| !id.contains('#')),
        "distinctness must come from the column, not from the occurrence \
         counter: {ids:?}"
    );
}

#[test]
fn an_id_is_the_same_on_a_second_call_for_the_same_declaration() {
    let symbols = vec![
        symbol("Order", SymbolKind::Class, 0, 6, &[]),
        symbol("Total", SymbolKind::Variable, 2, 15, &["Order"]),
    ];

    let first: Vec<String> = anchors(&symbols).into_iter().map(|a| a.id).collect();
    let second: Vec<String> = anchors(&symbols).into_iter().map(|a| a.id).collect();
    assert_eq!(first, second);
    // And a declaration's id does not depend on what else is in the file: an
    // added sibling must not renumber it.
    let mut more = symbols.clone();
    more.insert(1, symbol("Other", SymbolKind::Class, 1, 6, &[]));
    let third: Vec<String> = anchors(&more).into_iter().map(|a| a.id).collect();
    assert!(
        third.contains(&first[1]),
        "{third:?} should still contain {:?}",
        first[1]
    );
}

#[test]
fn two_identical_declarations_still_get_distinct_ids() {
    // Nothing legal produces this, but a server may repeat a symbol, and two
    // rows sharing an id is a UI defect rather than an honest duplicate.
    let one = symbol("Add", SymbolKind::Function, 4, 16, &["Order"]);
    let ids: Vec<String> = anchors(&[one.clone(), one])
        .into_iter()
        .map(|a| a.id)
        .collect();
    assert_eq!(ids.len(), 2);
    assert_ne!(ids[0], ids[1]);
}

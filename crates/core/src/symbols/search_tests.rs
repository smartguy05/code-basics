//! Tests for querying the index behind a symbol palette.
//! Included by `search.rs` under `#[cfg(test)]`.

use std::path::{Path, PathBuf};

use super::*;
use crate::model::{ConfigSource, RunKind};
use crate::symbols::index::Symbol;

/// An index built by hand rather than by walking a temporary directory.
///
/// [`crate::symbols::index`] already proves that a walk produces these lists;
/// what is under test here is only what happens to them afterwards, and a
/// literal index makes each ranking assertion readable without the reader
/// having to reconstruct a tree of files in their head.
fn index(files: &[&str], symbols: &[(&str, &str, u32)]) -> SymbolIndex {
    SymbolIndex {
        root: PathBuf::from("/ws"),
        files: files.iter().map(PathBuf::from).collect(),
        symbols: symbols
            .iter()
            .map(|(name, path, line)| Symbol {
                name: (*name).to_string(),
                kind: SymbolKind::Function,
                path: PathBuf::from(path),
                line: *line,
                project_id: None,
            })
            .collect(),
        truncated: false,
    }
}

fn config(id: &str, name: &str) -> RunConfig {
    RunConfig::new(id, name, RunKind::App, "dotnet", ConfigSource::Detected)
}

fn query(text: &str) -> Query {
    Query {
        text: text.to_string(),
        scope: SearchScope::All,
        limit: 20,
    }
}

fn labels(hits: &[SearchHit]) -> Vec<&str> {
    hits.iter().map(|h| h.label.as_str()).collect()
}

/// Where a hit for `path` landed in the result, or a panic naming what did come
/// back — an index-of assertion that fails with "None" tells you nothing about
/// why.
fn rank_of(hits: &[SearchHit], path: &str) -> usize {
    hits.iter()
        .position(|h| h.path.as_deref() == Some(Path::new(path)))
        .unwrap_or_else(|| {
            panic!(
                "expected a hit for {path:?}, got {:?}",
                hits.iter().map(|h| (&h.label, h.score)).collect::<Vec<_>>()
            )
        })
}

#[test]
fn a_file_is_ranked_on_its_name_before_its_path() {
    // The pin the whole file-scoring rule exists for. Both paths contain
    // `treelogic` as a subsequence, but only one of them is *named* that, and
    // the named one is the one the user meant.
    let idx = index(
        &[
            "src/components/tree/logic.ts",
            "src/components/treeLogic.ts",
        ],
        &[],
    );

    let hits = search(&idx, &[], &query("treelogic"));

    let named = rank_of(&hits, "src/components/treeLogic.ts");
    let pathed = rank_of(&hits, "src/components/tree/logic.ts");
    assert!(
        named < pathed,
        "the file named treeLogic.ts must outrank the one that only spells it across directories: {:?}",
        hits.iter().map(|h| (&h.detail, h.score)).collect::<Vec<_>>()
    );
}

#[test]
fn a_file_whose_name_does_not_match_is_still_found_by_its_path() {
    // `logic.ts` cannot contain `components`; the whole relative path can. A
    // file that is only reachable this way must still be reachable.
    let idx = index(&["src/components/logic.ts"], &[]);

    let hits = search(&idx, &[], &query("componentslogic"));

    assert_eq!(labels(&hits), ["logic.ts"]);
    assert_eq!(
        hits[0].path.as_deref(),
        Some(Path::new("src/components/logic.ts"))
    );
}

#[test]
fn a_path_fallback_costs_twenty_points_and_carries_no_positions() {
    // Two files with the same name, so the name match is identical and the only
    // difference left is which string the query was scored against.
    let by_name = search(&index(&["a/logic.ts"], &[]), &[], &query("logic"));
    let by_path = search(&index(&["logic/a.ts"], &[]), &[], &query("logic"));

    assert_eq!(by_name[0].score - 20, by_path[0].score);
    assert!(
        !by_name[0].positions.is_empty(),
        "a name match highlights the label"
    );
    assert!(
        by_path[0].positions.is_empty(),
        "a path match must not hand back positions that index into a string the row does not show"
    );
}

#[test]
fn positions_index_into_the_label() {
    let idx = index(&[], &[("GitDiffService", "src/git.rs", 12)]);

    let hits = search(&idx, &[], &query("GDS"));

    let label: Vec<char> = hits[0].label.chars().collect();
    let matched: String = hits[0]
        .positions
        .iter()
        .map(|&i| label[i as usize])
        .collect();
    assert_eq!(matched, "GDS");
}

#[test]
fn an_action_matches_its_configuration_name() {
    let configs = [config("c1", "Run Api"), config("c2", "Watch Web")];

    let hits = search(&index(&[], &[]), &configs, &query("runapi"));

    assert_eq!(labels(&hits), ["Run Api"]);
    assert_eq!(hits[0].kind, HitKind::Action);
    assert_eq!(hits[0].action_id.as_deref(), Some("c1"));
    assert_eq!(hits[0].path, None);
}

#[test]
fn each_scope_filters_to_its_own_kind() {
    let idx = index(&["src/thing.ts"], &[("thing", "src/thing.ts", 3)]);
    let configs = [config("c1", "thing")];

    let kinds = |scope: SearchScope| -> Vec<HitKind> {
        let q = Query {
            text: "thing".to_string(),
            scope,
            limit: 20,
        };
        let mut kinds: Vec<HitKind> = search(&idx, &configs, &q).iter().map(|h| h.kind).collect();
        kinds.sort_by_key(|k| format!("{k:?}"));
        kinds.dedup();
        kinds
    };

    assert_eq!(
        kinds(SearchScope::All),
        [HitKind::Action, HitKind::File, HitKind::Symbol]
    );
    assert_eq!(kinds(SearchScope::Files), [HitKind::File]);
    assert_eq!(kinds(SearchScope::Symbols), [HitKind::Symbol]);
    assert_eq!(kinds(SearchScope::Actions), [HitKind::Action]);
}

#[test]
fn the_limit_is_honoured() {
    let idx = index(
        &["thing1.ts", "thing2.ts", "thing3.ts", "thing4.ts"],
        &[("thing", "a.ts", 1), ("thing", "b.ts", 1)],
    );
    let configs = [config("c1", "thing")];

    for limit in [0usize, 1, 3] {
        let q = Query {
            text: "thing".to_string(),
            scope: SearchScope::All,
            limit,
        };
        assert_eq!(search(&idx, &configs, &q).len(), limit);
    }
}

#[test]
fn ties_come_out_in_the_same_order_every_run() {
    // Four candidates that are indistinguishable on score: the palette re-ranks
    // on every keystroke, so a list that reshuffles under an unchanged query is
    // unusable even when every row in it is correct.
    let idx = index(
        &["a/thing.ts", "b/thing.ts"],
        &[("thing", "a/thing.ts", 1), ("thing", "b/thing.ts", 1)],
    );
    let configs = [config("c1", "thing"), config("c2", "thing")];

    let first = search(&idx, &configs, &query("thing"));
    assert_eq!(
        first.len(),
        6,
        "every candidate matches: {:?}",
        labels(&first)
    );
    for _ in 0..20 {
        assert_eq!(search(&idx, &configs, &query("thing")), first);
    }
}

#[test]
fn a_trailing_line_number_is_parsed_off_the_query() {
    // Parsed here, in Rust, so that the palette's front end has no reason to
    // re-implement it when it is written.
    let idx = index(&["src/Foo.cs"], &[]);

    let hits = search(&idx, &[], &query("Foo:123"));

    assert_eq!(labels(&hits), ["Foo.cs"]);
    assert_eq!(hits[0].line, Some(123));
}

#[test]
fn an_explicit_line_overrides_the_declaration_line() {
    let idx = index(&[], &[("Foo", "src/Foo.cs", 12)]);

    let hits = search(&idx, &[], &query("Foo:99"));

    assert_eq!(hits[0].line, Some(99));
}

#[test]
fn a_trailing_colon_with_no_digits_is_an_unfinished_line_reference() {
    // `Foo:` is what `Foo:12` looks like halfway through being typed, so it
    // searches for `Foo` with no line rather than for the literal text `Foo:`,
    // which no file or symbol could ever contain.
    let idx = index(&["src/Foo.cs"], &[]);

    let hits = search(&idx, &[], &query("Foo:"));

    assert_eq!(labels(&hits), ["Foo.cs"]);
    assert_eq!(hits[0].line, None);
}

#[test]
fn a_colon_followed_by_text_is_not_a_line_reference() {
    // `:abc` is not a line number and is not the beginning of one, so the
    // colon stays part of the text being searched for. Abstaining is the point:
    // silently dropping a suffix that was never a line reference would search
    // for something the user did not type.
    let idx = index(&["src/Foo.cs"], &[]);

    let hits = search(&idx, &[], &query("Foo:abc"));

    assert!(
        hits.is_empty(),
        "expected `Foo:abc` to be searched literally, got {:?}",
        labels(&hits)
    );
    // The control: the same index does answer the query with the suffix gone,
    // so the emptiness above is the colon being kept and not the search being
    // broken.
    assert_eq!(labels(&search(&idx, &[], &query("Foo"))), ["Foo.cs"]);
}

#[test]
fn a_zero_line_number_is_dropped_rather_than_treated_as_line_one() {
    // A gutter counts from one, so `:0` names no line. Dropping it back to
    // "no line given" keeps the text answerable; snapping it to line one would
    // send the editor somewhere the user did not ask for.
    let idx = index(&["src/Foo.cs"], &[]);

    let hits = search(&idx, &[], &query("Foo:0"));

    assert_eq!(labels(&hits), ["Foo.cs"]);
    assert_eq!(hits[0].line, None);
}

#[test]
fn a_line_number_too_large_to_be_a_line_still_finds_the_file() {
    // A pasted or mistyped line number is all digits and unmistakably a line
    // reference; it is just one no file could reach. Keeping the digits as
    // literal text would empty the palette and make the file look missing, so
    // the unusable part is discarded and the name is still answered.
    let idx = index(&["src/Foo.cs"], &[]);

    let hits = search(&idx, &[], &query("Foo:5000000000"));

    assert_eq!(
        labels(&hits),
        ["Foo.cs"],
        "an out-of-range line must degrade to `Foo`, not empty the list"
    );
    assert_eq!(hits[0].line, None);
}

#[test]
fn every_trailing_colon_form_resolves_the_same_way_on_every_run() {
    // The whole table in one place, because the cases only make sense against
    // each other: all-digit suffixes are line references and are consumed even
    // when the number is unusable, and anything else is text.
    assert_eq!(split_line_suffix("Foo:123"), ("Foo", Some(123)));
    assert_eq!(split_line_suffix("Foo:0"), ("Foo", None));
    assert_eq!(split_line_suffix("Foo:5000000000"), ("Foo", None));
    assert_eq!(split_line_suffix("Foo:"), ("Foo", None));
    assert_eq!(split_line_suffix("Foo:abc"), ("Foo:abc", None));
    // A colon with nothing before it names a line in no particular file.
    assert_eq!(split_line_suffix(":42"), (":42", None));
}

#[test]
fn a_query_matching_nothing_returns_an_empty_vec() {
    let idx = index(&["src/Foo.cs"], &[("Foo", "src/Foo.cs", 1)]);
    let configs = [config("c1", "Run Api")];

    assert!(search(&idx, &configs, &query("zzzzzz")).is_empty());
    // Nothing matched because nothing matches, not because the index was empty.
    assert_eq!(search(&idx, &configs, &query("foo")).len(), 2);
}

#[test]
fn search_hit_serialises_with_the_keys_the_ui_reads() {
    // Written a phase before the counterparty existed, to fix the shape the
    // mirror would be written against. Both counterparties now exist —
    // `search_everywhere` returns this type and `SearchHit` in
    // `src/ipc/types.ts` mirrors it by hand — so this is a live contract, and
    // a rename here that is not carried into that file surfaces as `undefined`
    // in a palette row rather than as any kind of error. Every key is present
    // on every hit — `null` where a kind has no answer — so a row can never
    // confuse "this kind has no line" with "the backend forgot to send one".
    let hits = search(&index(&["src/Foo.cs"], &[]), &[], &query("foo"));
    let hit = hits.first().expect("expected a hit to serialise");

    let json = serde_json::to_value(hit).unwrap();
    let object = json.as_object().expect("a SearchHit is a JSON object");
    let mut keys: Vec<String> = object.keys().cloned().collect();
    keys.sort();

    assert_eq!(
        keys,
        [
            "actionId",
            "detail",
            "kind",
            "label",
            "line",
            "path",
            "positions",
            "score",
            "symbolKind",
        ],
        "SearchHit's JSON keys changed — this shape is the palette's wire contract, so change it deliberately and carry the change into the hand-written mirror in src/ipc/types.ts in the same commit"
    );
    assert!(
        object["line"].is_null(),
        "an inapplicable field must cross as null, not vanish — the mirror types these as `T | null` for exactly this reason, and a missing key cannot be told from a kind that has no answer"
    );
}

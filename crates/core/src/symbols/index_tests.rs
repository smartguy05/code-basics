//! Tests for building the index of what a workspace declares.
//! Included by `index.rs` under `#[cfg(test)]`.

use std::fs;
use std::path::{Path, PathBuf};

use super::*;
use crate::model::{Project, ProjectKind};

/// A project rooted at `dir`, with only the fields the index actually reads
/// filled in meaningfully. The index looks at `id` and `dir` and nothing else,
/// which is the whole reason [`build`] takes `&[Project]` instead of a
/// `Workspace`.
fn project(root: &Path, id: &str, dir: &str) -> Project {
    let dir = root.join(dir);
    Project {
        id: id.to_string(),
        name: id.to_string(),
        manifest_path: dir.join(format!("{id}.csproj")),
        dir,
        ecosystem: "dotnet".to_string(),
        kind: ProjectKind::Library,
        frameworks: vec![],
        configurations: vec![],
        is_test_project: false,
        test_runner: None,
        unreadable: None,
    }
}

fn write(root: &Path, relative: &str, contents: &str) -> PathBuf {
    let path = root.join(relative);
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(&path, contents).unwrap();
    path
}

fn write_bytes(root: &Path, relative: &str, contents: &[u8]) {
    let path = root.join(relative);
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(&path, contents).unwrap();
}

fn names(index: &SymbolIndex) -> Vec<&str> {
    index.symbols.iter().map(|s| s.name.as_str()).collect()
}

fn files(index: &SymbolIndex) -> Vec<String> {
    index
        .files
        .iter()
        .map(|f| f.to_string_lossy().replace('\\', "/"))
        .collect()
}

#[test]
fn a_declaration_is_indexed_with_its_file_line_and_project() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    write(root, "src/App/App.csproj", "<Project />");
    write(
        root,
        "src/App/Service.cs",
        "namespace App;\n\n\
         public class GitDiffService\n\
         {\n}\n",
    );

    let index = build(root, &[project(root, "App", "src/App")]);

    let service = index
        .symbols
        .iter()
        .find(|s| s.name == "GitDiffService")
        .expect("expected the class to be indexed");
    assert_eq!(
        service,
        &Symbol {
            name: "GitDiffService".to_string(),
            kind: SymbolKind::Class,
            path: PathBuf::from("src/App/Service.cs"),
            line: 3,
            project_id: Some("App".to_string()),
        }
    );
    assert!(!index.truncated);
}

#[test]
fn a_file_with_no_parsable_extension_is_listed_but_yields_no_symbols() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    write(
        root,
        "README.md",
        "# Title\n\npublic class NotACSharpClass\n",
    );
    write(root, "src/lib.rs", "pub fn real_symbol() {}\n");

    let index = build(root, &[]);

    assert!(
        files(&index).contains(&"README.md".to_string()),
        "go-to-file must still see it: {:?}",
        files(&index)
    );
    assert_eq!(names(&index), ["real_symbol"]);
}

#[test]
fn an_oversized_file_is_listed_but_yields_no_symbols() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let mut generated = String::from("pub fn generated_symbol() {}\n");
    while generated.len() as u64 <= MAX_INDEXED_BYTES {
        generated.push_str("// padding padding padding padding padding padding\n");
    }
    write(root, "src/generated.rs", &generated);
    write(root, "src/hand_written.rs", "pub fn hand_written() {}\n");

    let index = build(root, &[]);

    assert!(files(&index).contains(&"src/generated.rs".to_string()));
    assert_eq!(names(&index), ["hand_written"]);
}

#[test]
fn an_oversized_file_marks_the_index_as_truncated() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let mut generated = String::from("pub fn generated_symbol() {}\n");
    while generated.len() as u64 <= MAX_INDEXED_BYTES {
        generated.push_str("// padding padding padding padding padding padding\n");
    }
    write(root, "src/generated.rs", &generated);
    write(root, "src/hand_written.rs", "pub fn hand_written() {}\n");

    let index = build(root, &[]);

    assert!(
        index.truncated,
        "the byte cap dropped a file's symbols, so the index is not complete and must say so"
    );
}

#[test]
fn a_file_below_the_byte_cap_leaves_the_index_whole() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    // An empty vector from a file that genuinely declares nothing is the honest
    // complete answer, and must not be confused with the skip above.
    write(root, "src/silent.rs", "// nothing declared here\n");
    write(root, "src/hand_written.rs", "pub fn hand_written() {}\n");

    let index = build(root, &[]);

    assert!(!index.truncated);
    assert_eq!(names(&index), ["hand_written"]);
}

#[test]
fn a_file_carrying_a_nul_byte_is_listed_but_yields_no_symbols() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let mut bytes = b"pub fn looks_like_source() {}\n".to_vec();
    bytes.extend_from_slice(&[0x00, 0x01, 0x02, 0x03]);
    write_bytes(root, "src/blob.rs", &bytes);

    let index = build(root, &[]);

    assert!(files(&index).contains(&"src/blob.rs".to_string()));
    assert!(index.symbols.is_empty(), "{:?}", names(&index));
}

#[test]
fn a_file_that_is_not_utf8_is_listed_but_yields_no_symbols() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    // Latin-1 `é` (0xE9) is not a valid UTF-8 sequence on its own.
    let bytes = b"pub fn caf\xE9() {}\n".to_vec();
    write_bytes(root, "src/latin1.rs", &bytes);

    let index = build(root, &[]);

    assert!(files(&index).contains(&"src/latin1.rs".to_string()));
    assert!(index.symbols.is_empty(), "{:?}", names(&index));
}

#[test]
fn an_index_that_reaches_the_file_cap_stops_and_says_so() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    for i in 0..6 {
        write(
            root,
            &format!("src/f{i}.rs"),
            &format!("pub fn f{i}() {{}}\n"),
        );
    }

    let index = build_with(
        root,
        &[],
        Limits {
            max_files: 3,
            ..Limits::default()
        },
    );

    assert!(index.truncated, "a clipped index must admit it is clipped");
    assert_eq!(index.files.len(), 3);
}

#[test]
fn an_index_that_reaches_the_symbol_cap_stops_and_says_so() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    write(
        root,
        "src/many.rs",
        "pub fn a() {}\npub fn b() {}\npub fn c() {}\npub fn d() {}\n",
    );

    let index = build_with(
        root,
        &[],
        Limits {
            max_symbols: 2,
            ..Limits::default()
        },
    );

    assert!(index.truncated);
    assert_eq!(names(&index), ["a", "b"]);
}

#[test]
fn a_nested_project_wins_over_the_one_that_contains_it() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    write(root, "src/Outer/Outer.csproj", "<Project />");
    write(root, "src/Outer/Inner/Inner.csproj", "<Project />");
    write(root, "src/Outer/Inner/Thing.cs", "public class Thing {}\n");

    let index = build_with(
        root,
        &[
            project(root, "Outer", "src/Outer"),
            project(root, "Inner", "src/Outer/Inner"),
        ],
        Limits::default(),
    );

    let thing = index
        .symbols
        .iter()
        .find(|s| s.name == "Thing")
        .expect("expected the class to be indexed");
    assert_eq!(thing.project_id.as_deref(), Some("Inner"));
}

#[test]
fn a_file_under_no_project_has_no_project_id() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    write(root, "src/App/App.csproj", "<Project />");
    write(root, "scripts/tool.ts", "export function loose() {}\n");

    let index = build(root, &[project(root, "App", "src/App")]);

    let loose = index
        .symbols
        .iter()
        .find(|s| s.name == "loose")
        .expect("expected the function to be indexed");
    assert_eq!(loose.project_id, None);
}

#[test]
fn symbols_come_out_ordered_by_path_then_line_then_name() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    write(root, "src/b.rs", "pub fn zeta() {}\npub fn alpha() {}\n");
    write(root, "src/a.rs", "pub fn middle() {}\n");

    let index = build(root, &[]);

    assert_eq!(names(&index), ["middle", "zeta", "alpha"]);
}

#[test]
fn an_empty_workspace_produces_an_empty_index_rather_than_an_error() {
    let dir = tempfile::tempdir().unwrap();

    let index = build(dir.path(), &[]);

    assert!(index.files.is_empty());
    assert!(index.symbols.is_empty());
    assert!(!index.truncated);
    assert_eq!(index.root, dir.path());
}

#[test]
fn an_indented_local_binding_is_excluded_but_a_top_level_one_is_kept() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    write(
        root,
        "src/app.ts",
        "export const RETRY_LIMIT = 3;\n\
         export function run() {\n\
         \x20 const scratch = new Map();\n\
         \x20 let cursor = 0;\n\
         \x20 var legacy = null;\n\
         \x20 return scratch.size + cursor;\n\
         }\n",
    );

    let index = build(root, &[]);

    assert_eq!(names(&index), ["RETRY_LIMIT", "run"]);
}

#[test]
fn a_workspace_builds_identically_across_repeated_runs() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    for i in 0..40 {
        write(
            root,
            &format!("src/mod{i}/thing.rs"),
            &format!("pub struct Thing{i} {{}}\npub fn make{i}() {{}}\n"),
        );
        write(
            root,
            &format!("src/mod{i}/view.tsx"),
            &format!("export function View{i}() {{}}\n"),
        );
    }

    let first = build(root, &[]);
    assert_eq!(first.symbols.len(), 120);
    for _ in 0..9 {
        assert_eq!(build(root, &[]), first);
    }
}

#[test]
fn a_single_file_is_reindexed_without_walking_the_workspace() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    write(root, "src/App/App.csproj", "<Project />");
    write(root, "src/App/Service.cs", "public class Service {}\n");
    write(
        root,
        "src/App/bin/Debug/Service.cs",
        "public class Copy {}\n",
    );

    let projects = [project(root, "App", "src/App")];

    let symbols = index_file(root, Path::new("src/App/Service.cs"), &projects);
    assert_eq!(symbols.len(), 1);
    assert_eq!(symbols[0].name, "Service");
    assert_eq!(symbols[0].project_id.as_deref(), Some("App"));
    assert_eq!(symbols[0].path, PathBuf::from("src/App/Service.cs"));

    // Build output is excluded by the walk, so it has to be excluded here too
    // or a save inside `bin/` would smuggle a symbol into the index that a
    // rebuild would then drop.
    assert!(index_file(root, Path::new("src/App/bin/Debug/Service.cs"), &projects).is_empty());
}

#[test]
fn the_walk_skips_build_output_and_nested_checkouts() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    write(root, "src/kept.rs", "pub fn kept() {}\n");
    write(root, "src/obj/Debug/dropped.rs", "pub fn dropped() {}\n");
    write(
        root,
        "node_modules/pkg/dropped.js",
        "export function gone() {}\n",
    );
    write(root, "vendor/other/.git", "gitdir: elsewhere\n");
    write(root, "vendor/other/dropped.rs", "pub fn vendored() {}\n");

    let index = build(root, &[]);

    assert_eq!(names(&index), ["kept"]);
    assert_eq!(files(&index), ["src/kept.rs"]);
}

#[test]
fn a_source_file_nested_deeper_than_ten_levels_is_indexed() {
    // A conventional layout is shallow, but monorepos and generated trees are
    // not: a file fourteen directories down is still source, and the walk must
    // reach it. Depth 15 here (root is depth 0) is comfortably past the old
    // limit of 10 and well short of anything that could stall a scan.
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let nested = "d01/d02/d03/d04/d05/d06/d07/d08/d09/d10/d11/d12/d13/d14/deep.rs";
    write(root, nested, "pub fn buried() {}\n");

    let index = build(root, &[]);

    assert!(
        files(&index).iter().any(|f| f.ends_with("deep.rs")),
        "deeply nested file must be indexed: {:?}",
        files(&index)
    );
    assert!(
        names(&index).contains(&"buried"),
        "its symbol must be indexed: {:?}",
        names(&index)
    );
}

/// Pinned before the counterparty exists, on the same reasoning as
/// `search_hit_serialises_with_the_keys_the_ui_reads`: there is no TypeScript
/// mirror of `Symbol` or `SymbolIndex` yet and no command returning one, so
/// this fixes the shape while it is still cheap to decide rather than guards a
/// contract already in use.
#[test]
fn a_symbol_index_serialises_with_the_keys_the_ui_reads() {
    let symbol = Symbol {
        name: "Thing".to_string(),
        kind: SymbolKind::Class,
        path: PathBuf::from("src/Thing.cs"),
        line: 1,
        project_id: None,
    };
    let json = serde_json::to_value(SymbolIndex {
        root: PathBuf::from("/w"),
        files: vec![PathBuf::from("src/Thing.cs")],
        symbols: vec![symbol],
        truncated: false,
    })
    .unwrap();

    let mut index_keys: Vec<&str> = json.as_object().unwrap().keys().map(|k| &**k).collect();
    index_keys.sort_unstable();
    assert_eq!(index_keys, ["files", "root", "symbols", "truncated"]);

    let mut symbol_keys: Vec<&str> = json["symbols"][0]
        .as_object()
        .unwrap()
        .keys()
        .map(|k| &**k)
        .collect();
    symbol_keys.sort_unstable();
    assert_eq!(symbol_keys, ["kind", "line", "name", "path", "projectId"]);
    assert_eq!(json["symbols"][0]["kind"], "class");
}

// ---------------------------------------------------------------------------
// SymbolIndexStatus
// ---------------------------------------------------------------------------

#[test]
fn a_symbol_index_status_serialises_with_the_keys_the_ui_reads() {
    let json = serde_json::to_value(SymbolIndexStatus {
        ready: true,
        building: false,
        files: 2,
        symbols: 7,
        truncated: false,
    })
    .unwrap();

    let mut keys: Vec<&str> = json.as_object().unwrap().keys().map(|k| &**k).collect();
    keys.sort_unstable();
    assert_eq!(
        keys,
        ["building", "files", "ready", "symbols", "truncated"],
        "SymbolIndexStatus is mirrored by hand in src/ipc/types.ts; update it in the same change"
    );
}

#[test]
fn a_status_with_no_index_is_not_ready_and_counts_nothing() {
    let status = SymbolIndexStatus::of(None, true);

    assert!(!status.ready);
    assert!(status.building);
    assert_eq!(status.files, 0);
    assert_eq!(status.symbols, 0);
    assert!(!status.truncated);
}

#[test]
fn a_status_built_from_an_index_reports_its_counts_and_truncation() {
    let index = SymbolIndex {
        root: PathBuf::from("/w"),
        files: vec![PathBuf::from("a.rs"), PathBuf::from("b.rs")],
        symbols: vec![Symbol {
            name: "Thing".into(),
            kind: SymbolKind::Class,
            path: PathBuf::from("a.rs"),
            line: 1,
            project_id: None,
        }],
        truncated: true,
    };

    let status = SymbolIndexStatus::of(Some(&index), false);

    assert!(status.ready);
    assert!(!status.building);
    assert_eq!(status.files, 2);
    assert_eq!(status.symbols, 1);
    assert!(status.truncated);
}

#[test]
fn an_index_that_exists_is_ready_even_while_another_build_is_running() {
    // A rebuild does not make the index that is already there unusable, and
    // reporting `ready: false` for the duration would blank a working palette.
    let index = SymbolIndex {
        root: PathBuf::from("/w"),
        files: Vec::new(),
        symbols: Vec::new(),
        truncated: false,
    };

    let status = SymbolIndexStatus::of(Some(&index), true);

    assert!(status.ready);
    assert!(status.building);
}

// ---------------------------------------------------------------------------
// replace_file
// ---------------------------------------------------------------------------

fn symbol(name: &str, path: &str, line: u32) -> Symbol {
    Symbol {
        name: name.to_string(),
        kind: SymbolKind::Function,
        path: PathBuf::from(path),
        line,
        project_id: None,
    }
}

fn small_index() -> SymbolIndex {
    SymbolIndex {
        root: PathBuf::from("/w"),
        files: vec![PathBuf::from("a.rs"), PathBuf::from("b.rs")],
        symbols: vec![symbol("alpha", "a.rs", 1), symbol("beta", "b.rs", 1)],
        truncated: false,
    }
}

#[test]
fn replacing_a_files_symbols_drops_the_ones_it_no_longer_declares() {
    let mut index = small_index();

    replace_file(
        &mut index,
        Path::new("a.rs"),
        vec![symbol("renamed", "a.rs", 4)],
    );

    assert_eq!(names(&index), ["renamed", "beta"]);
    assert_eq!(index.symbols[0].line, 4);
}

#[test]
fn replacing_a_files_symbols_leaves_every_other_file_alone() {
    let mut index = small_index();

    replace_file(&mut index, Path::new("a.rs"), Vec::new());

    assert_eq!(names(&index), ["beta"]);
    assert_eq!(files(&index), ["a.rs", "b.rs"]);
}

#[test]
fn replacing_a_files_symbols_keeps_the_index_sorted_by_path_then_line() {
    let mut index = small_index();

    replace_file(
        &mut index,
        Path::new("b.rs"),
        vec![symbol("zeta", "b.rs", 9), symbol("gamma", "b.rs", 2)],
    );

    let ordered: Vec<(&str, u32)> = index
        .symbols
        .iter()
        .map(|s| (s.name.as_str(), s.line))
        .collect();
    assert_eq!(ordered, [("alpha", 1), ("gamma", 2), ("zeta", 9)]);
}

/// The same two-file index, but rooted somewhere that really exists, so the
/// on-disk check in [`replace_file`] is exercised against real files rather
/// than against a root nobody could `stat`.
fn index_rooted_at(root: &Path) -> SymbolIndex {
    SymbolIndex {
        root: root.to_path_buf(),
        ..small_index()
    }
}

#[test]
fn saving_a_file_the_index_had_never_seen_adds_it_to_the_file_list() {
    // Creating a file and saving it must make it findable immediately; leaving
    // it out until the next full build is exactly the staleness this exists to
    // prevent.
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    write(root, "src/new.rs", "pub fn fresh() {}\n");
    let mut index = index_rooted_at(root);

    replace_file(
        &mut index,
        Path::new("src/new.rs"),
        vec![symbol("fresh", "src/new.rs", 3)],
    );

    assert_eq!(files(&index), ["a.rs", "b.rs", "src/new.rs"]);
    assert_eq!(names(&index), ["alpha", "beta", "fresh"]);
}

#[test]
fn a_save_whose_path_names_nothing_on_disk_does_not_join_the_file_list() {
    // The path is well formed and passes every lexical check, so nothing but a
    // look at the disk can tell it apart from a file that was just created.
    // Admitting it puts a row in the palette that opens nothing at all, which
    // is the wrong answer this module exists to refuse.
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let mut index = index_rooted_at(root);

    replace_file(&mut index, Path::new("src/Api/Program.cs"), Vec::new());

    assert_eq!(files(&index), ["a.rs", "b.rs"]);
    assert_eq!(names(&index), ["alpha", "beta"]);
}

#[test]
fn a_directory_saved_over_is_not_offered_as_a_file() {
    // `is_file` rather than `exists`: a directory is on disk and is not
    // openable, and the walk only ever recorded files.
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    fs::create_dir_all(root.join("src/nested")).unwrap();
    let mut index = index_rooted_at(root);

    replace_file(&mut index, Path::new("src/nested"), Vec::new());

    assert_eq!(files(&index), ["a.rs", "b.rs"]);
}

#[test]
fn a_path_that_escapes_the_root_changes_nothing_even_when_it_names_a_real_file() {
    // Existence is not the gate that refuses these — the escape is. A path that
    // climbs out of the workspace, or names an absolute location, describes a
    // file the walk could never have produced, and a symbol recorded under such
    // a path cannot be resolved by anything downstream.
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("w");
    fs::create_dir_all(&root).unwrap();
    write(dir.path(), "outside.rs", "pub fn outside() {}\n");
    let mut index = index_rooted_at(&root);

    replace_file(
        &mut index,
        Path::new("../outside.rs"),
        vec![symbol("outside", "../outside.rs", 1)],
    );
    replace_file(
        &mut index,
        &dir.path().join("outside.rs"),
        vec![symbol("outside", "outside.rs", 1)],
    );

    assert_eq!(names(&index), ["alpha", "beta"]);
    assert_eq!(files(&index), ["a.rs", "b.rs"]);
}

// ---------------------------------------------------------------------------
// relative_to_root
// ---------------------------------------------------------------------------

#[test]
fn a_file_saved_inside_the_root_is_keyed_by_the_path_below_it() {
    let key = relative_to_root(Path::new("/w"), Path::new("/w/src/App/Service.cs"));

    assert_eq!(key, Some(PathBuf::from("src/App/Service.cs")));
}

#[test]
fn a_file_named_from_a_repository_above_the_workspace_is_keyed_from_the_workspace_root() {
    // The Changes tab's paths are relative to the repository, which
    // `Repository::discover` may find above the opened workspace. Joining such
    // a path onto the repository root and re-keying it here is the only way the
    // edited file's own entry gets refreshed.
    let key = relative_to_root(
        Path::new("/repo/src/Api"),
        Path::new("/repo/src/Api/Program.cs"),
    );

    assert_eq!(key, Some(PathBuf::from("Program.cs")));
}

#[test]
fn a_root_spelled_with_a_trailing_separator_keys_the_same_file() {
    // libgit2 hands back a working directory with a trailing separator, and the
    // workspace root has none. They must key the same file.
    let key = relative_to_root(Path::new("/w/"), Path::new("/w/src/a.rs"));

    assert_eq!(key, Some(PathBuf::from("src/a.rs")));
}

#[test]
fn a_file_outside_the_workspace_root_has_no_key() {
    // A repository wider than the workspace contains files the workspace does
    // not. Keying one of them relative to a root it is not under would put a
    // path in the index that resolves to somebody else's file, so there is no
    // key at all.
    assert_eq!(
        relative_to_root(
            Path::new("/repo/src/Api"),
            Path::new("/repo/src/Web/Program.cs")
        ),
        None
    );
    assert_eq!(
        relative_to_root(Path::new("/repo/src/Api"), Path::new("/elsewhere/a.rs")),
        None
    );
}

#[test]
fn the_root_itself_has_no_key() {
    // Stripping the root from itself leaves an empty path, which names no file
    // and would sort ahead of every real entry in the list.
    assert_eq!(relative_to_root(Path::new("/w"), Path::new("/w")), None);
}

#[test]
fn a_replacement_carrying_another_files_path_is_dropped_rather_than_misplaced() {
    // The vector is one file's declarations by construction — `index_file`
    // stamps every entry with the path it was asked about. An entry naming a
    // different file is not something this call was asked to say anything
    // about: it would be filed under a file whose own next save would not
    // remove it, and it would land at the wrong place in an order the rest of
    // the module relies on being correct.
    let mut index = small_index();

    replace_file(
        &mut index,
        Path::new("a.rs"),
        vec![symbol("mine", "a.rs", 1), symbol("stowaway", "b.rs", 2)],
    );

    assert_eq!(names(&index), ["mine", "beta"]);
}

#[test]
fn a_file_replaced_in_the_middle_of_the_index_leaves_it_ordered() {
    // The replacement is spliced into the run the file already occupies rather
    // than appended and re-sorted, so the ordering has to be checked over
    // neighbours on both sides, not just within the one file.
    let mut index = SymbolIndex {
        root: PathBuf::from("/w"),
        files: vec![
            PathBuf::from("a.rs"),
            PathBuf::from("m.rs"),
            PathBuf::from("z.rs"),
        ],
        symbols: vec![
            symbol("alpha", "a.rs", 1),
            symbol("mid", "m.rs", 1),
            symbol("omega", "z.rs", 1),
        ],
        truncated: false,
    };

    replace_file(
        &mut index,
        Path::new("m.rs"),
        vec![symbol("second", "m.rs", 9), symbol("first", "m.rs", 2)],
    );

    assert_eq!(names(&index), ["alpha", "first", "second", "omega"]);
    let sorted = index
        .symbols
        .windows(2)
        .all(|w| (&w[0].path, w[0].line, &w[0].name) <= (&w[1].path, w[1].line, &w[1].name));
    assert!(sorted, "{:?}", names(&index));
}

#[test]
fn a_windows_shaped_path_replaces_the_same_file_the_walk_recorded() {
    // The walk records forward slashes on every platform; a caller handing in
    // a path the OS spelled must hit the same entry rather than adding a
    // second one under a different spelling.
    let mut index = SymbolIndex {
        root: PathBuf::from("/w"),
        files: vec![PathBuf::from("src/a.rs")],
        symbols: vec![symbol("alpha", "src/a.rs", 1)],
        truncated: false,
    };

    replace_file(
        &mut index,
        Path::new("src\\a.rs"),
        vec![symbol("renamed", "src/a.rs", 1)],
    );

    assert_eq!(names(&index), ["renamed"]);
    assert_eq!(files(&index), ["src/a.rs"]);
}

#[test]
fn a_path_the_walk_would_never_have_produced_changes_nothing() {
    // A save into build output, or above the workspace, must not smuggle a
    // symbol into the index that the next full build would drop.
    let mut index = small_index();

    replace_file(
        &mut index,
        Path::new("obj/Debug/gen.rs"),
        vec![symbol("generated", "obj/Debug/gen.rs", 1)],
    );
    replace_file(
        &mut index,
        Path::new("../outside.rs"),
        vec![symbol("outside", "../outside.rs", 1)],
    );

    assert_eq!(names(&index), ["alpha", "beta"]);
    assert_eq!(files(&index), ["a.rs", "b.rs"]);
}

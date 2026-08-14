//! Tests for persisting a built index and reusing what it can still vouch for.
//! Included by `cache.rs` under `#[cfg(test)]`.

use super::*;
use crate::model::ProjectKind;
use crate::symbols::declarations::SymbolKind;

/// A workspace with one Rust file that declares `real_thing`.
fn workspace() -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("lib.rs"), "pub fn real_thing() {}\n").unwrap();
    dir
}

/// A project rooted at `dir` relative to the workspace root. Only `id` and
/// `dir` are load-bearing here — they are the two fields ownership is decided
/// from — but the struct has no `Default`, so the rest is filled in.
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

/// Every symbol's owning project, in index order.
fn owners(index: &SymbolIndex) -> Vec<Option<String>> {
    index.symbols.iter().map(|s| s.project_id.clone()).collect()
}

/// The symbol a fresh parse of `lib.rs` genuinely produces.
fn real_symbol() -> Symbol {
    Symbol {
        name: "real_thing".into(),
        kind: SymbolKind::Function,
        path: PathBuf::from("lib.rs"),
        line: 1,
        project_id: None,
    }
}

/// A symbol that no parse of `lib.rs` could ever produce. Its presence in a
/// built index is proof — the only available proof — that the file on disk was
/// not read.
fn fabricated() -> Symbol {
    Symbol {
        name: "never_written_by_anyone".into(),
        kind: SymbolKind::Struct,
        path: PathBuf::from("lib.rs"),
        line: 99,
        project_id: None,
    }
}

/// Write a cache claiming `symbols` for `lib.rs`, fingerprinted so that it
/// matches the file as it currently sits on disk.
fn write_matching_cache(root: &Path, symbols: Vec<Symbol>) {
    let (mtime_secs, len) = fingerprint(&root.join("lib.rs")).unwrap();
    save(
        root,
        &SymbolCache {
            version: CACHE_VERSION,
            heuristic_version: HEURISTIC_VERSION,
            projects: vec![],
            entries: vec![CacheEntry {
                path: PathBuf::from("lib.rs"),
                mtime_secs,
                len,
                symbols,
            }],
        },
    )
    .unwrap();
}

fn names(index: &SymbolIndex) -> Vec<String> {
    index.symbols.iter().map(|s| s.name.clone()).collect()
}

// -- shape ---------------------------------------------------------------

#[test]
fn a_cache_round_trips_through_json() {
    let cache = SymbolCache {
        version: CACHE_VERSION,
        heuristic_version: HEURISTIC_VERSION,
        projects: vec![],
        entries: vec![CacheEntry {
            path: PathBuf::from("src/lib.rs"),
            mtime_secs: 1_700_000_000,
            len: 42,
            symbols: vec![real_symbol()],
        }],
    };

    let json = serde_json::to_string(&cache).unwrap();
    let back: SymbolCache = serde_json::from_str(&json).unwrap();

    assert_eq!(back, cache);
}

/// Not the "keys the ui reads" wording the rest of the crate's pinning tests
/// use, because this shape never reaches a UI: `SymbolCache` derives no
/// `specta::Type`, no command returns it, and its only counterparty is `load`
/// reading back what `save` wrote. What is pinned here is the file format.
#[test]
fn a_symbol_cache_serialises_with_the_keys_it_is_read_back_with() {
    let cache = SymbolCache {
        version: 1,
        heuristic_version: 1,
        projects: vec![CachedProject {
            id: "outer".into(),
            dir: PathBuf::from("/ws/src"),
        }],
        entries: vec![CacheEntry {
            path: PathBuf::from("lib.rs"),
            mtime_secs: 7,
            len: 9,
            symbols: vec![],
        }],
    };

    // Sorted, matching the pinning tests in `model.rs`: `serde_json::Value`
    // holds a `BTreeMap` here, so its iteration order is alphabetical and says
    // nothing about the struct.
    let value: serde_json::Value = serde_json::to_value(&cache).unwrap();
    let top: Vec<&str> = value.as_object().unwrap().keys().map(|k| &**k).collect();
    assert_eq!(
        top,
        vec!["entries", "heuristicVersion", "projects", "version"]
    );

    let entry = &value["entries"][0];
    let keys: Vec<&str> = entry.as_object().unwrap().keys().map(|k| &**k).collect();
    assert_eq!(keys, vec!["len", "mtimeSecs", "path", "symbols"]);

    let project = &value["projects"][0];
    let keys: Vec<&str> = project.as_object().unwrap().keys().map(|k| &**k).collect();
    assert_eq!(keys, vec!["dir", "id"]);
}

// -- reuse ---------------------------------------------------------------

/// The whole point of the file. A fabricated symbol can only survive a build
/// if that build never opened `lib.rs`.
#[test]
fn an_unchanged_file_is_reused_rather_than_reparsed() {
    let dir = workspace();
    write_matching_cache(dir.path(), vec![fabricated()]);

    let index = build_cached(dir.path(), &[]);

    assert!(
        names(&index).contains(&"never_written_by_anyone".to_string()),
        "an unchanged file must be served from the cache, not re-read: {:?}",
        names(&index)
    );
    assert!(
        !names(&index).contains(&"real_thing".to_string()),
        "a reused entry must replace the parse, not sit beside it: {:?}",
        names(&index)
    );
}

#[test]
fn a_changed_length_invalidates_that_entry() {
    let dir = workspace();
    let (mtime_secs, len) = fingerprint(&dir.path().join("lib.rs")).unwrap();
    save(
        dir.path(),
        &SymbolCache {
            version: CACHE_VERSION,
            heuristic_version: HEURISTIC_VERSION,
            projects: vec![],
            entries: vec![CacheEntry {
                path: PathBuf::from("lib.rs"),
                mtime_secs,
                len: len + 1,
                symbols: vec![fabricated()],
            }],
        },
    )
    .unwrap();

    let index = build_cached(dir.path(), &[]);

    assert_eq!(
        names(&index),
        vec!["real_thing".to_string()],
        "a length that does not match the file must force a re-parse"
    );
}

#[test]
fn a_changed_mtime_invalidates_that_entry() {
    let dir = workspace();
    let (mtime_secs, len) = fingerprint(&dir.path().join("lib.rs")).unwrap();
    save(
        dir.path(),
        &SymbolCache {
            version: CACHE_VERSION,
            heuristic_version: HEURISTIC_VERSION,
            projects: vec![],
            entries: vec![CacheEntry {
                path: PathBuf::from("lib.rs"),
                mtime_secs: mtime_secs + 1,
                len,
                symbols: vec![fabricated()],
            }],
        },
    )
    .unwrap();

    let index = build_cached(dir.path(), &[]);

    assert_eq!(
        names(&index),
        vec!["real_thing".to_string()],
        "a modification time that does not match the file must force a re-parse"
    );
}

#[test]
fn a_heuristic_version_bump_discards_the_whole_cache() {
    let dir = workspace();
    let (mtime_secs, len) = fingerprint(&dir.path().join("lib.rs")).unwrap();
    save(
        dir.path(),
        &SymbolCache {
            version: CACHE_VERSION,
            heuristic_version: HEURISTIC_VERSION - 1,
            projects: vec![],
            entries: vec![CacheEntry {
                path: PathBuf::from("lib.rs"),
                mtime_secs,
                len,
                symbols: vec![fabricated()],
            }],
        },
    )
    .unwrap();

    let index = build_cached(dir.path(), &[]);

    assert_eq!(
        names(&index),
        vec!["real_thing".to_string()],
        "symbols recorded by a different heuristic must not be trusted, however \
         well their fingerprints match"
    );
}

#[test]
fn an_unrecognised_cache_version_discards_the_whole_cache() {
    let dir = workspace();
    let (mtime_secs, len) = fingerprint(&dir.path().join("lib.rs")).unwrap();
    save(
        dir.path(),
        &SymbolCache {
            version: CACHE_VERSION + 1,
            heuristic_version: HEURISTIC_VERSION,
            projects: vec![],
            entries: vec![CacheEntry {
                path: PathBuf::from("lib.rs"),
                mtime_secs,
                len,
                symbols: vec![fabricated()],
            }],
        },
    )
    .unwrap();

    let index = build_cached(dir.path(), &[]);

    assert_eq!(names(&index), vec!["real_thing".to_string()]);
}

// -- the cache is never fatal --------------------------------------------

#[test]
fn corrupt_json_is_ignored_and_a_full_build_runs() {
    let dir = workspace();
    std::fs::create_dir_all(crate::config::config_dir(dir.path())).unwrap();
    std::fs::write(cache_path(dir.path()), "{ this is not json").unwrap();

    let index = build_cached(dir.path(), &[]);

    assert_eq!(
        names(&index),
        vec!["real_thing".to_string()],
        "an unreadable cache must cost a rebuild, never an error"
    );
}

#[test]
fn a_missing_cache_is_an_ordinary_full_build() {
    let dir = workspace();

    let index = build_cached(dir.path(), &[]);

    assert_eq!(names(&index), vec!["real_thing".to_string()]);
}

// -- files that are no longer there --------------------------------------

#[test]
fn a_deleted_files_entry_is_dropped() {
    let dir = workspace();
    write_matching_cache(dir.path(), vec![fabricated()]);
    std::fs::remove_file(dir.path().join("lib.rs")).unwrap();

    let index = build_cached(dir.path(), &[]);

    assert!(
        index.symbols.is_empty(),
        "a file that is no longer on disk must contribute nothing: {:?}",
        names(&index)
    );
    assert!(
        !index.files.iter().any(|f| f == Path::new("lib.rs")),
        "a deleted file must not be listed either"
    );
}

/// A stale entry naming something the walk excludes must not sneak back in
/// through the cache. The walk is the authority on what the index contains.
#[test]
fn an_entry_the_walk_would_never_produce_is_dropped() {
    let dir = workspace();
    std::fs::create_dir_all(dir.path().join("bin")).unwrap();
    std::fs::write(dir.path().join("bin/gen.rs"), "pub fn generated() {}\n").unwrap();
    let (mtime_secs, len) = fingerprint(&dir.path().join("bin/gen.rs")).unwrap();
    save(
        dir.path(),
        &SymbolCache {
            version: CACHE_VERSION,
            heuristic_version: HEURISTIC_VERSION,
            projects: vec![],
            entries: vec![CacheEntry {
                path: PathBuf::from("bin/gen.rs"),
                mtime_secs,
                len,
                symbols: vec![Symbol {
                    name: "generated".into(),
                    kind: SymbolKind::Function,
                    path: PathBuf::from("bin/gen.rs"),
                    line: 1,
                    project_id: None,
                }],
            }],
        },
    )
    .unwrap();

    let index = build_cached(dir.path(), &[]);

    assert!(
        !names(&index).contains(&"generated".to_string()),
        "build output is excluded by the walk and the cache must not reintroduce it: {:?}",
        names(&index)
    );
}

// -- writing back --------------------------------------------------------

#[test]
fn building_writes_a_cache_the_next_build_can_use() {
    let dir = workspace();

    let first = build_cached(dir.path(), &[]);
    assert_eq!(names(&first), vec!["real_thing".to_string()]);

    let stored = load(dir.path());
    assert_eq!(stored.version, CACHE_VERSION);
    assert_eq!(stored.heuristic_version, HEURISTIC_VERSION);
    assert_eq!(
        stored.entries.len(),
        1,
        "the file the walk found must be recorded: {stored:?}"
    );
    assert_eq!(stored.entries[0].path, PathBuf::from("lib.rs"));
    assert_eq!(stored.entries[0].symbols, vec![real_symbol()]);

    let second = build_cached(dir.path(), &[]);
    assert_eq!(second, first, "a second build must agree with the first");
}

/// The property the whole file rests on: with nothing cached, the cached path
/// must produce exactly what the cold path produces. If these two can differ
/// the cache is not an optimisation, it is a second implementation.
///
/// The project list is deliberately **not** empty. It used to be, and that is
/// precisely how a stale `project_id` survived this assertion for as long as it
/// did: with no projects every symbol on both sides carried `None`, so the one
/// field that can diverge between the two paths was the one field the
/// comparison could not see. Two projects, one nested inside the other, so the
/// longest-prefix rule is exercised as well.
#[test]
fn a_cold_cached_build_agrees_with_an_uncached_one() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(dir.path().join("src/nested")).unwrap();
    std::fs::write(
        dir.path().join("src/a.rs"),
        "pub fn one() {}\nstruct Two;\n",
    )
    .unwrap();
    std::fs::write(
        dir.path().join("src/nested/b.ts"),
        "export class Three {}\n",
    )
    .unwrap();
    std::fs::write(dir.path().join("README.md"), "# not parsable\n").unwrap();

    let projects = [
        project(dir.path(), "outer", "src"),
        project(dir.path(), "inner", "src/nested"),
    ];

    let cold = crate::symbols::index::build(dir.path(), &projects);
    let cached = build_cached(dir.path(), &projects);

    assert_eq!(cached.files, cold.files);
    assert_eq!(cached.symbols, cold.symbols);
    assert_eq!(cached.truncated, cold.truncated);
    assert_eq!(
        owners(&cold),
        vec![
            Some("outer".to_string()),
            Some("outer".to_string()),
            Some("inner".to_string())
        ],
        "the fixture must actually produce differing owners, or the comparison \
         above proves nothing about attribution"
    );
}

// -- ownership is an input, not a property of the file --------------------

/// The defect this section exists for. `Symbol::project_id` is derived from the
/// project list handed to the build, not from the bytes of the file, so a file
/// whose fingerprint is entirely honest can still be carrying an answer that
/// was correct for a different question.
#[test]
fn a_project_discovered_after_a_cached_build_takes_ownership_of_its_files() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(dir.path().join("src/NewProj")).unwrap();
    std::fs::write(
        dir.path().join("src/NewProj/Thing.cs"),
        "public class Thing\n",
    )
    .unwrap();

    let outer = project(dir.path(), "outer", "");
    let inner = project(dir.path(), "newproj", "src/NewProj");

    let first = build_cached(dir.path(), std::slice::from_ref(&outer));
    assert_eq!(
        owners(&first),
        vec![Some("outer".to_string())],
        "with only the root project known, it owns the file"
    );

    // The `.cs` file is not touched between the two builds, so its fingerprint
    // still describes it exactly — the cache will serve the stored entry, and
    // the stored entry names a project that no longer owns the file.
    let warm = build_cached(dir.path(), &[outer.clone(), inner.clone()]);
    let cold = crate::symbols::index::build(dir.path(), &[outer, inner]);

    assert_eq!(
        owners(&warm),
        owners(&cold),
        "a cached build must attribute symbols to the projects a cold build \
         would, and the project list is not covered by any fingerprint"
    );
    assert_eq!(owners(&warm), vec![Some("newproj".to_string())]);
}

/// The other direction, which a fix that only ever *narrows* ownership would
/// pass by accident: removing a project has to give its files back.
#[test]
fn a_project_removed_since_the_cached_build_gives_its_files_back() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(dir.path().join("src/Gone")).unwrap();
    std::fs::write(dir.path().join("src/Gone/Thing.cs"), "public class Thing\n").unwrap();

    let outer = project(dir.path(), "outer", "");
    let inner = project(dir.path(), "gone", "src/Gone");

    let first = build_cached(dir.path(), &[outer.clone(), inner]);
    assert_eq!(owners(&first), vec![Some("gone".to_string())]);

    let warm = build_cached(dir.path(), std::slice::from_ref(&outer));

    assert_eq!(
        owners(&warm),
        vec![Some("outer".to_string())],
        "a project that no longer exists must not go on owning files"
    );
}

/// The guard against over-correcting. A fix that discarded the cache whenever
/// any project existed would pass both tests above and cost the whole feature,
/// so an unchanged project list has to still be a hit.
#[test]
fn an_unchanged_project_list_still_serves_entries_from_the_cache() {
    let dir = workspace();
    let projects = [project(dir.path(), "outer", "")];

    build_cached(dir.path(), &projects);
    let stored = load(dir.path());
    assert_eq!(
        stored.projects,
        vec![CachedProject {
            id: "outer".into(),
            dir: dir.path().to_path_buf(),
        }],
        "the list the entries were attributed against must be recorded"
    );

    // Overwrite the entry with something no parse could produce; only a cache
    // hit can put it in the result.
    let mut doctored = stored.clone();
    doctored.entries[0].symbols = vec![fabricated()];
    save(dir.path(), &doctored).unwrap();

    let index = build_cached(dir.path(), &projects);

    assert!(
        names(&index).contains(&"never_written_by_anyone".to_string()),
        "an unchanged project list must not throw the cache away: {:?}",
        names(&index)
    );
}

/// The escape hatch the doc comment promises for the second-granularity
/// `mtime` problem: an explicit rebuild ignores whatever is on disk.
#[test]
fn rebuild_ignores_the_cache_entirely() {
    let dir = workspace();
    write_matching_cache(dir.path(), vec![fabricated()]);

    let index = rebuild(dir.path(), &[]);

    assert_eq!(
        names(&index),
        vec!["real_thing".to_string()],
        "an explicit rebuild must re-read every file"
    );
    assert_eq!(
        load(dir.path()).entries[0].symbols,
        vec![real_symbol()],
        "and must leave the refreshed result behind for the next build"
    );
}

/// The cache lands in a directory people commit, so its ignore entry has to be
/// written by the same call that writes the file.
#[test]
fn saving_the_cache_gitignores_it_in_the_same_call() {
    let dir = workspace();
    save(dir.path(), &SymbolCache::default()).unwrap();

    let ignore =
        std::fs::read_to_string(crate::config::config_dir(dir.path()).join(".gitignore")).unwrap();
    assert!(
        ignore.lines().any(|l| l.trim() == CACHE_FILE),
        "the cache must not reach a shared history: {ignore}"
    );
}

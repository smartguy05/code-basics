//! Tests for change groups. Included by `changelists.rs` under `#[cfg(test)]`.

use super::*;

fn workspace() -> tempfile::TempDir {
    tempfile::tempdir().unwrap()
}

#[test]
fn a_workspace_with_no_file_has_no_groups() {
    let dir = workspace();
    let lists = load(dir.path()).unwrap();

    assert!(lists.groups.is_empty());
    assert_eq!(lists.version, 1);
}

#[test]
fn groups_round_trip_through_the_file() {
    let dir = workspace();
    create(dir.path(), "Refactor").unwrap();
    assign(dir.path(), &["src/a.rs".into()], Some("Refactor")).unwrap();

    let reloaded = load(dir.path()).unwrap();
    assert_eq!(reloaded.groups.len(), 1);
    assert_eq!(reloaded.groups[0].name, "Refactor");
    assert_eq!(reloaded.groups[0].paths, vec!["src/a.rs"]);
}

#[test]
fn creating_a_group_twice_is_rejected() {
    let dir = workspace();
    create(dir.path(), "Refactor").unwrap();

    let error = create(dir.path(), "Refactor").unwrap_err().to_string();
    assert!(error.contains("already exists"), "got: {error}");
}

#[test]
fn a_group_needs_a_name() {
    let dir = workspace();
    assert!(create(dir.path(), "   ").is_err());
}

#[test]
fn group_names_are_trimmed() {
    let dir = workspace();
    let lists = create(dir.path(), "  Refactor  ").unwrap();
    assert_eq!(lists.groups[0].name, "Refactor");
}

#[test]
fn a_file_belongs_to_at_most_one_group() {
    // Moving a file must take it out of wherever it was, or the UI would have
    // to decide which of two groups to draw it in.
    let dir = workspace();
    create(dir.path(), "First").unwrap();
    create(dir.path(), "Second").unwrap();

    assign(dir.path(), &["src/a.rs".into()], Some("First")).unwrap();
    let lists = assign(dir.path(), &["src/a.rs".into()], Some("Second")).unwrap();

    assert_eq!(lists.group_of("src/a.rs"), Some("Second"));
    assert!(lists
        .groups
        .iter()
        .find(|g| g.name == "First")
        .unwrap()
        .paths
        .is_empty());
}

#[test]
fn assigning_to_no_group_removes_the_file_from_every_group() {
    let dir = workspace();
    create(dir.path(), "Refactor").unwrap();
    assign(dir.path(), &["src/a.rs".into()], Some("Refactor")).unwrap();

    let lists = assign(dir.path(), &["src/a.rs".into()], None).unwrap();

    assert_eq!(lists.group_of("src/a.rs"), None);
    assert!(
        lists.groups.iter().any(|g| g.name == "Refactor"),
        "the group itself survives"
    );
}

#[test]
fn assigning_the_same_file_twice_does_not_duplicate_it() {
    let dir = workspace();
    create(dir.path(), "Refactor").unwrap();
    assign(dir.path(), &["src/a.rs".into()], Some("Refactor")).unwrap();
    let lists = assign(dir.path(), &["src/a.rs".into()], Some("Refactor")).unwrap();

    assert_eq!(lists.groups[0].paths, vec!["src/a.rs"]);
}

#[test]
fn backslash_paths_match_the_forward_slash_ones_git_reports() {
    // The UI passes through whatever it was given; git status always says
    // `src/a.rs`, so a Windows-shaped path must not create a second entry.
    let dir = workspace();
    create(dir.path(), "Refactor").unwrap();

    let lists = assign(dir.path(), &[r"src\a.rs".into()], Some("Refactor")).unwrap();

    assert_eq!(lists.groups[0].paths, vec!["src/a.rs"]);
    assert_eq!(lists.group_of("src/a.rs"), Some("Refactor"));
}

#[test]
fn several_files_can_be_assigned_at_once() {
    let dir = workspace();
    create(dir.path(), "Refactor").unwrap();

    let lists = assign(
        dir.path(),
        &["src/b.rs".into(), "src/a.rs".into()],
        Some("Refactor"),
    )
    .unwrap();

    // Sorted, so the list does not reshuffle as files are added.
    assert_eq!(lists.groups[0].paths, vec!["src/a.rs", "src/b.rs"]);
}

#[test]
fn deleting_a_group_leaves_its_files_ungrouped() {
    let dir = workspace();
    create(dir.path(), "Refactor").unwrap();
    assign(dir.path(), &["src/a.rs".into()], Some("Refactor")).unwrap();

    let lists = remove(dir.path(), "Refactor").unwrap();

    assert!(lists.groups.is_empty());
    assert_eq!(
        lists.group_of("src/a.rs"),
        None,
        "the file is ungrouped, not lost"
    );
}

#[test]
fn renaming_keeps_the_members() {
    let dir = workspace();
    create(dir.path(), "Refactor").unwrap();
    assign(dir.path(), &["src/a.rs".into()], Some("Refactor")).unwrap();

    let lists = rename(dir.path(), "Refactor", "Cleanup").unwrap();

    assert_eq!(lists.group_of("src/a.rs"), Some("Cleanup"));
}

#[test]
fn renaming_onto_an_existing_name_is_rejected() {
    let dir = workspace();
    create(dir.path(), "First").unwrap();
    create(dir.path(), "Second").unwrap();

    assert!(rename(dir.path(), "First", "Second").is_err());
    // Renaming a group to itself is a no-op, not a clash.
    assert!(rename(dir.path(), "First", "First").is_ok());
}

#[test]
fn operating_on_an_unknown_group_is_an_error() {
    let dir = workspace();
    assert!(remove(dir.path(), "Nope").is_err());
    assert!(rename(dir.path(), "Nope", "Other").is_err());
    assert!(assign(dir.path(), &["a.rs".into()], Some("Nope")).is_err());
}

#[test]
fn the_file_is_gitignored_so_it_stays_personal() {
    // Committing one person's work-in-progress grouping would be noise for
    // everyone else.
    let dir = workspace();
    create(dir.path(), "Refactor").unwrap();

    let ignore =
        std::fs::read_to_string(crate::config::config_dir(dir.path()).join(".gitignore")).unwrap();

    assert!(
        ignore.lines().any(|l| l.trim() == CHANGELISTS_FILE),
        "got: {ignore}"
    );
    assert!(
        ignore.lines().any(|l| l.trim() == "results/"),
        "the existing entry must survive"
    );
}

#[test]
fn an_existing_gitignore_is_extended_not_replaced() {
    // A workspace created before change groups existed still needs the entry,
    // and anything hand-written has to survive.
    let dir = workspace();
    let config_dir = crate::config::config_dir(dir.path());
    std::fs::create_dir_all(&config_dir).unwrap();
    std::fs::write(config_dir.join(".gitignore"), "results/\nmy-own-entry\n").unwrap();

    create(dir.path(), "Refactor").unwrap();

    let ignore = std::fs::read_to_string(config_dir.join(".gitignore")).unwrap();
    assert!(
        ignore.contains("my-own-entry"),
        "hand-written entries must survive"
    );
    assert!(ignore.lines().any(|l| l.trim() == CHANGELISTS_FILE));
    assert_eq!(
        ignore.matches("results/").count(),
        1,
        "no duplicate entries"
    );
}

#[test]
fn a_corrupt_file_is_reported_rather_than_silently_ignored() {
    let dir = workspace();
    let config_dir = crate::config::config_dir(dir.path());
    std::fs::create_dir_all(&config_dir).unwrap();
    std::fs::write(config_dir.join(CHANGELISTS_FILE), "{ not json").unwrap();

    let error = load(dir.path()).unwrap_err().to_string();
    assert!(error.contains("change-group JSON"), "got: {error}");
}

// -- where the file lives, and the save/load pair on its own -----------------

/// Grouping is one person's work in progress, so the file sits inside the
/// git-ignored `.code-basics/` directory rather than beside the code.
#[test]
fn the_file_lives_in_the_config_directory_under_a_stable_name() {
    let root = Path::new("/repo");

    assert_eq!(
        changelists_path(root),
        crate::config::config_dir(root).join(CHANGELISTS_FILE)
    );
    assert_eq!(CHANGELISTS_FILE, "changelists.json");
    assert!(changelists_path(root).starts_with(crate::config::config_dir(root)));
}

#[test]
fn saving_and_loading_round_trips_the_whole_document() {
    let dir = workspace();
    let lists = Changelists {
        version: 1,
        groups: vec![
            Changelist {
                name: "Refactor".into(),
                paths: vec!["src/a.rs".into(), "src/b.rs".into()],
            },
            Changelist {
                name: "Docs".into(),
                paths: Vec::new(),
            },
        ],
    };

    save(dir.path(), &lists).unwrap();

    assert_eq!(load(dir.path()).unwrap(), lists);
}

/// The order groups are written in is the order they are shown, so a save must
/// not sort or otherwise rearrange them.
#[test]
fn saving_preserves_the_order_the_groups_were_given_in() {
    let dir = workspace();
    let lists = Changelists {
        version: 1,
        groups: ["Zebra", "Apple", "Mango"]
            .iter()
            .map(|name| Changelist {
                name: (*name).to_string(),
                paths: Vec::new(),
            })
            .collect(),
    };

    save(dir.path(), &lists).unwrap();

    let names: Vec<String> = load(dir.path())
        .unwrap()
        .groups
        .into_iter()
        .map(|g| g.name)
        .collect();
    assert_eq!(names, vec!["Zebra", "Apple", "Mango"]);
}

/// The first save in a workspace has no `.code-basics/` to write into.
#[test]
fn saving_creates_the_config_directory_it_needs() {
    let dir = workspace();
    assert!(!crate::config::config_dir(dir.path()).exists());

    save(dir.path(), &Changelists::default()).unwrap();

    assert!(changelists_path(dir.path()).exists());
}

#[test]
fn saving_replaces_the_previous_document_rather_than_appending() {
    let dir = workspace();
    create(dir.path(), "Old").unwrap();

    save(dir.path(), &Changelists::default()).unwrap();

    assert!(load(dir.path()).unwrap().groups.is_empty());
}

/// The file is read by hand as often as by the app, so it stays pretty-printed
/// and newline-terminated.
#[test]
fn the_saved_file_is_readable_json() {
    let dir = workspace();
    save(
        dir.path(),
        &Changelists {
            version: 1,
            groups: vec![Changelist {
                name: "Refactor".into(),
                paths: vec!["src/a.rs".into()],
            }],
        },
    )
    .unwrap();

    let text = std::fs::read_to_string(changelists_path(dir.path())).unwrap();

    assert!(text.ends_with('\n'), "got: {text:?}");
    assert!(text.contains('\n'), "the document must be pretty-printed");
    assert!(text.contains("\"Refactor\""), "got: {text}");
}

#[test]
fn a_saved_document_carries_its_version_forward() {
    let dir = workspace();
    save(dir.path(), &Changelists::default()).unwrap();

    assert_eq!(load(dir.path()).unwrap().version, 1);
}

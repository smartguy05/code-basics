//! Tests for [`super::store`].
//!
//! Two things are pinned harder than the rest here. The first is provenance:
//! every path through [`parse`] that cannot be understood has to land on
//! [`DiagramDerivation::User`] *and* say so in a warning, because a diagram
//! that silently claims to be derived is the failure this module exists to
//! prevent. The second is the name check, which takes a string that may one day
//! arrive from inside a diagram file's own contents.

use std::path::Path;

use super::store::*;

fn workspace() -> tempfile::TempDir {
    tempfile::tempdir().unwrap()
}

fn user_front() -> FrontMatter {
    FrontMatter {
        level: Some("project-map".into()),
        derivation: DiagramDerivation::User,
        generated: None,
        source_commit: None,
        edited: false,
    }
}

fn inferred_front(agent: &str) -> FrontMatter {
    FrontMatter {
        level: Some("project-map".into()),
        derivation: DiagramDerivation::Inferred {
            agent: agent.into(),
        },
        generated: Some("2026-08-11T09:14:00Z".into()),
        source_commit: Some("4f1a2b3".into()),
        edited: false,
    }
}

fn derived_front() -> FrontMatter {
    FrontMatter {
        level: Some("project-map".into()),
        derivation: DiagramDerivation::Derived,
        generated: Some("2026-08-11T09:14:00Z".into()),
        source_commit: Some("4f1a2b3".into()),
        edited: false,
    }
}

// -- layout -----------------------------------------------------------------

#[test]
fn diagrams_live_in_the_shared_config_directory_so_a_team_keeps_them() {
    let root = Path::new("/ws");
    assert_eq!(
        dir(root),
        crate::config::config_dir(root).join(DIAGRAMS_DIR)
    );
    assert_eq!(derived_dir(root), dir(root).join(DERIVED_DIR));
    assert_eq!(prompts_dir(root), dir(root).join(PROMPTS_DIR));
}

#[test]
fn a_derived_diagram_belongs_in_the_regenerated_subdirectory() {
    let root = Path::new("/ws");
    assert_eq!(
        path(root, "project-map.md", &DiagramDerivation::Derived).unwrap(),
        derived_dir(root).join("project-map.md")
    );
}

#[test]
fn an_inferred_or_user_diagram_belongs_beside_the_committed_ones() {
    let root = Path::new("/ws");
    assert_eq!(
        path(root, "project-map.md", &DiagramDerivation::User).unwrap(),
        dir(root).join("project-map.md")
    );
    assert_eq!(
        path(
            root,
            "project-map.md",
            &DiagramDerivation::Inferred {
                agent: "claude-code".into()
            }
        )
        .unwrap(),
        dir(root).join("project-map.md")
    );
}

#[test]
fn a_name_without_a_markdown_extension_gains_one() {
    let root = Path::new("/ws");
    assert_eq!(
        path(root, "project-map", &DiagramDerivation::User).unwrap(),
        dir(root).join("project-map.md")
    );
}

// -- names, which may one day come from inside a diagram --------------------

#[test]
fn a_name_containing_a_parent_directory_segment_is_refused() {
    let root = Path::new("/ws");
    for name in ["..", "../secrets.md", "a/../../b.md"] {
        assert!(
            path(root, name, &DiagramDerivation::User).is_err(),
            "{name} should be refused"
        );
    }
}

#[test]
fn a_name_containing_a_separator_is_refused() {
    let root = Path::new("/ws");
    for name in ["derived/a.md", "sub\\a.md", "a/b"] {
        assert!(
            path(root, name, &DiagramDerivation::User).is_err(),
            "{name} should be refused"
        );
    }
}

#[test]
fn an_absolute_name_is_refused() {
    let root = Path::new("/ws");
    for name in ["/etc/passwd", "C:\\windows\\system32\\drivers\\etc\\hosts"] {
        assert!(
            path(root, name, &DiagramDerivation::User).is_err(),
            "{name} should be refused"
        );
    }
}

#[test]
fn an_empty_or_blank_name_is_refused() {
    let root = Path::new("/ws");
    for name in ["", "   ", ".", ".md"] {
        assert!(
            path(root, name, &DiagramDerivation::User).is_err(),
            "{name:?} should be refused"
        );
    }
}

#[test]
fn a_refused_name_never_reaches_the_filesystem() {
    let dir = workspace();
    assert!(write(dir.path(), "../escaped.md", "hello").is_err());
    assert!(read(dir.path(), "../escaped.md").is_err());
    assert!(!dir.path().parent().unwrap().join("escaped.md").exists());
}

// -- front matter -----------------------------------------------------------

#[test]
fn front_matter_round_trips_through_render_and_parse_unchanged() {
    let front = inferred_front("claude-code");
    let body = "```mermaid\nflowchart LR\n  a --> b\n```\n";

    let rendered = render(&front, body).unwrap();
    let parsed = parse(&rendered);

    assert_eq!(parsed.front, front);
    assert_eq!(parsed.body, body);
    assert_eq!(parsed.warning, None);
}

#[test]
fn a_derived_and_a_user_diagram_also_round_trip() {
    for front in [derived_front(), user_front()] {
        let rendered = render(&front, "body\n").unwrap();
        let parsed = parse(&rendered);
        assert_eq!(parsed.front, front, "in: {rendered}");
        assert_eq!(parsed.body, "body\n");
    }
}

#[test]
fn the_format_version_is_written_so_a_later_reader_can_refuse_the_file() {
    let rendered = render(&user_front(), "body\n").unwrap();
    assert!(
        rendered
            .lines()
            .any(|l| l.trim() == format!("code-basics: {FORMAT_VERSION}")),
        "got: {rendered}"
    );
}

#[test]
fn a_file_with_no_front_matter_is_read_as_a_user_diagram() {
    let text = "flowchart LR\n  a --> b\n";
    let parsed = parse(text);

    assert_eq!(parsed.front.derivation, DiagramDerivation::User);
    assert_eq!(parsed.body, text, "the whole file is still the body");
    assert!(parsed.warning.is_some(), "the reader must be told why");
}

#[test]
fn front_matter_carrying_a_key_this_version_does_not_know_is_not_guessed_at() {
    let text = "---\ncode-basics: v1\nderivation: derived\nconfidence: 0.9\n---\nbody\n";
    let parsed = parse(text);

    assert_eq!(
        parsed.front.derivation,
        DiagramDerivation::User,
        "an unknown key may change what the keys beside it mean"
    );
    assert!(parsed.warning.unwrap().contains("confidence"));
}

#[test]
fn front_matter_from_an_unknown_format_version_is_not_guessed_at() {
    let text = "---\ncode-basics: v2\nderivation: derived\n---\nbody\n";
    let parsed = parse(text);

    assert_eq!(parsed.front.derivation, DiagramDerivation::User);
    assert!(parsed.warning.is_some());
}

#[test]
fn front_matter_with_no_version_marker_is_not_guessed_at() {
    let text = "---\nderivation: derived\n---\nbody\n";
    let parsed = parse(text);

    assert_eq!(parsed.front.derivation, DiagramDerivation::User);
    assert!(parsed.warning.is_some());
}

#[test]
fn an_unterminated_front_matter_block_is_not_guessed_at() {
    let text = "---\ncode-basics: v1\nderivation: derived\nbody\n";
    let parsed = parse(text);

    assert_eq!(parsed.front.derivation, DiagramDerivation::User);
    assert!(parsed.warning.is_some());
}

#[test]
fn an_unknown_derivation_value_is_not_guessed_at() {
    let text = "---\ncode-basics: v1\nderivation: generated\n---\nbody\n";
    let parsed = parse(text);

    assert_eq!(parsed.front.derivation, DiagramDerivation::User);
    assert!(parsed.warning.is_some());
}

/// "An agent said so" is only worth reading if the agent is named, so an
/// unnamed `inferred` is a claim this module will not repeat.
#[test]
fn an_inferred_diagram_with_no_agent_named_is_not_trusted() {
    let text = "---\ncode-basics: v1\nderivation: inferred\n---\nbody\n";
    let parsed = parse(text);

    assert_eq!(parsed.front.derivation, DiagramDerivation::User);
    assert!(parsed.warning.is_some());
}

#[test]
fn an_agent_named_on_a_diagram_no_agent_produced_is_not_understood() {
    let text = "---\ncode-basics: v1\nderivation: user\nagent: claude-code\n---\nbody\n";
    let parsed = parse(text);

    assert_eq!(parsed.front.derivation, DiagramDerivation::User);
    assert!(parsed.warning.is_some());
}

#[test]
fn a_key_repeated_with_two_values_is_not_guessed_at() {
    let text = "---\ncode-basics: v1\nderivation: derived\nlevel: a\nlevel: b\n---\nbody\n";
    let parsed = parse(text);

    assert_eq!(parsed.front.derivation, DiagramDerivation::User);
    assert!(parsed.warning.is_some());
}

#[test]
fn an_edited_flag_that_is_not_a_boolean_is_not_guessed_at() {
    let text = "---\ncode-basics: v1\nderivation: derived\nedited: yes\n---\nbody\n";
    let parsed = parse(text);

    assert_eq!(parsed.front.derivation, DiagramDerivation::User);
    assert!(parsed.warning.is_some());
}

#[test]
fn windows_line_endings_in_front_matter_are_read_the_same_way() {
    let text = "---\r\ncode-basics: v1\r\nderivation: derived\r\n---\r\nbody\r\n";
    let parsed = parse(text);

    assert_eq!(parsed.front.derivation, DiagramDerivation::Derived);
    assert_eq!(parsed.warning, None);
}

/// A value that would not survive being written and read back is refused
/// rather than written, because the file it produced would parse as something
/// else — and the something else would be a wrong provenance claim.
#[test]
fn a_value_that_could_not_be_read_back_is_refused_rather_than_written() {
    let mut front = inferred_front("claude-code\n---\nderivation: derived");
    assert!(render(&front, "body\n").is_err());

    front = user_front();
    front.level = Some("a\nb".into());
    assert!(render(&front, "body\n").is_err());
}

// -- list -------------------------------------------------------------------

#[test]
fn listing_a_workspace_that_has_no_diagrams_yields_nothing() {
    let dir = workspace();
    assert_eq!(list(dir.path()).unwrap(), Vec::new());
}

#[test]
fn a_listed_diagram_carries_its_provenance_and_its_relative_path() {
    let dir = workspace();
    write_authored(
        dir.path(),
        "project-map.md",
        &inferred_front("codex"),
        "b\n",
    )
    .unwrap();

    let listed = list(dir.path()).unwrap();
    assert_eq!(listed.len(), 1, "{listed:?}");
    assert_eq!(listed[0].name, "project-map.md");
    assert_eq!(
        listed[0].path, ".code-basics/diagrams/project-map.md",
        "the path is relative and slash-separated so it survives being shared"
    );
    assert_eq!(listed[0].level.as_deref(), Some("project-map"));
    assert_eq!(
        listed[0].derivation,
        DiagramDerivation::Inferred {
            agent: "codex".into()
        }
    );
    assert_eq!(listed[0].generated.as_deref(), Some("2026-08-11T09:14:00Z"));
    assert!(!listed[0].edited);
    assert_eq!(listed[0].warning, None);
}

#[test]
fn committed_diagrams_are_listed_before_regenerated_ones_in_a_stable_order() {
    let dir = workspace();
    write_authored(dir.path(), "zebra.md", &user_front(), "b\n").unwrap();
    write_authored(dir.path(), "alpha.md", &user_front(), "b\n").unwrap();
    write_authored(dir.path(), "omega.md", &derived_front(), "b\n").unwrap();
    write_authored(dir.path(), "beta.md", &derived_front(), "b\n").unwrap();

    let names: Vec<String> = list(dir.path())
        .unwrap()
        .into_iter()
        .map(|d| d.name)
        .collect();
    assert_eq!(names, ["alpha.md", "zebra.md", "beta.md", "omega.md"]);
}

#[test]
fn a_malformed_diagram_is_listed_with_a_warning_rather_than_hidden() {
    let dir = workspace();
    std::fs::create_dir_all(super::store::dir(dir.path())).unwrap();
    std::fs::write(
        super::store::dir(dir.path()).join("hand-written.md"),
        "flowchart LR\n",
    )
    .unwrap();

    let listed = list(dir.path()).unwrap();
    assert_eq!(listed.len(), 1);
    assert_eq!(listed[0].derivation, DiagramDerivation::User);
    assert!(listed[0].warning.is_some());
}

#[test]
fn only_markdown_files_are_listed() {
    let dir = workspace();
    let d = super::store::dir(dir.path());
    std::fs::create_dir_all(d.join(PROMPTS_DIR)).unwrap();
    std::fs::write(d.join("notes.txt"), "x").unwrap();
    std::fs::write(d.join(PROMPTS_DIR).join("ask.md"), "x").unwrap();
    write_authored(dir.path(), "project-map.md", &user_front(), "b\n").unwrap();

    let names: Vec<String> = list(dir.path())
        .unwrap()
        .into_iter()
        .map(|d| d.name)
        .collect();
    assert_eq!(names, ["project-map.md"]);
}

// -- writing ----------------------------------------------------------------

#[test]
fn writing_creates_every_missing_parent_directory() {
    let dir = workspace();
    assert!(!super::store::dir(dir.path()).exists());

    let written = write_authored(dir.path(), "project-map.md", &derived_front(), "b\n").unwrap();

    assert!(written.exists(), "{}", written.display());
    assert_eq!(written, derived_dir(dir.path()).join("project-map.md"));
}

/// The regenerated directory is written into a directory the user shares with
/// their team, so the ignore entry has to appear the moment the first file
/// does — not the next time some other feature happens to run.
#[test]
fn writing_a_diagram_ignores_the_regenerated_directories_in_git() {
    let dir = workspace();
    write_authored(dir.path(), "project-map.md", &derived_front(), "b\n").unwrap();

    let ignore =
        std::fs::read_to_string(crate::config::config_dir(dir.path()).join(".gitignore")).unwrap();
    let lines: Vec<&str> = ignore.lines().map(str::trim).collect();
    assert!(lines.contains(&"diagrams/derived/"), "got: {ignore}");
    assert!(lines.contains(&"diagrams/.prompts/"), "got: {ignore}");
}

#[test]
fn reading_returns_the_file_exactly_as_it_is_on_disk() {
    let dir = workspace();
    let contents = render(&user_front(), "body\n").unwrap();
    write(dir.path(), "project-map.md", &contents).unwrap();

    assert_eq!(read(dir.path(), "project-map.md").unwrap(), contents);
}

#[test]
fn a_derived_diagram_can_be_read_back_by_name_from_the_regenerated_directory() {
    let dir = workspace();
    write_authored(dir.path(), "project-map.md", &derived_front(), "b\n").unwrap();

    let text = read(dir.path(), "project-map.md").unwrap();
    assert_eq!(parse(&text).front.derivation, DiagramDerivation::Derived);
}

#[test]
fn reading_a_diagram_that_is_not_there_fails() {
    let dir = workspace();
    assert!(read(dir.path(), "missing.md").is_err());
}

#[test]
fn a_saved_body_is_stored_verbatim() {
    let dir = workspace();
    let body = "```mermaid\nflowchart LR\n  a --> b\n```\n";
    write_authored(dir.path(), "m.md", &user_front(), body).unwrap();

    assert_eq!(parse(&read(dir.path(), "m.md").unwrap()).body, body);
}

// -- editing, and what it does to provenance --------------------------------

#[test]
fn hand_editing_an_inferred_diagram_marks_it_edited_without_erasing_the_agent() {
    let dir = workspace();
    write_authored(
        dir.path(),
        "m.md",
        &inferred_front("claude-code"),
        "flowchart LR\n  a --> b\n",
    )
    .unwrap();

    let mut edited = read(dir.path(), "m.md").unwrap();
    edited = edited.replace("a --> b", "a --> c");
    write(dir.path(), "m.md", &edited).unwrap();

    let parsed = parse(&read(dir.path(), "m.md").unwrap());
    assert_eq!(
        parsed.front.derivation,
        DiagramDerivation::Inferred {
            agent: "claude-code".into()
        },
        "the arrows still came from an agent"
    );
    assert!(parsed.front.edited, "but a person has since changed them");
    assert!(parsed.body.contains("a --> c"));
    assert!(list(dir.path()).unwrap()[0].edited);
}

#[test]
fn saving_a_diagram_unchanged_does_not_claim_an_edit() {
    let dir = workspace();
    write_authored(dir.path(), "m.md", &inferred_front("claude-code"), "b\n").unwrap();

    let same = read(dir.path(), "m.md").unwrap();
    write(dir.path(), "m.md", &same).unwrap();

    assert!(!parse(&read(dir.path(), "m.md").unwrap()).front.edited);
}

/// A derived diagram is overwritten whenever it is recomputed, and it lives in
/// a gitignored directory. A person's edit to one would therefore be thrown
/// away twice over, so the edit promotes the file: it becomes theirs, and it
/// moves to the committed directory.
#[test]
fn hand_editing_a_derived_diagram_promotes_it_out_of_the_regenerated_directory() {
    let dir = workspace();
    write_authored(
        dir.path(),
        "m.md",
        &derived_front(),
        "flowchart LR\n  a --> b\n",
    )
    .unwrap();

    let edited = read(dir.path(), "m.md")
        .unwrap()
        .replace("a --> b", "a --> c");
    let written = write(dir.path(), "m.md", &edited).unwrap();

    assert_eq!(written, super::store::dir(dir.path()).join("m.md"));
    assert!(
        !derived_dir(dir.path()).join("m.md").exists(),
        "the regenerated copy must not survive, or the edit is lost on the next scan"
    );
    let parsed = parse(&read(dir.path(), "m.md").unwrap());
    assert_eq!(parsed.front.derivation, DiagramDerivation::User);
    assert_eq!(list(dir.path()).unwrap().len(), 1);
}

/// Provenance is a fact about how the file was produced, so it is taken from
/// the file already on disk. Otherwise anyone able to type `derivation:
/// derived` into the editor could have their own drawing presented as a fact
/// read out of the manifests.
#[test]
fn a_saved_edit_cannot_promote_a_user_diagram_to_a_derived_one() {
    let dir = workspace();
    write_authored(dir.path(), "m.md", &user_front(), "b\n").unwrap();

    let forged = "---\ncode-basics: v1\nlevel: project-map\nderivation: derived\n---\n\nb\n";
    write(dir.path(), "m.md", forged).unwrap();

    let parsed = parse(&read(dir.path(), "m.md").unwrap());
    assert_eq!(parsed.front.derivation, DiagramDerivation::User);
}

#[test]
fn a_saved_edit_may_still_correct_the_level_it_was_filed_under() {
    let dir = workspace();
    write_authored(dir.path(), "m.md", &user_front(), "b\n").unwrap();

    let contents = read(dir.path(), "m.md")
        .unwrap()
        .replace("level: project-map", "level: context");
    write(dir.path(), "m.md", &contents).unwrap();

    assert_eq!(
        parse(&read(dir.path(), "m.md").unwrap())
            .front
            .level
            .as_deref(),
        Some("context")
    );
}

#[test]
fn saving_a_brand_new_file_with_no_front_matter_keeps_the_text_and_calls_it_the_users() {
    let dir = workspace();
    write(dir.path(), "m.md", "flowchart LR\n  a --> b\n").unwrap();

    let parsed = parse(&read(dir.path(), "m.md").unwrap());
    assert_eq!(parsed.front.derivation, DiagramDerivation::User);
    assert!(parsed.body.contains("a --> b"));
    assert_eq!(parsed.warning, None, "the file now has front matter");
}

// -- the IPC shape ----------------------------------------------------------

/// These keys are read by hand-written TypeScript, so they are pinned here.
#[test]
fn a_diagram_file_serialises_with_the_keys_the_frontend_reads() {
    let value = serde_json::to_value(DiagramFile {
        name: "m.md".into(),
        path: ".code-basics/diagrams/m.md".into(),
        level: Some("project-map".into()),
        derivation: DiagramDerivation::Inferred {
            agent: "codex".into(),
        },
        generated: Some("2026-08-11T09:14:00Z".into()),
        edited: true,
        warning: None,
    })
    .unwrap();

    let mut keys: Vec<&str> = value
        .as_object()
        .unwrap()
        .keys()
        .map(String::as_str)
        .collect();
    keys.sort_unstable();
    assert_eq!(
        keys,
        [
            "derivation",
            "edited",
            "generated",
            "level",
            "name",
            "path",
            "warning"
        ]
    );
    assert_eq!(value["derivation"]["inferred"]["agent"], "codex");
}

#[test]
fn a_derived_or_user_derivation_serialises_as_a_bare_string() {
    assert_eq!(
        serde_json::to_value(DiagramDerivation::Derived).unwrap(),
        serde_json::json!("derived")
    );
    assert_eq!(
        serde_json::to_value(DiagramDerivation::User).unwrap(),
        serde_json::json!("user")
    );
}

#[test]
fn front_matter_serialises_its_source_commit_in_camel_case() {
    let value = serde_json::to_value(inferred_front("codex")).unwrap();
    assert!(value.get("sourceCommit").is_some(), "{value}");
}

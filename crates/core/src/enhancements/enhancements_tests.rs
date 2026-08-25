//! Tests for the instruction-template library.
//!
//! Everything here is pure string/filesystem logic, so the whole feature is
//! exercised without a window or a Tauri handle: parse a template, splice it in
//! at each anchor, prove it is idempotent, and cut it back out cleanly.

use super::*;

const FRONT: &str = "\
---
title: Memory Files
id: memory
placement: after-first-heading
---
## CRITICAL: Memory Files

Body line one.
";

#[test]
fn parses_front_matter() {
    let t = parse_template(FRONT, "fallback");
    assert_eq!(t.id, "memory");
    assert_eq!(t.title, "Memory Files");
    assert_eq!(t.placement, Placement::AfterFirstHeading);
    assert!(t.body.contains("## CRITICAL: Memory Files"));
    assert!(t.body.contains("Body line one."));
    // The front matter itself is not part of the body.
    assert!(!t.body.contains("placement:"));
}

#[test]
fn missing_front_matter_is_all_body_appended_at_end() {
    let t = parse_template("Just some text.\n", "note");
    assert_eq!(t.id, "note");
    assert_eq!(t.title, "note");
    assert_eq!(t.placement, Placement::End);
    assert_eq!(t.body.trim(), "Just some text.");
}

#[test]
fn missing_id_falls_back_to_default() {
    let text = "---\ntitle: Thing\n---\nbody\n";
    let t = parse_template(text, "the-stem");
    assert_eq!(t.id, "the-stem");
    assert_eq!(t.title, "Thing");
}

#[test]
fn placement_keywords_parse_and_unknown_falls_back_to_end() {
    let mk =
        |p: &str| parse_template(&format!("---\nid: x\nplacement: {p}\n---\nb\n"), "x").placement;
    assert_eq!(mk("top"), Placement::Top);
    assert_eq!(mk("after-first-heading"), Placement::AfterFirstHeading);
    assert_eq!(mk("end"), Placement::End);
    assert_eq!(mk("nonsense"), Placement::End);
}

#[test]
fn marker_placement_carries_its_anchor() {
    let text = "---\nid: x\nplacement: after-marker\nanchor: <!-- pin -->\n---\nb\n";
    assert_eq!(
        parse_template(text, "x").placement,
        Placement::AfterMarker("<!-- pin -->".to_string())
    );
}

#[test]
fn markers_are_namespaced_by_id() {
    assert_eq!(
        begin_marker("memory"),
        "<!-- code-basics: enhancement:memory -->"
    );
    assert_eq!(
        end_marker("memory"),
        "<!-- /code-basics: enhancement:memory -->"
    );
    // Must not collide with the intent block's marker.
    assert_ne!(begin_marker("memory"), "<!-- code-basics: agent intent -->");
}

fn template(id: &str, placement: Placement, body: &str) -> Template {
    Template {
        id: id.to_string(),
        title: id.to_string(),
        placement,
        once: false,
        body: body.to_string(),
    }
}

#[test]
fn once_defaults_to_false_and_opts_in_only_on_a_truthy_value() {
    // Absent, or any non-truthy value, is not run-once.
    assert!(!parse_template("---\nid: x\n---\nb\n", "x").once);
    assert!(!parse_template("---\nid: x\nonce: false\n---\nb\n", "x").once);
    assert!(!parse_template("---\nid: x\nonce: maybe\n---\nb\n", "x").once);
    // Explicit truthy values opt in (case-insensitive).
    assert!(parse_template("---\nid: x\nonce: true\n---\nb\n", "x").once);
    assert!(parse_template("---\nid: x\nonce: TRUE\n---\nb\n", "x").once);
    assert!(parse_template("---\nid: x\nonce: yes\n---\nb\n", "x").once);
}

#[test]
fn list_prompts_carries_the_run_once_flag() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join("setup.md"),
        "---\nid: setup\ntitle: Setup\nonce: true\n---\nBody.\n",
    )
    .unwrap();
    std::fs::write(
        dir.path().join("review.md"),
        "---\nid: review\ntitle: Review\n---\nBody.\n",
    )
    .unwrap();

    let prompts = list_prompts(dir.path());
    let setup = prompts.iter().find(|p| p.id == "setup").unwrap();
    let review = prompts.iter().find(|p| p.id == "review").unwrap();
    assert!(setup.once, "declared run-once");
    assert!(!review.once, "not declared run-once");
}

#[test]
fn inserts_at_top_of_empty_file() {
    let t = template("m", Placement::Top, "HELLO");
    let out = insert("", &t);
    assert!(out.starts_with("<!-- code-basics: enhancement:m -->"));
    assert!(out.contains("HELLO"));
    assert!(is_present(&out, "m"));
}

#[test]
fn inserts_just_under_the_first_heading() {
    let file = "# CLAUDE.md\n\nExisting content.\n";
    let t = template("memory", Placement::AfterFirstHeading, "MEM");
    let out = insert(file, &t);

    let heading = out.find("# CLAUDE.md").unwrap();
    let marker = out.find("enhancement:memory").unwrap();
    let existing = out.find("Existing content.").unwrap();
    // Section sits after the heading and before the pre-existing content.
    assert!(heading < marker && marker < existing);
    // Exactly one blank line between the heading and our block.
    assert!(out.contains("# CLAUDE.md\n\n<!-- code-basics: enhancement:memory -->"));
}

#[test]
fn after_first_heading_with_no_heading_falls_back_to_top() {
    let file = "no headings here\nsecond line\n";
    let t = template("m", Placement::AfterFirstHeading, "MEM");
    let out = insert(file, &t);
    assert!(out.starts_with("<!-- code-basics: enhancement:m -->"));
}

#[test]
fn appends_at_end() {
    let file = "# Title\n\nStuff.\n";
    let t = template("z", Placement::End, "TAIL");
    let out = insert(file, &t);
    let stuff = out.find("Stuff.").unwrap();
    let marker = out.find("enhancement:z").unwrap();
    assert!(stuff < marker);
    assert!(out.ends_with("<!-- /code-basics: enhancement:z -->\n"));
}

#[test]
fn inserts_relative_to_a_named_marker() {
    let file = "one\n<!-- pin -->\ntwo\n";
    let after = insert(
        file,
        &template("a", Placement::AfterMarker("<!-- pin -->".into()), "X"),
    );
    assert!(after.find("<!-- pin -->").unwrap() < after.find("enhancement:a").unwrap());

    let before = insert(
        file,
        &template("b", Placement::BeforeMarker("<!-- pin -->".into()), "Y"),
    );
    assert!(before.find("enhancement:b").unwrap() < before.find("<!-- pin -->").unwrap());
}

#[test]
fn missing_marker_anchor_falls_back_to_end() {
    let file = "one\ntwo\n";
    let t = template("a", Placement::AfterMarker("<!-- absent -->".into()), "X");
    let out = insert(file, &t);
    let two = out.find("two").unwrap();
    assert!(two < out.find("enhancement:a").unwrap());
}

#[test]
fn re_inserting_is_idempotent_and_refreshes_in_place() {
    let file = "# H\n\ntail\n";
    let t = template("m", Placement::AfterFirstHeading, "V1");
    let once = insert(file, &t);
    let twice = insert(&once, &t);
    assert_eq!(
        once, twice,
        "adding an already-present section changes nothing"
    );
    assert_eq!(
        once.matches("<!-- code-basics: enhancement:m -->").count(),
        1
    );

    // A changed body rewrites the existing span rather than duplicating it.
    let updated = insert(&once, &template("m", Placement::AfterFirstHeading, "V2"));
    assert!(updated.contains("V2"));
    assert!(!updated.contains("V1"));
    assert_eq!(
        updated
            .matches("<!-- code-basics: enhancement:m -->")
            .count(),
        1
    );
}

#[test]
fn removes_a_present_section_and_leaves_the_rest_intact() {
    let file = "# H\n\nbefore\n";
    let t = template("m", Placement::AfterFirstHeading, "MEM");
    let with = insert(file, &t);

    let without = remove(&with, "m").expect("section was present");
    assert!(!without.contains("enhancement:m"));
    assert!(!without.contains("MEM"));
    assert!(without.contains("# H"));
    assert!(without.contains("before"));
    // No doubled blank line left where the block was.
    assert!(!without.contains("\n\n\n"));
}

#[test]
fn removing_an_absent_section_reports_nothing_to_do() {
    assert_eq!(remove("# H\n\nbody\n", "m"), None);
}

#[test]
fn preserves_crlf_line_endings() {
    let file = "# H\r\n\r\nbody\r\n";
    let t = template("m", Placement::AfterFirstHeading, "MEM");
    let out = insert(file, &t);
    assert!(is_present(&out, "m"));
    assert!(out.contains("\r\n"));
    assert!(
        !out.contains("\n\n"),
        "no bare LF pairs snuck into a CRLF file"
    );
}

#[test]
fn discover_reads_markdown_skips_others_and_sorts_by_title() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("b.md"), "---\ntitle: Beta\nid: b\n---\nx\n").unwrap();
    std::fs::write(
        dir.path().join("a.md"),
        "---\ntitle: Alpha\nid: a\n---\ny\n",
    )
    .unwrap();
    std::fs::write(dir.path().join("notes.txt"), "ignored").unwrap();

    let found = discover(dir.path());
    let titles: Vec<_> = found.iter().map(|t| t.title.as_str()).collect();
    assert_eq!(titles, ["Alpha", "Beta"]);
}

#[test]
fn seed_copies_missing_defaults_but_never_overwrites_edits() {
    let bundled = tempfile::tempdir().unwrap();
    std::fs::write(bundled.path().join("memory.md"), "bundled-memory").unwrap();
    std::fs::write(bundled.path().join("extra.md"), "bundled-extra").unwrap();

    let dir = tempfile::tempdir().unwrap();
    // The user already edited memory.md; it must survive seeding.
    std::fs::write(dir.path().join("memory.md"), "user-edited").unwrap();

    seed(dir.path(), Some(bundled.path())).unwrap();

    assert_eq!(
        std::fs::read_to_string(dir.path().join("memory.md")).unwrap(),
        "user-edited"
    );
    assert_eq!(
        std::fs::read_to_string(dir.path().join("extra.md")).unwrap(),
        "bundled-extra"
    );
}

#[test]
fn list_reports_installed_when_present_in_either_agent_file() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join("memory.md"),
        "---\ntitle: Memory\nid: memory\nplacement: end\n---\nMEM\n",
    )
    .unwrap();
    std::fs::write(
        dir.path().join("other.md"),
        "---\ntitle: Other\nid: other\n---\nX\n",
    )
    .unwrap();

    let root = tempfile::tempdir().unwrap();
    // memory is installed in CLAUDE.md only; other in neither.
    std::fs::write(
        root.path().join("CLAUDE.md"),
        "# H\n\n<!-- code-basics: enhancement:memory -->\nMEM\n<!-- /code-basics: enhancement:memory -->\n",
    )
    .unwrap();

    let infos = list(dir.path(), root.path());
    let memory = infos.iter().find(|i| i.id == "memory").unwrap();
    let other = infos.iter().find(|i| i.id == "other").unwrap();
    assert!(memory.installed);
    assert!(!other.installed);
}

#[test]
fn add_and_remove_touch_both_agent_files() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join("memory.md"),
        "---\ntitle: Memory\nid: memory\nplacement: end\n---\nMEM\n",
    )
    .unwrap();

    let root = tempfile::tempdir().unwrap();
    std::fs::write(root.path().join("CLAUDE.md"), "# Claude\n").unwrap();
    std::fs::write(root.path().join("AGENTS.md"), "# Agents\n").unwrap();

    add(root.path(), dir.path(), "memory").unwrap();
    for name in ["CLAUDE.md", "AGENTS.md"] {
        let text = std::fs::read_to_string(root.path().join(name)).unwrap();
        assert!(
            text.contains("enhancement:memory"),
            "{name} got the section"
        );
    }

    let changed = remove_from_agents(root.path(), "memory").unwrap();
    assert_eq!(changed, 2);
    for name in ["CLAUDE.md", "AGENTS.md"] {
        let text = std::fs::read_to_string(root.path().join(name)).unwrap();
        assert!(
            !text.contains("enhancement:memory"),
            "{name} lost the section"
        );
    }
}

#[test]
fn adding_a_file_that_does_not_exist_creates_it() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join("memory.md"),
        "---\ntitle: Memory\nid: memory\nplacement: after-first-heading\n---\nMEM\n",
    )
    .unwrap();

    let root = tempfile::tempdir().unwrap();
    // Neither agent file exists yet.
    add(root.path(), dir.path(), "memory").unwrap();
    assert!(root.path().join("CLAUDE.md").exists());
    assert!(root.path().join("AGENTS.md").exists());
}

#[test]
fn list_prompts_returns_bodies_with_front_matter_stripped_sorted_by_title() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join("review.md"),
        "---\ntitle: Code Review\nid: review\n---\nReview this diff.\n",
    )
    .unwrap();
    std::fs::write(
        dir.path().join("tests.md"),
        "---\ntitle: Add Tests\nid: tests\n---\nWrite the failing test first.\n",
    )
    .unwrap();
    std::fs::write(dir.path().join("ignore.txt"), "not a prompt").unwrap();

    let prompts = list_prompts(dir.path());
    let titles: Vec<_> = prompts.iter().map(|p| p.title.as_str()).collect();
    assert_eq!(titles, ["Add Tests", "Code Review"]);

    let review = prompts.iter().find(|p| p.id == "review").unwrap();
    assert_eq!(review.body.trim(), "Review this diff.");
    // The front matter is not part of what gets copied.
    assert!(!review.body.contains("title:"));
}

#[test]
fn list_prompts_uses_the_file_stem_when_no_front_matter() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("bare.md"), "Just a raw prompt body.\n").unwrap();

    let prompts = list_prompts(dir.path());
    assert_eq!(prompts.len(), 1);
    assert_eq!(prompts[0].id, "bare");
    assert_eq!(prompts[0].title, "bare");
    assert_eq!(prompts[0].body.trim(), "Just a raw prompt body.");
}

// --- Saving a note as an instruction template ----------------------------

#[test]
fn slugify_makes_a_filesystem_safe_id() {
    assert_eq!(slugify("Deploy Steps"), "deploy-steps");
    assert_eq!(
        slugify("  Trim & Collapse  --  runs "),
        "trim-collapse-runs"
    );
    assert_eq!(slugify("CAPS_and_under"), "caps-and-under");
    // All punctuation / empty falls back rather than yielding an empty filename.
    assert_eq!(slugify("!!!"), "note");
    assert_eq!(slugify(""), "note");
}

#[test]
fn serialize_template_round_trips_through_parse_template() {
    let body = "Line one.\nLine two with `code` and a stray --- inside.";
    let text = serialize_template("deploy-steps", "Deploy Steps", body);
    let parsed = parse_template(&text, "deploy-steps");
    assert_eq!(parsed.id, "deploy-steps");
    assert_eq!(parsed.title, "Deploy Steps");
    assert_eq!(parsed.placement, Placement::End);
    assert_eq!(parsed.body, body, "body preserved verbatim");
}

#[test]
fn serialize_template_flattens_a_multiline_title() {
    let text = serialize_template("t", "Line A\nLine B", "body");
    let parsed = parse_template(&text, "t");
    assert_eq!(parsed.title, "Line A Line B");
}

#[test]
fn save_template_writes_a_discoverable_file() {
    let dir = tempfile::tempdir().unwrap();
    let path = save_template(dir.path(), "Deploy Steps", "Do the thing.").unwrap();
    assert_eq!(
        path.file_name().unwrap().to_str().unwrap(),
        "deploy-steps.md"
    );
    assert!(path.exists());

    // It shows up in the instruction listing and as a runnable prompt.
    let templates = discover(dir.path());
    assert_eq!(templates.len(), 1);
    assert_eq!(templates[0].id, "deploy-steps");
    assert_eq!(templates[0].title, "Deploy Steps");
    assert_eq!(templates[0].body.trim(), "Do the thing.");
}

#[test]
fn save_template_refreshes_a_same_slug_file_rather_than_duplicating() {
    let dir = tempfile::tempdir().unwrap();
    save_template(dir.path(), "Deploy Steps", "first").unwrap();
    save_template(dir.path(), "Deploy  Steps", "second").unwrap();
    let templates = discover(dir.path());
    assert_eq!(templates.len(), 1, "same slug refreshes, not duplicates");
    assert_eq!(templates[0].body.trim(), "second");
}

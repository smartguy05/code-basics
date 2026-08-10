//! Tests for the label request appended to an agent's instruction file.
//! Included by `instructions.rs` under `#[cfg(test)]`.

use super::*;

fn workspace() -> tempfile::TempDir {
    tempfile::tempdir().unwrap()
}

#[test]
fn each_agent_gets_the_instruction_file_it_actually_reads() {
    let dir = workspace();

    assert!(path_for(ProviderId::ClaudeCode, dir.path()).ends_with("CLAUDE.md"));
    assert!(path_for(ProviderId::Codex, dir.path()).ends_with("AGENTS.md"));
}

#[test]
fn a_workspace_with_no_instruction_file_gets_one_created() {
    let dir = workspace();

    let write = planned_write(ProviderId::Codex, dir.path()).expect("a write");

    assert!(!write.merges_existing);
    assert!(write.content.contains("Intent:"));
    assert!(write.content.contains(MARKER));
}

/// The file is the user's own; appending to it must not disturb what is there.
#[test]
fn an_existing_instruction_file_keeps_everything_it_had() {
    let dir = workspace();
    let path = dir.path().join("CLAUDE.md");
    std::fs::write(&path, "# My project\n\nDo not use tabs.\n").unwrap();

    let write = planned_write(ProviderId::ClaudeCode, dir.path()).expect("a write");

    assert!(write.merges_existing);
    assert!(write.content.starts_with("# My project"));
    assert!(write.content.contains("Do not use tabs."));
    assert!(write.content.contains("Intent:"));
}

#[test]
fn the_appended_section_is_separated_by_exactly_one_blank_line() {
    let dir = workspace();
    let path = dir.path().join("AGENTS.md");
    std::fs::write(&path, "Existing guidance.\n\n\n\n").unwrap();

    let write = planned_write(ProviderId::Codex, dir.path()).expect("a write");

    assert!(
        write
            .content
            .contains("Existing guidance.\n\n<!-- code-basics"),
        "got: {:?}",
        write.content
    );
}

#[test]
fn a_file_without_a_trailing_newline_is_still_separated_properly() {
    let dir = workspace();
    let path = dir.path().join("AGENTS.md");
    std::fs::write(&path, "No trailing newline").unwrap();

    let write = planned_write(ProviderId::Codex, dir.path()).expect("a write");

    assert!(write
        .content
        .contains("No trailing newline\n\n<!-- code-basics"));
}

/// Re-running setup after the request's wording changed must bring the file
/// up to date: the marked span is replaced in place, everything the user
/// wrote around it untouched.
#[test]
fn an_out_of_date_section_is_rewritten_in_place() {
    let dir = workspace();
    let path = dir.path().join("CLAUDE.md");
    std::fs::write(
        &path,
        format!(
            "# My project\n\nDo not use tabs.\n\n{MARKER}\n## Old heading\n\nStale request \
             wording.\n{END_MARKER}\n\n## After\n\nKeep this too.\n"
        ),
    )
    .unwrap();

    let write = planned_write(ProviderId::ClaudeCode, dir.path()).expect("a rewrite");

    assert!(write.merges_existing);
    assert!(write.content.starts_with("# My project"));
    assert!(write.content.contains("Do not use tabs."));
    assert!(write.content.contains("Keep this too."));
    assert!(write.content.contains("Intent: "));
    assert!(!write.content.contains("Stale request wording."));
    assert_eq!(write.content.matches(MARKER).count(), 1);
}

/// A marker whose end never arrives marks a span with no known extent.
/// Rewriting would mean guessing where the user's own text begins — abstain.
#[test]
fn a_marker_without_its_end_is_left_alone() {
    let dir = workspace();
    let path = dir.path().join("CLAUDE.md");
    std::fs::write(&path, format!("{MARKER}\nHalf a section, no end.\n")).unwrap();

    assert!(planned_write(ProviderId::ClaudeCode, dir.path()).is_none());
}

/// Installing twice must not append the request twice.
#[test]
fn a_file_that_already_has_the_request_needs_no_write() {
    let dir = workspace();
    let path = dir.path().join("CLAUDE.md");
    let first = planned_write(ProviderId::ClaudeCode, dir.path()).expect("a write");
    std::fs::write(&path, &first.content).unwrap();

    assert!(planned_write(ProviderId::ClaudeCode, dir.path()).is_none());
}

#[test]
fn the_request_is_recognised_once_written() {
    let dir = workspace();
    let path = dir.path().join("CLAUDE.md");
    assert!(!is_present(&path));

    let write = planned_write(ProviderId::ClaudeCode, dir.path()).unwrap();
    std::fs::write(&path, write.content).unwrap();

    assert!(is_present(&path));
}

/// The instruction has to describe both forms, or the parser's file-scoped
/// variant is unreachable in practice. The scoped example must show a
/// comma-separated list, or nobody learns the parser accepts one, and the
/// paths must be called workspace-relative, or an absolute path silently
/// never joins (`label_for` compares against workspace-relative records).
#[test]
fn the_request_describes_both_the_plain_and_file_scoped_forms() {
    let dir = workspace();
    let write = planned_write(ProviderId::Codex, dir.path()).unwrap();

    assert!(write.content.contains("Intent: "));
    assert!(write
        .content
        .contains("Intent(src/api.ts, src/apiLogic.test.ts):"));
    assert!(write.content.contains("workspace-relative"));
}

/// `label_for` falls back to the FIRST plain label for every edit in the
/// turn, so several plain lines cannot disambiguate anything. The request
/// must say so, or "one line per distinct change" invites exactly that.
#[test]
fn the_request_warns_that_extra_plain_lines_are_ignored() {
    let dir = workspace();
    let write = planned_write(ProviderId::Codex, dir.path()).unwrap();

    assert!(write.content.contains("only the first plain line is used"));
}

/// What the instruction asks for must be what the Stop hook can read back.
#[test]
fn the_requested_form_is_one_the_hook_can_parse() {
    let dir = workspace();
    let write = planned_write(ProviderId::Codex, dir.path()).unwrap();

    // Take the example lines out of the section and run them through the
    // parser the Stop hook uses.
    let example = write
        .content
        .lines()
        .find(|l| l.trim().starts_with("Intent:"))
        .expect("a plain example");
    let scoped = write
        .content
        .lines()
        .find(|l| l.trim().starts_with("Intent("))
        .expect("a scoped example");

    let plain = crate::intents::hook::parse_labels(example);
    assert_eq!(plain.len(), 1, "the plain example must parse");
    assert!(plain[0].0.is_empty());

    let scoped = crate::intents::hook::parse_labels(scoped);
    assert_eq!(scoped.len(), 1, "the scoped example must parse");
    assert_eq!(scoped[0].0, vec!["src/api.ts", "src/apiLogic.test.ts"]);
}

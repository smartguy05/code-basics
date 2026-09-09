//! The two phases, every refusal, and the rollback.
//!
//! All of it runs against an injected [`Files`], for the same reason
//! [`super::super::results::TextProvider`] and
//! [`super::super::registry::Probe`] are injected: the interesting cases here
//! are the ones a developer's disk will not produce on demand. A write that
//! fails on the third file, and a *restore* that fails after it, are the only
//! paths that reach the rollback at all — and the second one is the single
//! unrecoverable state in the feature.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use super::*;
use crate::lsp::protocol::{DocumentEdits, Position, Range, ResourceOperation, TextEdit};
use crate::lsp::uri::{to_file_uri, UriStyle};

// ---------------------------------------------------------------------------
// Harness
// ---------------------------------------------------------------------------

/// A workspace root that is absolute on both platforms without being real.
///
/// Absolute matters: [`crate::symbols::index::relative_to_root`] strips a
/// prefix, and [`to_file_uri`] refuses a relative path outright, so a test using
/// a bare `"root"` would be testing the abstention rather than the rename.
fn root() -> PathBuf {
    if cfg!(windows) {
        PathBuf::from(r"C:\ws")
    } else {
        PathBuf::from("/ws")
    }
}

fn absolute(name: &str) -> PathBuf {
    let mut path = root();
    for part in name.split('/') {
        path.push(part);
    }
    path
}

fn uri(name: &str) -> String {
    to_file_uri(&absolute(name), UriStyle::Encoded).expect("an absolute path")
}

/// A whole-identifier replacement, the shape a conventional server sends.
fn replace(line: u32, start: u32, end: u32, text: &str) -> TextEdit {
    TextEdit {
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
        new_text: text.to_string(),
    }
}

/// A zero-width insertion, the shape the real Roslyn server sends.
fn insert(line: u32, at: u32, text: &str) -> TextEdit {
    replace(line, at, at, text)
}

fn document(name: &str, edits: Vec<TextEdit>) -> DocumentEdits {
    DocumentEdits {
        uri: uri(name),
        edits,
    }
}

fn edit(documents: Vec<DocumentEdits>) -> WorkspaceEdit {
    WorkspaceEdit {
        documents,
        resource_operations: Vec::new(),
    }
}

/// The disk, as a map, with every failure mode arrangeable.
struct FakeFiles {
    contents: BTreeMap<PathBuf, String>,
    /// Every write in order, so "phase 1 wrote nothing" is assertable.
    writes: Vec<(PathBuf, String)>,
    /// Fail the write with this 1-based ordinal. Restores count as writes, so
    /// the restores that follow are ordinals `n + 1`, `n + 2`, …
    fail_write: Option<usize>,
    /// Fail every write after the one `fail_write` names — i.e. every restore.
    fail_restore: bool,
    /// Model a **non-atomic** write: a failing write leaves the file truncated.
    ///
    /// This is what `std::fs::write` — and so `crate::files::write_file`, and so
    /// [`RealFiles`] — actually does: it truncates at open and then writes, so an
    /// error part-way through leaves a short file. The fake used to insert the
    /// new contents only on success, i.e. it modelled every write as atomic, and
    /// that modelling assumption is what hid this whole class of failure.
    truncating: bool,
    /// Make the file unreadable from the moment a write to it fails, so the
    /// verification read cannot establish anything either.
    unreadable_after_failure: bool,
    unreadable: BTreeSet<PathBuf>,
}

impl FakeFiles {
    fn new(files: &[(&str, &str)]) -> Self {
        Self {
            contents: files
                .iter()
                .map(|(name, text)| (absolute(name), (*text).to_string()))
                .collect(),
            writes: Vec::new(),
            fail_write: None,
            fail_restore: false,
            truncating: false,
            unreadable_after_failure: false,
            unreadable: BTreeSet::new(),
        }
    }

    fn text(&self, name: &str) -> &str {
        self.contents
            .get(&absolute(name))
            .map(String::as_str)
            .unwrap_or("<missing>")
    }
}

impl Files for FakeFiles {
    fn read(&mut self, path: &Path) -> Result<String, String> {
        if self.unreadable.contains(path) {
            return Err(format!("{} is unreadable in this test", path.display()));
        }
        self.contents
            .get(path)
            .cloned()
            .ok_or_else(|| format!("{} does not exist", path.display()))
    }

    fn write(&mut self, path: &Path, text: &str) -> Result<(), String> {
        self.writes.push((path.to_path_buf(), text.to_string()));
        let ordinal = self.writes.len();
        if let Some(fails_at) = self.fail_write {
            if ordinal == fails_at || (self.fail_restore && ordinal > fails_at) {
                if self.truncating {
                    // Half the bytes landed and the rest did not. Exactly what a
                    // truncate-then-write leaves behind.
                    let half = text.len() / 2;
                    let cut = text
                        .char_indices()
                        .map(|(index, _)| index)
                        .take_while(|index| *index <= half)
                        .last()
                        .unwrap_or(0);
                    self.contents
                        .insert(path.to_path_buf(), text[..cut].to_string());
                }
                if self.unreadable_after_failure {
                    self.unreadable.insert(path.to_path_buf());
                }
                return Err(format!("{} refused the write", path.display()));
            }
        }
        self.contents.insert(path.to_path_buf(), text.to_string());
        Ok(())
    }
}

fn open(names: &[&str]) -> BTreeSet<PathBuf> {
    names.iter().map(|name| absolute(name)).collect()
}

fn none() -> BTreeSet<PathBuf> {
    BTreeSet::new()
}

fn relative(name: &str) -> PathBuf {
    PathBuf::from(name.replace('/', std::path::MAIN_SEPARATOR_STR))
}

fn message_of(result: &RenameResult) -> String {
    result
        .message
        .clone()
        .unwrap_or_else(|| panic!("expected a reason: {result:?}"))
}

// ---------------------------------------------------------------------------
// The enclosing-identifier check
// ---------------------------------------------------------------------------

#[test]
fn the_enclosing_token_is_found_from_anywhere_inside_it() {
    let text = "var total = new Walker();\n";
    // `Walker` runs from column 16 to 22, and every position inside it —
    // including both edges — has to name the same token, because a server may
    // aim an insertion at any of them.
    for column in 16..=22 {
        assert_eq!(
            enclosing_identifier(text, 0, column),
            Some("Walker"),
            "column {column}"
        );
    }
}

#[test]
fn a_position_that_is_not_in_an_identifier_names_no_token() {
    let text = "a = b;\n";
    // A position whose neighbours on *both* sides are non-word characters names
    // nothing. The position immediately after `a` is deliberately not one of
    // these: it is that token's end boundary, and a server may aim an edit there
    // — see `the_enclosing_token_is_found_from_anywhere_inside_it`.
    assert_eq!(
        enclosing_identifier(text, 0, 1),
        Some("a"),
        "the end of `a`"
    );
    assert_eq!(enclosing_identifier(text, 0, 2), None, "the `=`");
    assert_eq!(enclosing_identifier(text, 0, 6), None, "past the `;`");
    assert_eq!(enclosing_identifier("   \n", 0, 1), None, "only spaces");
}

#[test]
fn a_token_may_contain_underscores_digits_and_non_ascii_letters() {
    // Refusing a non-ASCII identifier would refuse a correct rename, which is
    // exactly what this check must not do.
    assert_eq!(
        enclosing_identifier("let _order_2 = 1;\n", 0, 6),
        Some("_order_2")
    );
    assert_eq!(enclosing_identifier("let café = 1;\n", 0, 5), Some("café"));
}

#[test]
fn a_dollar_is_a_word_character_because_the_frontend_reads_one_into_the_name() {
    // `renameLogic.ts`'s WORD is `/[\p{L}\p{N}_$]/u`, and the string it reads out
    // of the buffer is the very `expect` this side compares against. A `$`
    // excluded here can therefore never equal a name that includes one, so every
    // `$scope`, `users$` or `$` in a JS/TS workspace failed the token check on
    // every closed file and the whole rename was refused with a stale-mirror
    // accusation about a perfectly current server. One rule, two destinations.
    assert_eq!(
        enclosing_identifier("const $scope = 1;\n", 0, 6),
        Some("$scope")
    );
    assert_eq!(
        enclosing_identifier("const users$ = of(1);\n", 0, 8),
        Some("users$")
    );
}

#[test]
fn a_dollar_prefixed_name_renames_a_closed_file_instead_of_being_refused() {
    let mut files = FakeFiles::new(&[("a.js", "const $scope = 1;\nuse($scope);\n")]);
    let edit = edit(vec![document(
        "a.js",
        vec![replace(0, 6, 12, "$state"), replace(1, 4, 10, "$state")],
    )]);

    let result = apply_workspace_edit(&root(), &edit, &none(), "$scope", &mut files);

    assert_eq!(result.outcome, Availability::Ready, "{result:?}");
    assert_eq!(files.text("a.js"), "const $state = 1;\nuse($state);\n");
}

#[test]
fn the_token_lookup_never_panics_on_a_position_the_document_does_not_have() {
    // `positions::byte_offset` clamps rather than refusing, and this is a
    // diagnostic helper, so a nonsense position has to produce an answer.
    for (line, character) in [(0, 999), (99, 0), (u32::MAX, u32::MAX)] {
        let _ = enclosing_identifier("a\r\nbé\n", line, character);
    }
}

// ---------------------------------------------------------------------------
// Refusals: the whole rename, having touched nothing
// ---------------------------------------------------------------------------

#[test]
fn a_resource_operation_refuses_the_whole_rename_and_names_the_kind_and_the_uris() {
    // The real safeguard against file operations. The client *declares*
    // `resourceOperations: []`, but the real Roslyn server ignores the
    // neighbouring `documentChanges: false` outright, so the declaration is not
    // a guarantee — the refusal is. It is keyed on the list being non-empty and
    // never on a match over known kinds, so a kind added to the protocol after
    // this was written is still declined.
    let mut files = FakeFiles::new(&[("a.cs", "class Walker {}\n")]);
    let edit = WorkspaceEdit {
        documents: vec![document("a.cs", vec![replace(0, 6, 12, "HeapWalker")])],
        resource_operations: vec![ResourceOperation {
            kind: "rename".into(),
            uris: vec![uri("a.cs"), uri("HeapWalker.cs")],
        }],
    };

    let result = apply_workspace_edit(&root(), &edit, &none(), "Walker", &mut files);

    assert_eq!(result.outcome, Availability::Failed);
    assert_eq!(result.total, None, "a refusal is not a count of zero");
    assert!(result.written.is_empty());
    assert!(result.buffers.is_empty());
    assert!(files.writes.is_empty(), "nothing may be written");
    let message = message_of(&result);
    assert!(
        message.contains("rename") && message.contains("HeapWalker.cs"),
        "the refusal must name the operation and what it was about: {message}"
    );
}

#[test]
fn a_uri_that_is_not_a_file_uri_refuses_the_whole_rename() {
    // Unlike find-usages, where an unopenable *row* is merely shown: an edit
    // that cannot be applied means a partial rename, and a half-done rename does
    // not compile.
    let mut files = FakeFiles::new(&[("a.cs", "class Walker {}\n")]);
    let edit = edit(vec![
        document("a.cs", vec![replace(0, 6, 12, "HeapWalker")]),
        DocumentEdits {
            uri: "source-generated:/Generated.cs".into(),
            edits: vec![replace(0, 0, 6, "HeapWalker")],
        },
    ]);

    let result = apply_workspace_edit(&root(), &edit, &none(), "Walker", &mut files);

    assert_eq!(result.outcome, Availability::Failed);
    assert_eq!(result.total, None);
    assert!(files.writes.is_empty(), "not even the resolvable file");
    let message = message_of(&result);
    assert!(
        message.contains("source-generated:/Generated.cs"),
        "the refusal must name the document it could not place: {message}"
    );
}

#[test]
fn a_uri_outside_the_workspace_refuses_the_whole_rename() {
    let mut files = FakeFiles::new(&[("a.cs", "class Walker {}\n")]);
    let outside = if cfg!(windows) {
        PathBuf::from(r"C:\elsewhere\b.cs")
    } else {
        PathBuf::from("/elsewhere/b.cs")
    };
    let edit = edit(vec![
        document("a.cs", vec![replace(0, 6, 12, "HeapWalker")]),
        DocumentEdits {
            uri: to_file_uri(&outside, UriStyle::Encoded).expect("absolute"),
            edits: vec![replace(0, 0, 6, "HeapWalker")],
        },
    ]);

    let result = apply_workspace_edit(&root(), &edit, &none(), "Walker", &mut files);

    assert_eq!(result.outcome, Availability::Failed);
    assert!(files.writes.is_empty());
    let message = message_of(&result);
    assert!(
        message.contains("b.cs") && message.contains("workspace"),
        "the refusal must say the file is outside the workspace: {message}"
    );
}

#[test]
fn overlapping_edits_refuse_the_whole_rename_before_a_file_is_read() {
    // `edits::plan` takes no text precisely so this happens in phase 1.
    let mut files = FakeFiles::new(&[("a.cs", "class Walker {}\n")]);
    let edit = edit(vec![document(
        "a.cs",
        vec![replace(0, 6, 12, "HeapWalker"), replace(0, 8, 14, "Other")],
    )]);

    let result = apply_workspace_edit(&root(), &edit, &none(), "Walker", &mut files);

    assert_eq!(result.outcome, Availability::Failed);
    assert!(files.writes.is_empty());
    let message = message_of(&result);
    assert!(
        message.contains("a.cs") && message.contains("overlap"),
        "the refusal must name the file and the problem: {message}"
    );
}

#[test]
fn an_unreadable_closed_file_refuses_the_whole_rename_and_writes_nothing() {
    let mut files = FakeFiles::new(&[
        ("a.cs", "class Walker {}\n"),
        ("b.cs", "var w = new Walker();\n"),
    ]);
    files.unreadable.insert(absolute("b.cs"));
    let edit = edit(vec![
        document("a.cs", vec![replace(0, 6, 12, "HeapWalker")]),
        document("b.cs", vec![replace(0, 12, 18, "HeapWalker")]),
    ]);

    let result = apply_workspace_edit(&root(), &edit, &none(), "Walker", &mut files);

    assert_eq!(result.outcome, Availability::Failed);
    assert!(
        files.writes.is_empty(),
        "`a.cs` must not be written just because it happened to sort first"
    );
    assert!(message_of(&result).contains("b.cs"));
}

#[test]
fn a_closed_file_too_large_to_edit_refuses_the_whole_rename() {
    let big = "x".repeat(crate::files::MAX_EDITABLE_BYTES as usize + 1);
    let mut files = FakeFiles::new(&[("big.cs", &big)]);
    let edit = edit(vec![document("big.cs", vec![replace(0, 0, 1, "y")])]);

    let result = apply_workspace_edit(&root(), &edit, &none(), "x", &mut files);

    assert_eq!(result.outcome, Availability::Failed);
    assert!(files.writes.is_empty());
    let message = message_of(&result);
    assert!(
        message.contains("big.cs") && message.contains("too large"),
        "{message}"
    );
}

#[test]
fn pre_images_over_the_total_cap_are_refused_in_phase_one_rather_than_held() {
    // The rollback needs every closed file's previous contents in memory at
    // once, so the bound has to be a refusal in phase 1 and not an allocation
    // failure in phase 2.
    // Real identifiers, because the token check runs per file and would
    // otherwise refuse the first one before the cap was reached — the test would
    // then pass for the wrong reason. Nine files of ~4 MB clears 32 MB.
    let one = format!("Walker;\n{}", ";\n".repeat(2 * 1024 * 1024));
    let names: Vec<String> = (0..9).map(|n| format!("f{n}.cs")).collect();
    let files_on_disk: Vec<(&str, &str)> = names
        .iter()
        .map(|name| (name.as_str(), one.as_str()))
        .collect();
    let mut files = FakeFiles::new(&files_on_disk);
    let edit = edit(
        names
            .iter()
            .map(|name| document(name, vec![replace(0, 0, 6, "HeapWalker")]))
            .collect(),
    );

    let result = apply_workspace_edit(&root(), &edit, &none(), "Walker", &mut files);

    assert_eq!(result.outcome, Availability::Failed);
    assert!(files.writes.is_empty());
    let message = message_of(&result);
    assert!(
        message.contains("too much text"),
        "the refusal must say what the bound is about: {message}"
    );
}

// ---------------------------------------------------------------------------
// The stale-mirror token check
// ---------------------------------------------------------------------------

#[test]
fn a_zero_width_insertion_inside_the_old_identifier_passes_the_token_check() {
    // The measured Roslyn shape: renaming `Walker` to `HeapWalker` inserts
    // `"Heap"` at the start of the identifier rather than replacing it. The
    // plan's original rule — "the replaced text contains the old name" — would
    // refuse every Roslyn rename, because an insertion replaces the empty
    // string. This is the rule that replaced it.
    let mut files = FakeFiles::new(&[("b.cs", "var w = new Walker();\n")]);
    let edit = edit(vec![document("b.cs", vec![insert(0, 12, "Heap")])]);

    let result = apply_workspace_edit(&root(), &edit, &none(), "Walker", &mut files);

    assert_eq!(result.outcome, Availability::Ready, "{result:?}");
    assert_eq!(result.total, Some(1));
    assert_eq!(result.message, None, "a clean rename qualifies nothing");
    assert_eq!(files.text("b.cs"), "var w = new HeapWalker();\n");
}

#[test]
fn a_file_where_no_edit_names_the_old_identifier_refuses_the_whole_rename() {
    // The stale-mirror case this check exists for: the server computed its
    // ranges from text this app does not have, so the edits land on something
    // else entirely.
    let mut files = FakeFiles::new(&[("b.cs", "var somethingElse = 1;\n")]);
    let edit = edit(vec![document("b.cs", vec![replace(0, 4, 17, "Renamed")])]);

    let result = apply_workspace_edit(&root(), &edit, &none(), "Walker", &mut files);

    assert_eq!(result.outcome, Availability::Failed);
    assert!(files.writes.is_empty());
    let message = message_of(&result);
    assert!(
        message.contains("b.cs") && message.contains("Walker"),
        "the refusal must name the file and the identifier it expected: {message}"
    );
}

#[test]
fn a_file_where_only_some_edits_name_the_old_identifier_is_applied_and_says_so() {
    // Rename-in-comments and TypeScript's shorthand-property expansion
    // (`{ foo }` becomes `{ newFoo: foo }`) both legitimately touch text that is
    // not the bare identifier. Refusing on the first non-matching edit would
    // refuse correct renames, so the disagreement goes in the message.
    let mut files = FakeFiles::new(&[("b.cs", "// Walker does things\nvar w = new Walker();\n")]);
    let edit = edit(vec![document(
        "b.cs",
        vec![
            // The comment's own word, replaced wholesale rather than as a token.
            replace(0, 2, 21, " HeapWalker does things"),
            insert(1, 12, "Heap"),
        ],
    )]);

    let result = apply_workspace_edit(&root(), &edit, &none(), "Walker", &mut files);

    assert_eq!(result.outcome, Availability::Ready, "{result:?}");
    assert_eq!(result.total, Some(2));
    let message = message_of(&result);
    assert!(
        message.contains("b.cs"),
        "the qualification must name the file it is about: {message}"
    );
    assert_eq!(
        files.text("b.cs"),
        "// HeapWalker does things\nvar w = new HeapWalker();\n"
    );
}

#[test]
fn an_empty_expected_name_abstains_from_the_token_check_rather_than_refusing_everything() {
    // There is nothing to compare against, so the check has no opinion. The
    // alternative — comparing against `""`, which no token equals — would refuse
    // every rename.
    let mut files = FakeFiles::new(&[("b.cs", "var w = new Walker();\n")]);
    let edit = edit(vec![document("b.cs", vec![insert(0, 12, "Heap")])]);

    let result = apply_workspace_edit(&root(), &edit, &none(), "", &mut files);

    assert_eq!(result.outcome, Availability::Ready, "{result:?}");
    assert_eq!(files.text("b.cs"), "var w = new HeapWalker();\n");
}

// ---------------------------------------------------------------------------
// The split: closed files written, open buffers returned
// ---------------------------------------------------------------------------

#[test]
fn zero_documents_is_ready_with_a_total_of_zero_and_not_a_refusal() {
    // `Some(0)` and `None` are different answers, and this is the one that says
    // the server found nothing to change.
    let mut files = FakeFiles::new(&[]);
    let result = apply_workspace_edit(&root(), &edit(vec![]), &none(), "Walker", &mut files);

    assert_eq!(result.outcome, Availability::Ready);
    assert_eq!(result.total, Some(0));
    assert!(result.written.is_empty());
    assert!(result.buffers.is_empty());
    assert!(result.failures.is_empty());
    assert_eq!(result.message, None);
}

#[test]
fn a_document_the_server_named_but_left_unedited_is_neither_read_nor_written() {
    // A `DocumentEdits` with an empty `edits` list is legal: the server named
    // the file and said it needs no changes. Reading it would refuse the whole
    // rename if it happened to be unreadable, over a file nothing was going to
    // change.
    let mut files = FakeFiles::new(&[("b.cs", "var w = new Walker();\n")]);
    files.unreadable.insert(absolute("gone.cs"));
    let edit = edit(vec![
        document("gone.cs", vec![]),
        document("b.cs", vec![insert(0, 12, "Heap")]),
    ]);

    let result = apply_workspace_edit(&root(), &edit, &none(), "Walker", &mut files);

    assert_eq!(result.outcome, Availability::Ready, "{result:?}");
    assert_eq!(result.total, Some(1));
    assert_eq!(result.written.len(), 1, "only the file that changed");
    assert_eq!(result.written[0].path, relative("b.cs"));
}

#[test]
fn an_open_buffer_is_never_written_to_disk_and_its_edits_come_back_instead() {
    // The clobber bug this split exists to prevent: `FileEditor`'s build effect
    // is keyed on identity alone, there is no file watcher and there is no
    // autosave, so a disk write behind an open tab is invisible to the buffer
    // and is overwritten by the next `Ctrl+S`.
    let mut files = FakeFiles::new(&[
        ("open.cs", "class Walker {}\n"),
        ("closed.cs", "var w = new Walker();\n"),
    ]);
    let edit = edit(vec![
        document("open.cs", vec![replace(0, 6, 12, "HeapWalker")]),
        document("closed.cs", vec![insert(0, 12, "Heap")]),
    ]);

    let result = apply_workspace_edit(&root(), &edit, &open(&["open.cs"]), "Walker", &mut files);

    assert_eq!(result.outcome, Availability::Ready, "{result:?}");
    assert_eq!(result.total, Some(2));
    assert_eq!(
        files.text("open.cs"),
        "class Walker {}\n",
        "the open buffer's file is untouched on disk"
    );
    assert_eq!(files.text("closed.cs"), "var w = new HeapWalker();\n");
    assert_eq!(
        files.writes.iter().map(|(p, _)| p).collect::<Vec<_>>(),
        vec![&absolute("closed.cs")]
    );

    assert_eq!(result.written.len(), 1);
    assert_eq!(result.written[0].path, relative("closed.cs"));
    assert_eq!(result.written[0].edits, 1);

    assert_eq!(result.buffers.len(), 1);
    let buffer = &result.buffers[0];
    assert_eq!(buffer.path, relative("open.cs"));
    assert_eq!(buffer.edits.len(), 1);
    let edit = &buffer.edits[0];
    assert_eq!(
        (edit.start_line, edit.start_character),
        (1, 6),
        "1-based line, 0-based UTF-16 character"
    );
    assert_eq!((edit.end_line, edit.end_character), (1, 12));
    assert_eq!(edit.new_text, "HeapWalker");
}

#[test]
fn an_open_buffer_is_not_read_at_all_so_an_unreadable_one_is_not_a_refusal() {
    // The buffer is the truth for an open file, and its text does not exist on
    // this side of the boundary. A file that is mid-save, locked, or has never
    // been written to disk must not refuse the rename.
    let mut files = FakeFiles::new(&[]);
    let edit = edit(vec![document(
        "open.cs",
        vec![replace(0, 6, 12, "HeapWalker")],
    )]);

    let result = apply_workspace_edit(&root(), &edit, &open(&["open.cs"]), "Walker", &mut files);

    assert_eq!(result.outcome, Availability::Ready, "{result:?}");
    assert_eq!(result.buffers.len(), 1);
    assert!(files.writes.is_empty());
}

#[test]
fn edits_for_two_spellings_of_one_path_are_planned_together() {
    // The decoder merges `documentChanges` entries by *uri string*, and Roslyn
    // spells a drive colon plainly while rust-analyzer percent-encodes it. Two
    // spellings of one path therefore arrive as two entries, and applying them
    // separately would compute the second against text the first had already
    // changed. Here they overlap, so planning them together is what catches it.
    let mut files = FakeFiles::new(&[("a.cs", "class Walker {}\n")]);
    let plain = to_file_uri(&absolute("a.cs"), UriStyle::Plain).expect("absolute");
    let encoded = to_file_uri(&absolute("a.cs"), UriStyle::Encoded).expect("absolute");
    let edit = edit(vec![
        DocumentEdits {
            uri: plain,
            edits: vec![replace(0, 6, 12, "HeapWalker")],
        },
        DocumentEdits {
            uri: encoded,
            edits: vec![replace(0, 8, 14, "Other")],
        },
    ]);

    let result = apply_workspace_edit(&root(), &edit, &none(), "Walker", &mut files);

    assert_eq!(
        result.outcome,
        Availability::Failed,
        "two spellings of one path must be one file: {result:?}"
    );
    assert!(files.writes.is_empty());
    assert!(message_of(&result).contains("overlap"));
}

#[test]
fn the_total_counts_every_edit_across_every_file() {
    let mut files = FakeFiles::new(&[
        ("a.cs", "class Walker {}\n"),
        ("b.cs", "var w = new Walker();\nvar x = new Walker();\n"),
    ]);
    let edit = edit(vec![
        document("a.cs", vec![insert(0, 6, "Heap")]),
        document("b.cs", vec![insert(0, 12, "Heap"), insert(1, 12, "Heap")]),
    ]);

    let result = apply_workspace_edit(&root(), &edit, &open(&["a.cs"]), "Walker", &mut files);

    assert_eq!(result.outcome, Availability::Ready, "{result:?}");
    assert_eq!(
        result.total,
        Some(3),
        "one buffer edit and two written ones"
    );
    assert_eq!(
        files.text("b.cs"),
        "var w = new HeapWalker();\nvar x = new HeapWalker();\n"
    );
}

// ---------------------------------------------------------------------------
// Rollback
// ---------------------------------------------------------------------------

#[test]
fn a_failed_write_restores_every_file_written_before_it() {
    // Reachable only through the injected `Files`, which is why it is injected.
    let mut files = FakeFiles::new(&[
        ("a.cs", "class Walker {}\n"),
        ("b.cs", "var w = new Walker();\n"),
        ("c.cs", "var x = new Walker();\n"),
    ]);
    files.fail_write = Some(3);
    let edit = edit(vec![
        document("a.cs", vec![insert(0, 6, "Heap")]),
        document("b.cs", vec![insert(0, 12, "Heap")]),
        document("c.cs", vec![insert(0, 12, "Heap")]),
    ]);

    let result = apply_workspace_edit(&root(), &edit, &none(), "Walker", &mut files);

    assert_eq!(result.outcome, Availability::Failed);
    assert_eq!(result.total, None, "an incomplete rename is not a count");
    assert!(
        result.written.is_empty(),
        "the earlier writes were undone, so nothing was written: {result:?}"
    );
    assert!(
        result.buffers.is_empty(),
        "the editor must not apply half a rename"
    );
    assert_eq!(files.text("a.cs"), "class Walker {}\n");
    assert_eq!(files.text("b.cs"), "var w = new Walker();\n");
    assert_eq!(files.text("c.cs"), "var x = new Walker();\n");

    let failure = result
        .failures
        .iter()
        .find(|failure| failure.path == relative("c.cs"))
        .unwrap_or_else(|| panic!("the failed write must be reported: {result:?}"));
    assert!(!failure.unrecoverable, "everything was restored");
    assert!(
        result.failures.iter().all(|failure| !failure.unrecoverable),
        "{result:?}"
    );
}

#[test]
fn a_restore_that_also_fails_is_reported_as_unrecoverable_and_names_git() {
    // The only genuinely unrecoverable state in the feature: a file holds half a
    // rename and this app no longer has anywhere to put its previous contents.
    let mut files = FakeFiles::new(&[
        ("a.cs", "class Walker {}\n"),
        ("b.cs", "var w = new Walker();\n"),
    ]);
    files.fail_write = Some(2);
    files.fail_restore = true;
    let edit = edit(vec![
        document("a.cs", vec![insert(0, 6, "Heap")]),
        document("b.cs", vec![insert(0, 12, "Heap")]),
    ]);

    let result = apply_workspace_edit(&root(), &edit, &none(), "Walker", &mut files);

    assert_eq!(result.outcome, Availability::Failed);
    assert!(result.written.is_empty());
    let unrecoverable: Vec<&RenameFailure> = result
        .failures
        .iter()
        .filter(|failure| failure.unrecoverable)
        .collect();
    assert_eq!(
        unrecoverable.len(),
        1,
        "exactly the file whose restore failed: {result:?}"
    );
    assert_eq!(unrecoverable[0].path, relative("a.cs"));
    assert!(
        unrecoverable[0].detail.contains("git"),
        "the strongest wording in the feature must send the user to git: {}",
        unrecoverable[0].detail
    );
    assert_eq!(
        files.text("a.cs"),
        "class HeapWalker {}\n",
        "the point of the flag: this file really is left changed"
    );
}

#[test]
fn a_write_that_failed_after_truncating_the_file_puts_the_pre_image_back() {
    // The write is not atomic: `crate::files::write_file` is `std::fs::write`,
    // which truncates at open. So "the write returned an error" does NOT
    // establish "this file was not changed" — and the code used to assert
    // exactly that, telling the user every file had been restored while one of
    // them sat on disk holding half its contents, which existed nowhere else.
    // The pre-image is in memory throughout; put it back and then say so.
    let mut files = FakeFiles::new(&[
        ("a.cs", "class Walker {}\n"),
        ("b.cs", "var w = new Walker();\n"),
    ]);
    files.fail_write = Some(2);
    files.truncating = true;
    let edit = edit(vec![
        document("a.cs", vec![insert(0, 6, "Heap")]),
        document("b.cs", vec![insert(0, 12, "Heap")]),
    ]);

    let result = apply_workspace_edit(&root(), &edit, &none(), "Walker", &mut files);

    assert_eq!(result.outcome, Availability::Failed);
    assert_eq!(
        files.text("b.cs"),
        "var w = new Walker();\n",
        "the file whose write failed part-way must be put back: {result:?}"
    );
    assert_eq!(
        files.text("a.cs"),
        "class Walker {}\n",
        "and the earlier one"
    );
    let failure = result
        .failures
        .iter()
        .find(|failure| failure.path == relative("b.cs"))
        .unwrap_or_else(|| panic!("the failed write must be reported: {result:?}"));
    assert!(
        !failure.unrecoverable,
        "it was restored, so nothing is beyond help: {failure:?}"
    );
}

#[test]
fn a_truncated_file_that_cannot_be_put_back_is_unrecoverable() {
    let mut files = FakeFiles::new(&[
        ("a.cs", "class Walker {}\n"),
        ("b.cs", "var w = new Walker();\n"),
    ]);
    files.fail_write = Some(2);
    files.fail_restore = true;
    files.truncating = true;
    let edit = edit(vec![
        document("a.cs", vec![insert(0, 6, "Heap")]),
        document("b.cs", vec![insert(0, 12, "Heap")]),
    ]);

    let result = apply_workspace_edit(&root(), &edit, &none(), "Walker", &mut files);

    let failure = result
        .failures
        .iter()
        .find(|failure| failure.path == relative("b.cs"))
        .unwrap_or_else(|| panic!("the failed write must be reported: {result:?}"));
    assert!(
        failure.unrecoverable,
        "this file really is left holding part of a rename: {failure:?}"
    );
    assert!(
        failure.detail.contains("git"),
        "the strongest wording in the feature sends the user to git: {}",
        failure.detail
    );
    assert!(
        message_of(&result).contains("could not be restored"),
        "and the summary must not say everything was restored: {result:?}"
    );
}

#[test]
fn a_failed_write_this_app_cannot_read_back_is_unrecoverable_rather_than_assumed_fine() {
    // Neither answer is established: the file may be untouched (the write failed
    // at open) or truncated (it failed after). Abstaining here means escalating,
    // because the cheap wrong answer — "nothing was changed" — is the one that
    // loses a file silently.
    let mut files = FakeFiles::new(&[("a.cs", "class Walker {}\n")]);
    files.fail_write = Some(1);
    files.unreadable_after_failure = true;
    let edit = edit(vec![document("a.cs", vec![insert(0, 6, "Heap")])]);

    let result = apply_workspace_edit(&root(), &edit, &none(), "Walker", &mut files);

    let failure = &result.failures[0];
    assert_eq!(failure.path, relative("a.cs"));
    assert!(
        failure.unrecoverable,
        "unread means unestablished, and unestablished is not `fine`: {failure:?}"
    );
}

// ---------------------------------------------------------------------------
// The real disk
// ---------------------------------------------------------------------------

#[test]
fn a_real_write_replaces_the_file_and_leaves_no_temporary_behind() {
    // `RealFiles::write` goes through a sibling temporary and a rename, so a
    // failure cannot leave a truncated file. The rename is the part worth
    // pinning: a temporary left in the workspace would show up in the Changes
    // tab and in the symbol index.
    let directory = tempfile::tempdir().expect("a temp dir");
    let root = directory.path().to_path_buf();
    std::fs::write(root.join("a.cs"), "class Walker {}\n").expect("the fixture");

    let mut files = RealFiles::new(root.clone());
    files
        .write(&root.join("a.cs"), "class HeapWalker {}\n")
        .expect("the write");

    assert_eq!(
        std::fs::read_to_string(root.join("a.cs")).expect("the file"),
        "class HeapWalker {}\n"
    );
    let leftovers: Vec<String> = std::fs::read_dir(&root)
        .expect("the directory")
        .filter_map(|entry| entry.ok())
        .map(|entry| entry.file_name().to_string_lossy().into_owned())
        .filter(|name| name != "a.cs")
        .collect();
    assert!(
        leftovers.is_empty(),
        "no temporary survives a write: {leftovers:?}"
    );
}

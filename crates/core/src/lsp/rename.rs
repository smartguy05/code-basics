//! Turning a server's `WorkspaceEdit` into files on disk, or refusing to.
//!
//! [`super::edits`] answers "what does this set of ranges do to one document?".
//! This answers the question above it: which files are these, may we touch them
//! at all, which of them is the editor already holding, and what happens when
//! the seventh write fails.
//!
//! # Two phases, and the split is the whole design
//!
//! **Phase 1 resolves and validates and writes nothing.** Every uri becomes a
//! workspace-relative path or the rename is refused whole; every file's edits go
//! through [`super::edits::plan`]; every closed file is read, size-checked and
//! token-checked. A refusal in phase 1 has touched no file at all, which is
//! where every foreseeable problem is arranged to land.
//!
//! **Phase 2 writes closed files only.** An open buffer is never written to
//! disk: for an open file the truth is the CodeMirror buffer plus the server's
//! own mirror, `FileEditor`'s build effect is keyed on identity alone with no
//! file watcher, and there is no autosave — so a disk write behind an open tab
//! is invisible to the buffer and is clobbered by the next `Ctrl+S`. Its edits
//! are **returned** instead, for the editor to dispatch as one transaction.
//!
//! # Why every refusal is whole
//!
//! Find-usages abstains per *row*: a location it cannot open is still shown and
//! still counted, because dropping it would make the count wrong. Rename cannot
//! do that. An edit that is not applied is a *partial rename*, and a half-done
//! rename does not compile — so a document this module cannot place, cannot
//! read, or cannot believe refuses the entire operation, including the files it
//! could have written.
//!
//! Zero documents is **not** a refusal. A server may answer that there is
//! nothing to change, which is a real answer: [`Availability::Ready`] with
//! `total: Some(0)`.
//!
//! # The stale-mirror check, as measured rather than as imagined
//!
//! The original rule was "the text each edit replaces contains the old
//! identifier". Measured against the real Roslyn server (2026-09-04), that rule
//! refuses **every** C# rename: Roslyn answers with a minimal diff, so renaming
//! `Walker` to `HeapWalker` emits a *zero-width insertion* of `"Heap"` and the
//! replaced text is the empty string every time.
//!
//! So the rule is instead [`enclosing_identifier`]: expand outward from each
//! edit's start over word characters and `_`, and require the resulting token to
//! equal the old name. That covers an insertion and a whole-identifier
//! replacement with one rule. It is **not** a per-edit refusal, because a
//! non-matching edit can be perfectly legitimate — Roslyn's rename-in-comments
//! and rename-in-strings options, and TypeScript's shorthand-property expansion
//! (`{ foo }` becoming `{ newFoo: foo }`), all touch text that is not the bare
//! identifier. A file is refused only when **no** edit in it matches; a partial
//! disagreement goes into the result's `message`.
//!
//! And note what this check is *not*: `documentChanges` entries arrive carrying
//! `"version": null`, so the document version cannot detect a stale mirror at
//! all. The real protection is the ordering guarantee — every `didChange` is
//! written to the server's stdin ahead of the request that follows it — and this
//! is the belt-and-braces beside it, not the other way round.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use super::edits;
use super::model::{
    Availability, BufferEdits, RangeEdit, RenameFailure, RenameResult, RenamedFile,
};
use super::positions::to_editor_line;
use super::protocol::{TextEdit, WorkspaceEdit};
use super::uri;
use crate::files::MAX_EDITABLE_BYTES;
use crate::symbols::index::relative_to_root;

/// How much pre-image text the rollback may hold at once, across every file.
///
/// The rollback works by keeping each closed file's previous contents in memory
/// until every write has succeeded, so the bound has to exist and it has to be
/// enforced in **phase 1**: refusing a 200-file rename before anything is
/// written is an answer, and running out of memory halfway through phase 2 is
/// the state this module exists to avoid.
pub const RENAME_MAX_TOTAL_BYTES: usize = 32 * 1024 * 1024;

/// Reading and writing workspace files, injected.
///
/// A trait for the same reason [`super::results::TextProvider`] and
/// [`super::registry::Probe`] are: the interesting cases cannot be arranged on a
/// developer's disk. It is also the **only** way the rollback path is reachable
/// in a test — a fake that fails on the *n*th write, and again on the restore
/// that follows — and that path is where the one unrecoverable state in the
/// feature lives.
///
/// Paths handed over are **absolute**, since that is the only thing that names a
/// file; [`RenamedFile::path`] and [`BufferEdits::path`] are the relative
/// spellings that cross IPC.
pub trait Files {
    /// The file's whole contents, or why not.
    fn read(&mut self, path: &Path) -> Result<String, String>;
    /// Replace the file's contents, or say why that did not happen.
    fn write(&mut self, path: &Path, text: &str) -> Result<(), String>;
}

/// The real disk, rooted at one workspace.
///
/// Carries the root so a write can go through [`crate::files::write_file`],
/// whose `resolve` refuses an absolute path, a drive prefix, `..` and the empty
/// path. Phase 1 has already established that every path is inside the root, so
/// this is defence in depth rather than the check itself — but a rename writes
/// files the user is not looking at, which is exactly where a second opinion is
/// worth having.
pub struct RealFiles {
    root: PathBuf,
}

impl RealFiles {
    pub fn new(root: PathBuf) -> Self {
        Self { root }
    }
}

impl Files for RealFiles {
    fn read(&mut self, path: &Path) -> Result<String, String> {
        std::fs::read_to_string(path)
            .map_err(|error| format!("{} could not be read: {error}", path.display()))
    }

    /// Write **atomically**: a sibling temporary file, then a rename over the
    /// target.
    ///
    /// [`crate::files::write_file`] is `std::fs::write`, which truncates the file
    /// at open and then writes — so an error part-way through leaves a short
    /// file whose remaining contents exist nowhere. That turns "the write
    /// returned an error" into a state the caller cannot describe, and a rename
    /// writes files the user is not looking at. `fs::rename` replaces the
    /// destination atomically on both Windows and POSIX (the same guarantee
    /// [`crate::notes::save`] relies on), so a failure here leaves the target
    /// exactly as it was. [`write_plan`] still verifies rather than trusting
    /// that, because a [`Files`] implementation is injected and this is only one
    /// of them.
    fn write(&mut self, path: &Path, text: &str) -> Result<(), String> {
        let relative = relative_to_root(&self.root, path).ok_or_else(|| {
            format!(
                "{} is not inside this workspace, so it was not written",
                path.display()
            )
        })?;
        let mut name = relative
            .file_name()
            .map(|name| name.to_os_string())
            .ok_or_else(|| format!("{} does not name a file", relative.display()))?;
        name.push(".cb-rename.tmp");
        let temporary = relative.with_file_name(name);
        crate::files::write_file(&self.root, &temporary, text)
            .map_err(|error| format!("{error:#}"))?;
        let from = self.root.join(&temporary);
        match std::fs::rename(&from, path) {
            Ok(()) => Ok(()),
            Err(error) => {
                // Best effort: a stray temporary file is untidy, a lost one is
                // nothing, and neither is worth masking the real error with.
                let _ = std::fs::remove_file(&from);
                Err(format!(
                    "could not replace {} ({error})",
                    relative.display()
                ))
            }
        }
    }
}

/// Apply a whole `WorkspaceEdit`, writing closed files and returning open ones.
///
/// `open` holds the **absolute** paths of the buffers the editor has open —
/// which is how [`super::session`] keys its document mirrors, so no conversion
/// can lose one. `expect` is the identifier being renamed, for the token check;
/// an empty `expect` abstains from that check rather than refusing every file,
/// because there is nothing to compare against.
///
/// Never panics and never returns a partially applied workspace without saying
/// so: either every closed file was written, or the ones that were are restored
/// and the failures are reported.
pub fn apply_workspace_edit(
    root: &Path,
    edit: &WorkspaceEdit,
    open: &BTreeSet<PathBuf>,
    expect: &str,
    files: &mut dyn Files,
) -> RenameResult {
    match plan_workspace_edit(root, edit, open, expect, files) {
        Ok(plan) => write_plan(plan, files),
        Err(refusal) => RenameResult::unavailable(Availability::Failed, refusal),
    }
}

/// The identifier token surrounding a position, or nothing.
///
/// Word characters and `_`, expanded in both directions from the position's byte
/// offset. `char::is_alphanumeric` rather than an ASCII test on purpose: an
/// identifier may be non-ASCII in every language this app talks to, and refusing
/// `café` would refuse a correct rename — which is the one thing this check must
/// not do.
///
/// Diagnostic, so it clamps rather than refusing: [`super::positions::byte_offset`]
/// resolves a position the document does not have to somewhere inside it, and a
/// caller asking "what token is here?" is owed `None` rather than a panic.
pub fn enclosing_identifier(text: &str, line: u32, character: u32) -> Option<&str> {
    let at = super::positions::byte_offset(text, line, character);
    let mut start = at;
    for (index, character) in text[..at].char_indices().rev() {
        if is_word(character) {
            start = index;
        } else {
            break;
        }
    }
    let mut end = at;
    for (index, character) in text[at..].char_indices() {
        if is_word(character) {
            end = at + index + character.len_utf8();
        } else {
            break;
        }
    }
    (start < end).then(|| &text[start..end])
}

/// Word characters, spelled to agree with `renameLogic.ts`'s `WORD`.
///
/// `$` is in the set because the frontend reads the old name out of the buffer
/// with `/[\p{L}\p{N}_$]/u`, and that string is the `expect` this side compares
/// against: a `$` excluded here could never equal a name containing one, so
/// every `$scope` or `users$` in a JS/TS workspace failed the token check on
/// every closed file and the entire rename was refused as a stale mirror. Two
/// spellings of one rule is the failure CLAUDE.md names; this is the same rule.
fn is_word(character: char) -> bool {
    character.is_alphanumeric() || character == '_' || character == '$'
}

// ---------------------------------------------------------------------------
// Phase 1
// ---------------------------------------------------------------------------

/// One file, resolved, planned, and — if it is closed — already applied in
/// memory with its pre-image kept for the rollback.
struct PlannedFile {
    absolute: PathBuf,
    /// Workspace-relative, forward-slashed as [`crate::symbols::index`] spells
    /// it. This is the spelling that crosses IPC.
    relative: PathBuf,
    planned: Vec<TextEdit>,
    /// `None` for an open buffer, which this side has no text for and must never
    /// write.
    applied: Option<Applied>,
}

/// A closed file's before and after.
struct Applied {
    before: String,
    after: String,
}

/// Everything phase 2 needs, and the qualification phase 1 collected.
struct Plan {
    files: Vec<PlannedFile>,
    total: u32,
    notes: Vec<String>,
}

fn plan_workspace_edit(
    root: &Path,
    edit: &WorkspaceEdit,
    open: &BTreeSet<PathBuf>,
    expect: &str,
    files: &mut dyn Files,
) -> Result<Plan, String> {
    if let Some(operation) = edit.resource_operations.first() {
        // Keyed on the list being non-empty and never on a match over known
        // kinds, so an operation kind added to the protocol after this was
        // written is still declined rather than silently applied as text.
        return Err(format!(
            "this rename would also {} {} on disk, which this app does not do — \
             moving a file invalidates open editor tabs, the symbol index and \
             every saved reference to it. Nothing was changed. ({} operation{} in \
             the answer.)",
            operation.kind,
            operation.uris.join(", "),
            edit.resource_operations.len(),
            if edit.resource_operations.len() == 1 {
                ""
            } else {
                "s"
            }
        ));
    }

    // Merged by *resolved path*, not by uri string. Roslyn spells a drive colon
    // plainly and rust-analyzer percent-encodes it, so one file can arrive as
    // two `documentChanges` entries — and applying them separately would compute
    // the second against text the first had already changed. Merging is what
    // lets `edits::plan` see a file's whole set and refuse an overlap it would
    // otherwise never be shown.
    let mut grouped: BTreeMap<PathBuf, Vec<TextEdit>> = BTreeMap::new();
    for document in &edit.documents {
        let Some(absolute) = uri::from_file_uri(&document.uri) else {
            return Err(format!(
                "the language server wants to edit `{}`, which is not a file this \
                 app can open. Renaming only some of the files would leave the code \
                 not compiling, so nothing was changed.",
                document.uri
            ));
        };
        if relative_to_root(root, &absolute).is_none() {
            return Err(format!(
                "the language server wants to edit {}, which is outside this \
                 workspace. Renaming only some of the files would leave the code \
                 not compiling, so nothing was changed.",
                absolute.display()
            ));
        }
        grouped
            .entry(absolute)
            .or_default()
            .extend(document.edits.iter().cloned());
    }

    let mut plan = Plan {
        files: Vec::new(),
        total: 0,
        notes: Vec::new(),
    };
    let mut held = 0usize;

    for (absolute, unplanned) in grouped {
        // Re-derived rather than carried from the check above so that the
        // relative spelling and the containment decision cannot drift apart.
        let relative = match relative_to_root(root, &absolute) {
            Some(relative) => relative,
            None => {
                return Err(format!(
                    "{} is outside this workspace. Nothing was changed.",
                    absolute.display()
                ))
            }
        };

        // A file the server named with nothing to change: legal, and neither
        // read nor written. Reading it would let an unreadable file refuse a
        // rename that was never going to touch it.
        if unplanned.is_empty() {
            continue;
        }

        let planned = edits::plan(&unplanned)
            .map_err(|error| format!("{}: {error}. Nothing was changed.", relative.display()))?;
        plan.total = plan.total.saturating_add(planned.len() as u32);

        if open.contains(&absolute) {
            // No read, no size check and no token check: this side has no text
            // for an open buffer, and the file on disk is not the truth. The
            // frontend checks the buffer it is about to edit.
            plan.files.push(PlannedFile {
                absolute,
                relative,
                planned,
                applied: None,
            });
            continue;
        }

        let before = files
            .read(&absolute)
            .map_err(|error| format!("{error}. Nothing was changed."))?;
        if before.len() as u64 > MAX_EDITABLE_BYTES {
            return Err(format!(
                "{} is too large to edit here ({} MB). Nothing was changed.",
                relative.display(),
                before.len() / (1024 * 1024)
            ));
        }
        held = held.saturating_add(before.len());
        if held > RENAME_MAX_TOTAL_BYTES {
            return Err(format!(
                "this rename touches too much text to undo safely ({} files, over \
                 {} MB). Nothing was changed.",
                plan.files.len() + 1,
                RENAME_MAX_TOTAL_BYTES / (1024 * 1024)
            ));
        }

        match token_verdict(&before, &planned, expect) {
            TokenVerdict::Agrees => {}
            TokenVerdict::Partly { matched, of } => plan.notes.push(format!(
                "In {}, only {matched} of {of} edits land on `{expect}` itself — the \
                 rest change text that merely contains it, such as a comment or a \
                 string.",
                relative.display()
            )),
            TokenVerdict::Disagrees => {
                return Err(format!(
                    "the language server's edits for {} do not land on `{expect}` at \
                     all, so it computed them from a different version of that file. \
                     Nothing was changed.",
                    relative.display()
                ))
            }
        }

        let after = edits::apply(&before, &planned)
            .map_err(|error| format!("{}: {error}. Nothing was changed.", relative.display()))?;
        plan.files.push(PlannedFile {
            absolute,
            relative,
            planned,
            applied: Some(Applied { before, after }),
        });
    }

    Ok(plan)
}

/// How much of a file's edit set lands on the identifier being renamed.
enum TokenVerdict {
    Agrees,
    /// Some do and some do not, which is ordinary — see the module docs.
    Partly {
        matched: usize,
        of: usize,
    },
    /// None do. The server is describing text this app does not have.
    Disagrees,
}

fn token_verdict(text: &str, planned: &[TextEdit], expect: &str) -> TokenVerdict {
    // Nothing to compare against. Comparing with `""`, which no token equals,
    // would refuse every rename — so this abstains instead.
    if expect.is_empty() || planned.is_empty() {
        return TokenVerdict::Agrees;
    }
    let matched = planned
        .iter()
        .filter(|edit| {
            enclosing_identifier(text, edit.range.start.line, edit.range.start.character)
                == Some(expect)
        })
        .count();
    if matched == planned.len() {
        TokenVerdict::Agrees
    } else if matched == 0 {
        TokenVerdict::Disagrees
    } else {
        TokenVerdict::Partly {
            matched,
            of: planned.len(),
        }
    }
}

// ---------------------------------------------------------------------------
// Phase 2
// ---------------------------------------------------------------------------

fn write_plan(plan: Plan, files: &mut dyn Files) -> RenameResult {
    let Plan {
        files: planned,
        total,
        notes,
    } = plan;

    let mut written: Vec<RenamedFile> = Vec::new();
    // Kept in write order so a rollback can restore exactly what it wrote.
    let mut done: Vec<(&PathBuf, &PathBuf, &str)> = Vec::new();

    for file in &planned {
        let Some(applied) = &file.applied else {
            continue;
        };
        match files.write(&file.absolute, &applied.after) {
            Ok(()) => {
                written.push(RenamedFile {
                    path: file.relative.clone(),
                    edits: file.planned.len() as u32,
                });
                done.push((&file.absolute, &file.relative, &applied.before));
            }
            Err(detail) => {
                let mut failures = vec![recover_failed_write(file, applied, detail, files)];
                failures.extend(roll_back(&done, files));
                let unrecoverable = failures.iter().filter(|f| f.unrecoverable).count();
                let message = if unrecoverable == 0 {
                    format!(
                        "the rename could not be completed: {} could not be written. \
                         Every file this app had already changed was restored, so \
                         nothing was renamed.",
                        file.relative.display()
                    )
                } else {
                    format!(
                        "the rename could not be completed, and {unrecoverable} file(s) \
                         could not be restored afterwards. Those files are left holding \
                         part of a rename and this app has no copy of their previous \
                         contents. Review them in git now, before making any other \
                         change."
                    )
                };
                return RenameResult {
                    outcome: Availability::Failed,
                    total: None,
                    // Restored, so nothing stands as written — and the editor must
                    // not be handed buffer edits that would apply half a rename.
                    written: Vec::new(),
                    buffers: Vec::new(),
                    failures,
                    message: Some(message),
                    server: None,
                };
            }
        }
    }

    let buffers = planned
        .iter()
        .filter(|file| file.applied.is_none())
        .map(|file| BufferEdits {
            path: file.relative.clone(),
            edits: file.planned.iter().map(range_edit).collect(),
        })
        .collect();

    RenameResult {
        outcome: Availability::Ready,
        total: Some(total),
        written,
        buffers,
        failures: Vec::new(),
        message: (!notes.is_empty()).then(|| notes.join(" ")),
        server: None,
    }
}

/// What actually happened to the file whose write failed.
///
/// A write returning an error does **not** establish that the file is unchanged:
/// a truncate-then-write leaves a short file when it fails after the truncate,
/// and no error this layer receives distinguishes the two. So this reads the
/// file back and finds out, instead of picking the comfortable reading.
///
/// Three answers, and they must stay apart:
///
/// * it still holds the pre-image — the ordinary case, the write failed at open
///   (permission denied, a lock, a read-only file), and "this file was not
///   changed" is now *established* rather than assumed;
/// * it holds something else — restore it from the pre-image, which is sitting
///   in [`PlannedFile::applied`] for exactly this and was previously never used;
/// * it cannot be read back, or the restore also failed — **unrecoverable**.
///   Unread is unestablished, and the cheap wrong answer here is the one that
///   loses a file silently.
fn recover_failed_write(
    file: &PlannedFile,
    applied: &Applied,
    detail: String,
    files: &mut dyn Files,
) -> RenameFailure {
    let unchanged = match files.read(&file.absolute) {
        Ok(text) => text == applied.before,
        Err(read_error) => {
            return RenameFailure {
                path: file.relative.clone(),
                detail: format!(
                    "this file could not be written ({detail}) and could not be read back afterwards ({read_error}), so this app cannot say whether it was left part-way through the write. Check it in git now, before doing anything else."
                ),
                unrecoverable: true,
            }
        }
    };
    if unchanged {
        return RenameFailure {
            path: file.relative.clone(),
            detail,
            unrecoverable: false,
        };
    }
    match files.write(&file.absolute, &applied.before) {
        Ok(()) => RenameFailure {
            path: file.relative.clone(),
            detail: format!(
                "this file could not be written ({detail}) and was left part-way through the write, so its previous contents were put back."
            ),
            unrecoverable: false,
        },
        Err(restore_error) => RenameFailure {
            path: file.relative.clone(),
            detail: format!(
                "this file could not be written ({detail}), was left part-way through the write, and could not be put back ({restore_error}). This app no longer has a copy of its previous contents — recover it from git before doing anything else."
            ),
            unrecoverable: true,
        },
    }
}

/// Put back everything already written, newest first.
///
/// Newest first so that a file written twice — which cannot happen today, since
/// the plan holds one entry per path, but would be the first thing a future
/// change breaks — ends up holding its oldest pre-image.
fn roll_back(done: &[(&PathBuf, &PathBuf, &str)], files: &mut dyn Files) -> Vec<RenameFailure> {
    let mut failures = Vec::new();
    for (absolute, relative, before) in done.iter().rev() {
        if let Err(detail) = files.write(absolute, before) {
            failures.push(RenameFailure {
                path: (*relative).clone(),
                detail: format!(
                    "this file was changed and could not be changed back ({detail}). It \
                     is left holding part of a rename, and this app no longer has a copy \
                     of its previous contents — recover it from git before doing anything \
                     else."
                ),
                unrecoverable: true,
            });
        }
    }
    failures
}

/// One planned edit in the IPC convention: 1-based line, 0-based UTF-16 column.
///
/// [`to_editor_line`] is the only converter, and it is applied to **both** ends —
/// this is the first type on the surface carrying an end position, and converting
/// only the start would move every multi-line edit's end one line up.
fn range_edit(edit: &TextEdit) -> RangeEdit {
    RangeEdit {
        start_line: to_editor_line(edit.range.start.line),
        start_character: edit.range.start.character,
        end_line: to_editor_line(edit.range.end.line),
        end_character: edit.range.end.character,
        new_text: edit.new_text.clone(),
    }
}

#[cfg(test)]
#[path = "rename_tests.rs"]
mod rename_tests;

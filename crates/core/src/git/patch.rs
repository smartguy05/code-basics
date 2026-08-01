//! Building unified diff patches restricted to a selection of lines.
//!
//! This is what makes "revert this one line" and "stage just this hunk" work.
//! Rather than rewriting file contents by hand — which goes wrong the moment
//! additions and deletions are interleaved — a patch containing only the
//! selected changes is generated and handed to `git apply`, which already
//! knows how to do this correctly.
//!
//! # Why direction matters
//!
//! A patch describes a transformation from an old state to a new one. Applying
//! it *forward* stages a change; applying it *reversed* undoes one. Crucially,
//! the lines that are **not** selected have to be treated differently in each
//! case, because what counts as "already present in the target" flips:
//!
//! | line | selected | forward | reversed |
//! |------|----------|---------|----------|
//! | context | –     | context | context  |
//! | addition | yes  | keep    | keep     |
//! | addition | no   | **drop** | **context** |
//! | deletion | yes  | keep    | keep     |
//! | deletion | no   | **context** | **drop** |
//!
//! Getting this backwards produces a patch that `git apply` rejects — or worse,
//! one that applies and silently reverts lines the user did not select.

use std::collections::BTreeSet;

use serde::{Deserialize, Serialize};
use specta::Type;

/// What a line represents in a diff.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub enum LineOrigin {
    Context,
    Addition,
    Deletion,
}

/// One line of a unified diff.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct DiffLine {
    /// Index within the whole file diff. Stable, and what the UI selects by.
    pub index: u32,
    pub origin: LineOrigin,
    /// Line content without its trailing newline.
    pub content: String,
    /// Line number in the baseline, for context and deletions.
    pub old_lineno: Option<u32>,
    /// Line number in the working copy, for context and additions.
    pub new_lineno: Option<u32>,
    /// True when the original file had no trailing newline here.
    pub no_newline: bool,
}

/// A contiguous run of changes.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct Hunk {
    pub old_start: u32,
    pub old_lines: u32,
    pub new_start: u32,
    pub new_lines: u32,
    /// The `@@ ... @@` trailer, usually the enclosing function.
    pub header: String,
    pub lines: Vec<DiffLine>,
}

/// The diff of a single file.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct FileDiff {
    /// Path in the working copy.
    pub path: String,
    /// Previous path, when the file was renamed.
    pub old_path: Option<String>,
    pub hunks: Vec<Hunk>,
    pub is_binary: bool,
}

impl FileDiff {
    /// Indices of every changed (non-context) line.
    pub fn changed_line_indices(&self) -> BTreeSet<u32> {
        self.hunks
            .iter()
            .flat_map(|h| &h.lines)
            .filter(|l| l.origin != LineOrigin::Context)
            .map(|l| l.index)
            .collect()
    }

    /// Indices of every changed line in one hunk.
    pub fn hunk_line_indices(&self, hunk: usize) -> BTreeSet<u32> {
        self.hunks
            .get(hunk)
            .map(|h| {
                h.lines
                    .iter()
                    .filter(|l| l.origin != LineOrigin::Context)
                    .map(|l| l.index)
                    .collect()
            })
            .unwrap_or_default()
    }
}

/// Which way the generated patch will be applied.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Direction {
    /// Applied as-is, moving the target towards the working copy. Used to
    /// stage changes.
    Forward,
    /// Applied with `--reverse`, undoing the change. Used to revert lines in
    /// the working copy and to unstage.
    Reverse,
}

/// Build a patch containing only `selected` lines.
///
/// Returns `None` when the selection contains nothing applicable, so callers
/// can skip running `git apply` entirely rather than feeding it an empty patch.
pub fn build_patch(file: &FileDiff, selected: &BTreeSet<u32>, direction: Direction) -> Option<String> {
    if file.is_binary {
        return None;
    }

    let mut hunks = String::new();

    for hunk in &file.hunks {
        if let Some(text) = build_hunk(hunk, selected, direction) {
            hunks.push_str(&text);
        }
    }

    if hunks.is_empty() {
        return None;
    }

    // `git apply` needs the a/ b/ prefixes it strips with -p1.
    let old_path = file.old_path.as_deref().unwrap_or(&file.path);
    let mut patch = format!("diff --git a/{old_path} b/{}\n", file.path);
    patch.push_str(&format!("--- a/{old_path}\n"));
    patch.push_str(&format!("+++ b/{}\n", file.path));
    patch.push_str(&hunks);

    Some(patch)
}

/// Render one hunk with unselected changes neutralised.
///
/// Returns `None` when the hunk ends up containing no changes at all — a hunk
/// of pure context is not just useless, it makes `git apply` fail.
fn build_hunk(hunk: &Hunk, selected: &BTreeSet<u32>, direction: Direction) -> Option<String> {
    let mut body = String::new();
    let mut old_count = 0u32;
    let mut new_count = 0u32;
    let mut has_change = false;

    for line in &hunk.lines {
        let is_selected = selected.contains(&line.index);

        // How this line should appear in the generated patch. See the table
        // in the module documentation for why direction changes the answer.
        let rendered = match (line.origin, is_selected, direction) {
            (LineOrigin::Context, _, _) => Some(' '),
            (LineOrigin::Addition, true, _) => Some('+'),
            (LineOrigin::Deletion, true, _) => Some('-'),

            // Unselected addition: forward, it must not be introduced at all;
            // reversed, the source already contains it, so it is context.
            (LineOrigin::Addition, false, Direction::Forward) => None,
            (LineOrigin::Addition, false, Direction::Reverse) => Some(' '),

            // Unselected deletion: the mirror image.
            (LineOrigin::Deletion, false, Direction::Forward) => Some(' '),
            (LineOrigin::Deletion, false, Direction::Reverse) => None,
        };

        let Some(marker) = rendered else { continue };

        match marker {
            ' ' => {
                old_count += 1;
                new_count += 1;
            }
            '+' => {
                new_count += 1;
                has_change = true;
            }
            '-' => {
                old_count += 1;
                has_change = true;
            }
            _ => unreachable!(),
        }

        body.push(marker);
        body.push_str(&line.content);
        body.push('\n');

        if line.no_newline {
            body.push_str("\\ No newline at end of file\n");
        }
    }

    if !has_change {
        return None;
    }

    // Counts describe the *filtered* hunk, not the original, or the patch will
    // not line up with the file it is applied to.
    let old_start = if old_count == 0 { hunk.old_start.saturating_sub(1) } else { hunk.old_start };
    let new_start = if new_count == 0 { hunk.new_start.saturating_sub(1) } else { hunk.new_start };

    let header = if hunk.header.is_empty() {
        String::new()
    } else {
        format!(" {}", hunk.header.trim_end())
    };

    Some(format!(
        "@@ -{old_start},{old_count} +{new_start},{new_count} @@{header}\n{body}"
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn line(index: u32, origin: LineOrigin, content: &str) -> DiffLine {
        DiffLine {
            index,
            origin,
            content: content.to_string(),
            old_lineno: matches!(origin, LineOrigin::Context | LineOrigin::Deletion).then_some(index + 1),
            new_lineno: matches!(origin, LineOrigin::Context | LineOrigin::Addition).then_some(index + 1),
            no_newline: false,
        }
    }

    /// A file where line 2 is replaced: `-old` / `+new`, with context either side.
    fn replacement_diff() -> FileDiff {
        FileDiff {
            path: "src/main.rs".into(),
            old_path: None,
            is_binary: false,
            hunks: vec![Hunk {
                old_start: 1,
                old_lines: 3,
                new_start: 1,
                new_lines: 3,
                header: "fn main()".into(),
                lines: vec![
                    line(0, LineOrigin::Context, "first"),
                    line(1, LineOrigin::Deletion, "old"),
                    line(2, LineOrigin::Addition, "new"),
                    line(3, LineOrigin::Context, "last"),
                ],
            }],
        }
    }

    /// Two independent additions in one hunk, so a partial selection is
    /// meaningful.
    fn two_additions() -> FileDiff {
        FileDiff {
            path: "notes.txt".into(),
            old_path: None,
            is_binary: false,
            hunks: vec![Hunk {
                old_start: 1,
                old_lines: 1,
                new_start: 1,
                new_lines: 3,
                header: String::new(),
                lines: vec![
                    line(0, LineOrigin::Context, "keep"),
                    line(1, LineOrigin::Addition, "alpha"),
                    line(2, LineOrigin::Addition, "beta"),
                ],
            }],
        }
    }

    #[test]
    fn a_full_selection_reproduces_the_whole_change() {
        let file = replacement_diff();
        let patch = build_patch(&file, &file.changed_line_indices(), Direction::Forward).unwrap();

        assert!(patch.contains("--- a/src/main.rs"));
        assert!(patch.contains("+++ b/src/main.rs"));
        assert!(patch.contains("-old"));
        assert!(patch.contains("+new"));
        assert!(patch.contains("@@ -1,3 +1,3 @@ fn main()"));
    }

    #[test]
    fn forward_drops_unselected_additions() {
        // Staging only "alpha" must not also introduce "beta".
        let file = two_additions();
        let selected = BTreeSet::from([1]);
        let patch = build_patch(&file, &selected, Direction::Forward).unwrap();

        assert!(patch.contains("+alpha"));
        assert!(!patch.contains("beta"), "unselected addition leaked into the patch");
        assert!(patch.contains("@@ -1,1 +1,2 @@"));
    }

    #[test]
    fn reverse_turns_unselected_additions_into_context() {
        // Reverting only "alpha" applies to a file that already contains
        // "beta", so "beta" has to be present as context for the patch to
        // line up.
        let file = two_additions();
        let selected = BTreeSet::from([1]);
        let patch = build_patch(&file, &selected, Direction::Reverse).unwrap();

        assert!(patch.contains("+alpha"));
        assert!(patch.contains(" beta"), "unselected addition must become context");
        assert!(!patch.contains("+beta"));
        // Two context lines plus the addition being removed.
        assert!(patch.contains("@@ -1,2 +1,3 @@"));
    }

    #[test]
    fn forward_turns_unselected_deletions_into_context() {
        let file = FileDiff {
            path: "f.txt".into(),
            old_path: None,
            is_binary: false,
            hunks: vec![Hunk {
                old_start: 1,
                old_lines: 3,
                new_start: 1,
                new_lines: 1,
                header: String::new(),
                lines: vec![
                    line(0, LineOrigin::Context, "keep"),
                    line(1, LineOrigin::Deletion, "gone"),
                    line(2, LineOrigin::Deletion, "staying"),
                ],
            }],
        };
        let patch = build_patch(&file, &BTreeSet::from([1]), Direction::Forward).unwrap();

        assert!(patch.contains("-gone"));
        assert!(patch.contains(" staying"), "unselected deletion must become context");
        assert!(!patch.contains("-staying"));
    }

    #[test]
    fn reverse_drops_unselected_deletions() {
        let file = FileDiff {
            path: "f.txt".into(),
            old_path: None,
            is_binary: false,
            hunks: vec![Hunk {
                old_start: 1,
                old_lines: 3,
                new_start: 1,
                new_lines: 1,
                header: String::new(),
                lines: vec![
                    line(0, LineOrigin::Context, "keep"),
                    line(1, LineOrigin::Deletion, "gone"),
                    line(2, LineOrigin::Deletion, "staying"),
                ],
            }],
        };
        let patch = build_patch(&file, &BTreeSet::from([1]), Direction::Reverse).unwrap();

        assert!(patch.contains("-gone"));
        assert!(!patch.contains("staying"), "unselected deletion leaked into a reverse patch");
    }

    #[test]
    fn counts_describe_the_filtered_hunk_not_the_original() {
        let file = two_additions();
        let patch = build_patch(&file, &BTreeSet::from([1]), Direction::Forward).unwrap();
        let header = patch.lines().find(|l| l.starts_with("@@")).unwrap();

        // One context line in, one context plus one addition out.
        assert_eq!(header, "@@ -1,1 +1,2 @@");
    }

    #[test]
    fn a_selection_containing_no_changes_produces_no_patch() {
        let file = replacement_diff();
        assert!(build_patch(&file, &BTreeSet::new(), Direction::Forward).is_none());
        // Selecting only a context line is likewise not a change.
        assert!(build_patch(&file, &BTreeSet::from([0]), Direction::Forward).is_none());
    }

    #[test]
    fn hunks_with_nothing_selected_are_omitted_entirely() {
        let mut file = two_additions();
        file.hunks.push(Hunk {
            old_start: 20,
            old_lines: 1,
            new_start: 20,
            new_lines: 2,
            header: String::new(),
            lines: vec![
                line(3, LineOrigin::Context, "other"),
                line(4, LineOrigin::Addition, "unrelated"),
            ],
        });

        let patch = build_patch(&file, &BTreeSet::from([1]), Direction::Forward).unwrap();

        assert_eq!(patch.matches("@@").count(), 2, "one hunk header has two @@ markers");
        assert!(!patch.contains("unrelated"));
    }

    #[test]
    fn binary_files_produce_no_patch() {
        let mut file = replacement_diff();
        file.is_binary = true;
        assert!(build_patch(&file, &file.changed_line_indices(), Direction::Forward).is_none());
    }

    #[test]
    fn a_rename_names_both_paths() {
        let mut file = replacement_diff();
        file.old_path = Some("src/old_name.rs".into());
        let patch = build_patch(&file, &file.changed_line_indices(), Direction::Forward).unwrap();

        assert!(patch.contains("--- a/src/old_name.rs"));
        assert!(patch.contains("+++ b/src/main.rs"));
    }

    #[test]
    fn a_missing_trailing_newline_is_marked() {
        let mut file = replacement_diff();
        file.hunks[0].lines[2].no_newline = true;
        let patch = build_patch(&file, &file.changed_line_indices(), Direction::Forward).unwrap();

        assert!(patch.contains("\\ No newline at end of file"));
    }

    #[test]
    fn hunk_line_indices_exclude_context() {
        let file = replacement_diff();
        assert_eq!(file.hunk_line_indices(0), BTreeSet::from([1, 2]));
        assert_eq!(file.hunk_line_indices(99), BTreeSet::new());
    }

    #[test]
    fn selecting_every_line_of_a_hunk_matches_selecting_the_hunk() {
        let file = replacement_diff();
        let by_hunk = build_patch(&file, &file.hunk_line_indices(0), Direction::Forward);
        let by_all = build_patch(&file, &file.changed_line_indices(), Direction::Forward);
        assert_eq!(by_hunk, by_all);
    }
}

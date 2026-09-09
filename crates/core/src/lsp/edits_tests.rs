use super::*;

/// A `TextEdit` from four numbers, so a test reads as a table rather than as
/// four lines of struct literal per edit.
fn edit(
    start_line: u32,
    start_character: u32,
    end_line: u32,
    end_character: u32,
    new_text: &str,
) -> TextEdit {
    TextEdit {
        range: Range {
            start: Position {
                line: start_line,
                character: start_character,
            },
            end: Position {
                line: end_line,
                character: end_character,
            },
        },
        new_text: new_text.to_string(),
    }
}

/// One edit inside a single line.
fn on(line: u32, from: u32, to: u32, new_text: &str) -> TextEdit {
    edit(line, from, line, to, new_text)
}

/// A multi-line document whose lines differ in length.
///
/// Deliberately not one line: on a single-line document "sorted", "in bounds"
/// and "non-overlapping" are all true by accident, and a test built on one
/// proves nothing about the arithmetic. The same trap as
/// `.memories/bugs/didchange-range-from-wrong-text`.
const DOC: &str = "let alpha = 1;\nlet beta = alpha + 2;\nreturn beta;\n";

#[test]
fn no_edits_leaves_the_document_alone() {
    let planned = plan(&[]).expect("an empty set is applicable");
    assert!(planned.is_empty());
    assert_eq!(DOC, apply(DOC, &planned).expect("nothing to do"));
}

#[test]
fn a_single_edit_replaces_exactly_its_range() {
    let planned = plan(&[on(0, 4, 9, "gamma")]).expect("applicable");
    assert_eq!(
        "let gamma = 1;\nlet beta = alpha + 2;\nreturn beta;\n",
        apply(DOC, &planned).expect("applies")
    );
}

#[test]
fn edits_arrive_in_any_order_and_apply_in_document_order() {
    // The rename case: a server has no obligation to sort, and applying in
    // arrival order would compute every later offset against the wrong text.
    let scrambled = [
        on(2, 7, 11, "gamma"),
        on(0, 4, 9, "gamma"),
        on(1, 11, 16, "gamma"),
    ];
    let forward = plan(&scrambled).expect("applicable");
    let reversed = {
        let mut input = scrambled.clone();
        input.reverse();
        plan(&input).expect("applicable")
    };
    assert_eq!(forward, reversed, "the plan must not depend on input order");
    assert_eq!(
        "let gamma = 1;\nlet beta = gamma + 2;\nreturn gamma;\n",
        apply(DOC, &forward).expect("applies")
    );
}

#[test]
fn a_replacement_shorter_than_its_range_shifts_every_later_edit_left() {
    // Together with the test below this is what pins the cursor arithmetic: a
    // set of same-length replacements would pass with the offsets computed
    // wrongly in either direction.
    let planned = plan(&[on(0, 4, 9, "a"), on(1, 11, 16, "a")]).expect("applicable");
    assert_eq!(
        "let a = 1;\nlet beta = a + 2;\nreturn beta;\n",
        apply(DOC, &planned).expect("applies")
    );
}

#[test]
fn a_replacement_longer_than_its_range_shifts_every_later_edit_right() {
    let planned =
        plan(&[on(0, 4, 9, "alphabetical"), on(1, 11, 16, "alphabetical")]).expect("applicable");
    assert_eq!(
        "let alphabetical = 1;\nlet beta = alphabetical + 2;\nreturn beta;\n",
        apply(DOC, &planned).expect("applies")
    );
}

#[test]
fn overlapping_edits_are_refused_rather_than_applied_in_a_guessed_order() {
    // Either order produces text the server did not ask for, so there is no
    // answer to pick — and picking one silently corrupts a file.
    let error = plan(&[on(0, 4, 9, "gamma"), on(0, 6, 12, "delta")])
        .expect_err("an overlap has no defined result");
    assert!(
        matches!(error, EditError::Overlapping { .. }),
        "expected an overlap, got {error:?}"
    );
}

#[test]
fn an_overlap_across_a_line_boundary_is_still_an_overlap() {
    let error = plan(&[edit(0, 4, 1, 4, "x"), on(1, 0, 3, "y")])
        .expect_err("a multi-line span can overlap the next line's edit");
    assert!(matches!(error, EditError::Overlapping { .. }), "{error:?}");
}

#[test]
fn two_edits_that_meet_at_a_boundary_are_not_an_overlap() {
    // `end` is exclusive, so `a.end == b.start` is adjacency. Treating it as an
    // overlap would refuse a great many perfectly ordinary rename answers.
    let planned =
        plan(&[on(0, 0, 3, "var"), on(0, 3, 4, "\t")]).expect("adjacent, not overlapping");
    assert_eq!(
        "var\talpha = 1;\nlet beta = alpha + 2;\nreturn beta;\n",
        apply(DOC, &planned).expect("applies")
    );
}

#[test]
fn an_identical_edit_sent_twice_is_applied_once() {
    // Same range *and* same text, so deduplicating changes nothing about the
    // result — whereas applying it twice would insert the replacement twice.
    let planned = plan(&[on(0, 4, 9, "gamma"), on(0, 4, 9, "gamma")]).expect("applicable");
    assert_eq!(1, planned.len());
    assert_eq!(
        "let gamma = 1;\nlet beta = alpha + 2;\nreturn beta;\n",
        apply(DOC, &planned).expect("applies")
    );
}

#[test]
fn the_same_range_with_different_text_is_an_overlap_and_not_a_duplicate() {
    // Two servers' worth of disagreement about one span. Silently keeping one is
    // the guess this module refuses.
    let error = plan(&[on(0, 4, 9, "gamma"), on(0, 4, 9, "delta")])
        .expect_err("one span cannot become two different things");
    assert!(matches!(error, EditError::Overlapping { .. }), "{error:?}");
}

#[test]
fn two_insertions_at_the_same_point_are_refused() {
    // Both are zero-width at one position, so neither "before" nor "after" is
    // derivable from the answer.
    let error = plan(&[on(0, 4, 4, "a"), on(0, 4, 4, "b")])
        .expect_err("two insertions at one point have no order");
    assert!(
        matches!(error, EditError::AmbiguousInsertion { .. }),
        "{error:?}"
    );
}

#[test]
fn one_insertion_at_the_start_of_a_replaced_range_is_not_ambiguous() {
    // A zero-width edit abutting a real one has a defined order: the insertion
    // goes in, then the replacement follows.
    let planned = plan(&[on(0, 4, 4, "the_"), on(0, 4, 9, "gamma")]).expect("applicable");
    assert_eq!(
        "let the_gamma = 1;\nlet beta = alpha + 2;\nreturn beta;\n",
        apply(DOC, &planned).expect("applies")
    );
}

#[test]
fn an_identical_insertion_sent_twice_is_applied_once() {
    let planned = plan(&[on(0, 4, 4, "a"), on(0, 4, 4, "a")]).expect("applicable");
    assert_eq!(1, planned.len());
}

#[test]
fn a_backwards_range_is_refused() {
    let error = plan(&[on(0, 9, 4, "gamma")]).expect_err("end before start is not a span");
    assert!(matches!(error, EditError::Backwards { .. }), "{error:?}");
}

#[test]
fn a_range_whose_end_line_precedes_its_start_line_is_refused() {
    let error = plan(&[edit(2, 0, 1, 0, "x")]).expect_err("backwards across lines");
    assert!(matches!(error, EditError::Backwards { .. }), "{error:?}");
}

#[test]
fn an_edit_beyond_the_end_of_the_document_is_refused_because_the_server_has_a_different_file() {
    // The stale-mirror case, and the one place this module deliberately does not
    // clamp. `positions::byte_offset` would happily resolve line 40 to the end
    // of the document, which would apply the edit somewhere plausible and wrong.
    let planned = plan(&[on(40, 0, 4, "x")]).expect("positionally well-formed");
    let error = apply(DOC, &planned).expect_err("the document has three lines");
    match error {
        EditError::OutOfDocument { line, lines } => {
            assert_eq!(40, line);
            assert_eq!(4, lines, "three lines of text plus the empty final line");
        }
        other => panic!("expected OutOfDocument, got {other:?}"),
    }
}

#[test]
fn an_edit_on_the_last_addressable_line_is_not_out_of_document() {
    // `DOC` ends in a newline, so line 3 exists and is empty. Refusing it would
    // refuse every append.
    let planned = plan(&[on(3, 0, 0, "// end\n")]).expect("applicable");
    assert_eq!(
        "let alpha = 1;\nlet beta = alpha + 2;\nreturn beta;\n// end\n",
        apply(DOC, &planned).expect("applies")
    );
}

#[test]
fn an_insertion_at_the_end_of_the_document_appends() {
    let doc = "abc";
    let planned = plan(&[on(0, 3, 3, "def")]).expect("applicable");
    assert_eq!("abcdef", apply(doc, &planned).expect("applies"));
}

#[test]
fn an_insertion_into_the_empty_document_is_the_whole_document() {
    let planned = plan(&[on(0, 0, 0, "hello")]).expect("applicable");
    assert_eq!("hello", apply("", &planned).expect("applies"));
}

#[test]
fn an_empty_replacement_deletes_the_range() {
    let planned = plan(&[on(0, 3, 9, "")]).expect("applicable");
    assert_eq!(
        "let = 1;\nlet beta = alpha + 2;\nreturn beta;\n",
        apply(DOC, &planned).expect("applies")
    );
}

#[test]
fn a_replacement_may_itself_be_several_lines() {
    let planned = plan(&[on(0, 4, 9, "first\nsecond")]).expect("applicable");
    assert_eq!(
        "let first\nsecond = 1;\nlet beta = alpha + 2;\nreturn beta;\n",
        apply(DOC, &planned).expect("applies")
    );
}

#[test]
fn an_edit_spanning_several_lines_replaces_the_terminators_too() {
    let planned = plan(&[edit(0, 0, 2, 0, "")]).expect("applicable");
    assert_eq!("return beta;\n", apply(DOC, &planned).expect("applies"));
}

#[test]
fn crlf_line_endings_survive_a_multi_line_edit() {
    // Every terminator the edits do not span must come out the other side
    // unchanged, or a rename quietly rewrites the line endings of a whole file.
    let doc = "let alpha = 1;\r\nlet beta = alpha;\r\nreturn beta;\r\n";
    let planned = plan(&[on(0, 4, 9, "gamma"), on(1, 11, 16, "gamma")]).expect("applicable");
    assert_eq!(
        "let gamma = 1;\r\nlet beta = gamma;\r\nreturn beta;\r\n",
        apply(doc, &planned).expect("applies")
    );
}

#[test]
fn an_overrunning_column_on_a_crlf_line_does_not_split_the_terminator() {
    // A range ending "past" the line is a common spelling for end-of-line. If it
    // resolved to after the `\r`, the replacement would land between the
    // carriage return and the newline and leave a lone `\r` behind.
    let doc = "let alpha = 1;\r\nreturn 2;\r\n";
    let planned = plan(&[on(0, 0, 900, "")]).expect("applicable");
    assert_eq!("\r\nreturn 2;\r\n", apply(doc, &planned).expect("applies"));
}

#[test]
fn a_column_is_utf16_code_units_and_not_bytes_or_chars() {
    // `é` is 2 bytes and 1 unit; the emoji is 4 bytes and *2* units. A
    // byte-based applier cuts the emoji in half and panics.
    let doc = "let é\u{1F600} = 1;\n";
    let planned = plan(&[on(0, 4, 7, "x")]).expect("applicable");
    assert_eq!("let x = 1;\n", apply(doc, &planned).expect("applies"));
}

#[test]
fn replaced_texts_report_what_each_edit_is_about_to_overwrite() {
    // The stale-mirror check's evidence: if these are not the old identifier,
    // the server computed its ranges from text we do not have.
    let planned = plan(&[on(0, 4, 9, "gamma"), on(1, 11, 16, "gamma")]).expect("applicable");
    assert_eq!(vec!["alpha", "alpha"], replaced_texts(DOC, &planned));
}

#[test]
fn replaced_texts_are_empty_for_insertions() {
    let planned = plan(&[on(0, 4, 4, "the_")]).expect("applicable");
    assert_eq!(vec![""], replaced_texts(DOC, &planned));
}

#[test]
fn replaced_texts_do_not_panic_on_a_range_past_the_end() {
    // `replaced_texts` is diagnostic, so unlike `apply` it reports what it can
    // rather than refusing — but it must never panic doing so.
    let planned = plan(&[on(40, 0, 4, "x")]).expect("positionally well-formed");
    assert_eq!(1, replaced_texts(DOC, &planned).len());
}

#[test]
fn no_set_of_edits_can_panic_the_applier() {
    let documents = ["", "\n", "\r\n", "a", "é€\u{1F600}", DOC, "a\nb\r\nc\rd\n"];
    let candidates = [
        on(0, 0, 0, ""),
        on(0, 0, 0, "x"),
        on(0, 0, u32::MAX, "x"),
        on(0, u32::MAX, u32::MAX, "x"),
        edit(0, 0, u32::MAX, u32::MAX, "x"),
        on(u32::MAX, 0, 1, "x"),
        on(2, 1, 3, "\u{1F600}"),
    ];
    for text in documents {
        for candidate in &candidates {
            // Whatever `plan` accepts, `apply` must answer rather than panic.
            if let Ok(planned) = plan(std::slice::from_ref(candidate)) {
                let _ = apply(text, &planned);
                let _ = replaced_texts(text, &planned);
            }
        }
    }
}

#[test]
fn a_refusal_names_both_of_the_ranges_it_is_about() {
    // The message reaches the user, and "some edits overlapped" is not something
    // anyone can act on.
    let error = plan(&[on(1, 0, 5, "a"), on(1, 3, 8, "b")]).expect_err("overlap");
    let sentence = error.to_string();
    // The line is rendered 1-based, because the reader of this message is
    // looking at an editor gutter and not at the protocol.
    assert!(sentence.contains("line 2"), "{sentence}");
    assert!(sentence.contains("0-5"), "{sentence}");
    assert!(sentence.contains("3-8"), "{sentence}");
}

use super::*;

#[test]
fn an_ascii_line_maps_one_utf16_unit_to_one_byte() {
    assert_eq!(0, utf16_to_byte("hello", 0));
    assert_eq!(3, utf16_to_byte("hello", 3));
    assert_eq!(5, utf16_to_byte("hello", 5));
}

#[test]
fn a_two_byte_character_is_one_utf16_unit() {
    // `é` is 2 bytes in UTF-8 and 1 unit in UTF-16. Confusing the two shifts
    // every column after the first accented character on the line.
    let line = "aébc";
    assert_eq!(1, utf16_to_byte(line, 1));
    assert_eq!(3, utf16_to_byte(line, 2), "past the é: 1 + 2 bytes");
    assert_eq!(4, utf16_to_byte(line, 3));
}

#[test]
fn a_three_byte_character_is_still_one_utf16_unit() {
    let line = "a€b";
    assert_eq!(1, utf16_to_byte(line, 1));
    assert_eq!(4, utf16_to_byte(line, 2), "past the €: 1 + 3 bytes");
}

#[test]
fn an_astral_character_is_two_utf16_units_and_four_bytes() {
    // An emoji is a surrogate pair in UTF-16. This is the case that makes a
    // naive `chars().take(n)` wrong, and the wrongness only shows up in files
    // that happen to contain one — so it must be pinned.
    let line = "a😀b";
    assert_eq!(1, utf16_to_byte(line, 1));
    assert_eq!(
        1,
        utf16_to_byte(line, 2),
        "halfway into a surrogate pair is not a byte boundary; clamp back to the character start"
    );
    assert_eq!(5, utf16_to_byte(line, 3), "past the emoji: 1 + 4 bytes");
    assert_eq!(6, utf16_to_byte(line, 4));
}

#[test]
fn a_character_past_the_end_of_the_line_clamps_instead_of_panicking() {
    // Servers do send these — a range whose end is the start of the next line
    // is a common spelling for "to end of line". Slicing with it would panic
    // inside a command, which the user sees as the app breaking. Same lesson as
    // `lineToPos` on the frontend.
    assert_eq!(5, utf16_to_byte("hello", 5));
    assert_eq!(5, utf16_to_byte("hello", 6));
    assert_eq!(5, utf16_to_byte("hello", u32::MAX));
    assert_eq!(0, utf16_to_byte("", 0));
    assert_eq!(0, utf16_to_byte("", 99));
}

#[test]
fn a_byte_offset_converts_back_to_a_utf16_unit() {
    assert_eq!(0, byte_to_utf16("aébc", 0));
    assert_eq!(1, byte_to_utf16("aébc", 1));
    assert_eq!(2, byte_to_utf16("aébc", 3));
    assert_eq!(
        3,
        byte_to_utf16("a😀b", 5),
        "the emoji counted as two units"
    );
}

#[test]
fn the_two_conversions_agree_on_every_boundary_of_a_mixed_line() {
    // The property that matters: they are inverses at character boundaries. A
    // fixture with only one value per field cannot show a divergence, so this
    // walks every boundary of a line containing all four UTF-8 widths.
    let line = "a é € 😀 z";
    let mut byte = 0usize;
    for character in line.chars() {
        let units = byte_to_utf16(line, byte);
        assert_eq!(
            byte,
            utf16_to_byte(line, units),
            "round trip at byte {byte}"
        );
        byte += character.len_utf8();
    }
    assert_eq!(
        line.len(),
        utf16_to_byte(line, byte_to_utf16(line, line.len()))
    );
}

#[test]
fn a_byte_offset_inside_a_character_clamps_to_its_start() {
    // Nothing should produce one, but a clamp is cheaper than a panic reaching
    // the UI, and `is_char_boundary` is the only honest test available.
    assert_eq!(1, byte_to_utf16("aé", 2), "byte 2 is inside the é");
}

#[test]
fn a_tab_is_one_unit_however_wide_it_is_drawn() {
    // Tab *display* width is an editor setting; a tab is one code unit. A
    // converter that expanded tabs would put the cursor in the wrong column in
    // every tab-indented file.
    assert_eq!(2, utf16_to_byte("\t\tif", 2));
    assert_eq!(2, byte_to_utf16("\t\tif", 2));
}

#[test]
fn a_zero_based_lsp_line_becomes_a_one_based_editor_line() {
    // The whole app counts lines the way an editor gutter does; LSP counts from
    // zero. One conversion, in one place, pinned — an off-by-one here is a jump
    // to the wrong line, which is the failure this feature exists to avoid.
    assert_eq!(1, to_editor_line(0));
    assert_eq!(138, to_editor_line(137));
}

#[test]
fn a_one_based_editor_line_becomes_a_zero_based_lsp_line() {
    assert_eq!(0, to_lsp_line(1));
    assert_eq!(137, to_lsp_line(138));
}

#[test]
fn line_zero_cannot_be_asked_for_and_maps_to_the_first_line() {
    // A 1-based line of 0 is not a line. Subtracting would wrap to u32::MAX and
    // ask the server about a line four billion down.
    assert_eq!(0, to_lsp_line(0));
}

#[test]
fn the_line_conversions_are_inverses() {
    for line in [0u32, 1, 2, 41, 1000, u32::MAX - 1] {
        assert_eq!(line, to_lsp_line(to_editor_line(line)));
    }
}

#[test]
fn a_snippet_is_the_line_with_its_indentation_removed() {
    // The list is narrow and every C# line starts eight columns in; keeping the
    // indentation would show a column of blanks and hide the code.
    let snippet = snippet("        return TryGetElements(x);", Some((15, 29)));
    assert_eq!("return TryGetElements(x);", snippet.text);
}

#[test]
fn the_highlight_moves_with_the_text_it_marks() {
    // Trimming the indentation without shifting the highlight underlines the
    // wrong word, which is worse than no highlight at all.
    let line = "    foo(bar);";
    let snippet = snippet(line, Some((4, 7)));
    assert_eq!("foo(bar);", snippet.text);
    let (start, end) = snippet.highlight.expect("a highlight");
    assert_eq!("foo", &snippet.text[start as usize..end as usize]);
}

#[test]
fn a_snippet_with_no_range_carries_no_highlight() {
    let snippet = snippet("  let x = 1;", None);
    assert_eq!("let x = 1;", snippet.text);
    assert_eq!(None, snippet.highlight);
}

#[test]
fn a_range_that_the_trim_would_cut_into_drops_the_highlight_rather_than_moving_it() {
    // A range starting inside the indentation is not a range over a word. No
    // highlight is honest; a shifted one is a claim about the wrong characters.
    let snippet = snippet("    foo();", Some((1, 6)));
    assert_eq!("foo();", snippet.text);
    assert_eq!(None, snippet.highlight);
}

#[test]
fn a_range_reaching_past_the_end_of_the_line_is_clamped_not_dropped() {
    let snippet = snippet("foo();", Some((0, 999)));
    let (start, end) = snippet.highlight.expect("a clamped highlight");
    assert_eq!(0, start);
    assert_eq!(snippet.text.len() as u32, end);
}

#[test]
fn a_backwards_range_yields_no_highlight() {
    assert_eq!(None, snippet("foo();", Some((4, 1))).highlight);
}

#[test]
fn an_empty_range_yields_no_highlight() {
    // Zero-width is a cursor position, not a span to underline.
    assert_eq!(None, snippet("foo();", Some((2, 2))).highlight);
}

#[test]
fn a_very_long_line_is_cut_and_says_so() {
    let line = format!("{}needle{}", "x".repeat(400), "y".repeat(400));
    let snippet = snippet(&line, Some((400, 406)));
    assert!(
        snippet.text.chars().count() <= SNIPPET_MAX_CHARS,
        "a minified bundle on one line must not be shipped whole: {} chars",
        snippet.text.chars().count()
    );
    assert!(snippet.truncated, "the cut has to be visible to the reader");
}

#[test]
fn a_cut_keeps_the_match_inside_the_window() {
    // Cutting from the left would show 200 characters of a minified bundle and
    // none of the thing the row is about.
    let line = format!("{}needle{}", "x".repeat(400), "y".repeat(400));
    let snippet = snippet(&line, Some((400, 406)));
    assert!(
        snippet.text.contains("needle"),
        "the match must survive the cut: {}",
        snippet.text
    );
    let (start, end) = snippet.highlight.expect("a highlight");
    assert_eq!("needle", &snippet.text[start as usize..end as usize]);
}

#[test]
fn a_short_line_is_not_marked_truncated() {
    assert!(!snippet("let x = 1;", None).truncated);
}

#[test]
fn a_line_of_only_whitespace_becomes_empty_rather_than_vanishing() {
    let snippet = snippet("\t   ", None);
    assert_eq!("", snippet.text);
    assert_eq!(None, snippet.highlight);
}

#[test]
fn a_carriage_return_left_on_the_line_is_removed() {
    // Lines split on `\n` from a CRLF file keep the `\r`, which renders as a
    // stray glyph or eats the rest of the row depending on the terminal.
    assert_eq!("let x = 1;", snippet("  let x = 1;\r", None).text);
}

#[test]
fn a_snippet_never_splits_a_character_in_half() {
    // Slicing a String at a non-boundary panics, and a long line of non-ASCII
    // is exactly where a byte-based cut would do it.
    let line = "é".repeat(500);
    let snippet = snippet(&line, Some((250, 251)));
    assert!(snippet.text.chars().all(|c| c == 'é'), "{}", snippet.text);
}

// ---------------------------------------------------------------------------
// `byte_offset` — the only whole-document arithmetic in this app.
// ---------------------------------------------------------------------------

#[test]
fn the_start_of_the_document_is_offset_zero() {
    assert_eq!(0, byte_offset("let x = 1;\nlet y = 2;\n", 0, 0));
}

#[test]
fn a_position_inside_a_later_line_counts_the_lines_before_it() {
    let text = "abc\ndefgh\nij\n";
    // Line 1 starts at byte 4; character 3 is three bytes further in.
    assert_eq!(7, byte_offset(text, 1, 3));
}

#[test]
fn a_column_past_the_end_of_a_line_clamps_to_that_lines_end() {
    let text = "abc\ndef\n";
    // Not the start of the next line: a range ending "past" a line is a common
    // spelling for end-of-line, and resolving it into the following line would
    // silently widen every such edit by one character.
    assert_eq!(3, byte_offset(text, 0, 99));
}

#[test]
fn an_overrunning_column_on_a_crlf_line_stops_before_the_carriage_return() {
    // The rule that keeps CRLF files intact. Resolving to *after* the `\r` would
    // let a replacement land between the carriage return and the newline,
    // leaving a lone `\r` in the middle of the file.
    let text = "abc\r\ndef\r\n";
    assert_eq!(3, byte_offset(text, 0, 99));
    assert_eq!(3, byte_offset(text, 0, 3));
}

#[test]
fn a_line_past_the_end_of_the_document_clamps_to_the_end() {
    let text = "abc\ndef";
    assert_eq!(text.len(), byte_offset(text, 500, 0));
}

#[test]
fn the_empty_document_has_only_offset_zero() {
    assert_eq!(0, byte_offset("", 0, 0));
    assert_eq!(0, byte_offset("", 0, 20));
    assert_eq!(0, byte_offset("", 7, 20));
}

#[test]
fn a_document_ending_in_a_newline_has_an_addressable_final_empty_line() {
    // `"a\n"` is two lines by LSP's counting, and the second is empty. An
    // insertion there appends to the file, so the position must resolve.
    let text = "a\n";
    assert_eq!(2, byte_offset(text, 1, 0));
}

#[test]
fn a_column_is_utf16_code_units_and_not_bytes_or_chars() {
    // `é` is 2 bytes and 1 unit, `€` is 3 and 1, and the emoji is 4 and *2*.
    let text = "é€\u{1F600}x";
    assert_eq!(0, byte_offset(text, 0, 0));
    assert_eq!(2, byte_offset(text, 0, 1));
    assert_eq!(5, byte_offset(text, 0, 2));
    assert_eq!(9, byte_offset(text, 0, 4));
}

#[test]
fn a_column_landing_inside_a_surrogate_pair_clamps_back_to_the_character() {
    // There is no byte boundary in the middle of an astral character, and the
    // alternative to clamping is a panic inside a command.
    let text = "\u{1F600}x";
    assert_eq!(0, byte_offset(text, 0, 1));
    assert_eq!(4, byte_offset(text, 0, 2));
}

#[test]
fn a_lone_carriage_return_still_terminates_a_line() {
    // Old-Mac line endings are rare and are not a reason to panic or to run two
    // lines together.
    let text = "abc\rdef";
    assert_eq!(4, byte_offset(text, 1, 0));
}

#[test]
fn no_position_can_panic_the_offset_lookup() {
    let documents = ["", "\n", "\r\n", "a", "é€\u{1F600}", "a\nb\r\nc\rd\n"];
    for text in documents {
        for line in [0u32, 1, 2, 7, u32::MAX] {
            for character in [0u32, 1, 3, 99, u32::MAX] {
                let offset = byte_offset(text, line, character);
                assert!(offset <= text.len(), "{text:?} {line} {character}");
                assert!(
                    text.is_char_boundary(offset),
                    "{text:?} {line} {character} -> {offset}"
                );
            }
        }
    }
}

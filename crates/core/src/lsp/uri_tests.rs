use super::*;
use std::path::PathBuf;

/// A Windows path, spelled the way `PathBuf::from` keeps it on every platform.
fn win(path: &str) -> PathBuf {
    PathBuf::from(path)
}

#[test]
fn a_drive_path_encodes_the_colon_when_asked_to() {
    assert_eq!(
        Some("file:///C%3A/x/y.cs".to_string()),
        to_file_uri(&win(r"C:\x\y.cs"), UriStyle::Encoded)
    );
}

#[test]
fn roslyn_receives_the_plain_colon_spelling() {
    // Verified against the real server this session: an `initialize` carrying
    // `file:///C:/Users/.../code-basics` and a `didOpen` of
    // `file:///C:/.../Collections.cs` resolved a cross-file reference
    // correctly. The C# extension itself serialises with
    // `vscode.Uri.toString(true)`, i.e. skipEncoding, so the colon is plain
    // there too. The common factor is that the colon is *not* percent-encoded.
    assert_eq!(
        Some("file:///C:/x/y.cs".to_string()),
        to_file_uri(&win(r"C:\x\y.cs"), UriStyle::Plain)
    );
}

#[test]
fn every_spelling_of_the_colon_and_the_drive_parses() {
    // Four spellings are all in the wild: rust-analyzer sends `%3A`, the C#
    // extension sends a plain colon and a lower-case drive, and this app sends
    // a plain colon with the drive as the workspace scan spelled it. All four
    // must decode, and the drive's case is preserved rather than normalised —
    // Windows does not care, and folding it would make our path differ from the
    // one `source_walker` produced for the very same file.
    for (uri, expected) in [
        ("file:///C%3A/x/y.cs", r"C:\x\y.cs"),
        ("file:///C:/x/y.cs", r"C:\x\y.cs"),
        ("file:///c%3A/x/y.cs", r"c:\x\y.cs"),
        ("file:///c:/x/y.cs", r"c:\x\y.cs"),
    ] {
        assert_eq!(Some(win(expected)), from_file_uri(uri), "{uri}");
    }
}

#[test]
fn the_drive_letter_survives_a_round_trip_in_both_styles() {
    for style in [UriStyle::Encoded, UriStyle::Plain] {
        let uri = to_file_uri(&win(r"C:\repo\src\main.rs"), style).expect("a drive path");
        assert_eq!(
            Some(win(r"C:\repo\src\main.rs")),
            from_file_uri(&uri),
            "{style:?}"
        );
    }
}

#[test]
fn a_space_is_percent_encoded_in_both_styles() {
    // An unencoded space is not merely ugly: it is not a legal URI, and some
    // servers reject the whole notification rather than the one field.
    for style in [UriStyle::Encoded, UriStyle::Plain] {
        let uri = to_file_uri(&win(r"C:\Program Files\a.cs"), style).unwrap();
        assert!(uri.contains("Program%20Files"), "{style:?} gave {uri}");
        assert_eq!(Some(win(r"C:\Program Files\a.cs")), from_file_uri(&uri));
    }
}

#[test]
fn a_hash_is_encoded_because_an_unencoded_one_truncates_the_path() {
    // `file:///C:/a#b/c.cs` has the path `/C:/a` and the fragment `b/c.cs`. A
    // server would open the wrong file and report success.
    let uri = to_file_uri(&win(r"C:\a#b\c.cs"), UriStyle::Plain).unwrap();
    assert!(uri.contains("%23"), "expected the hash encoded, got {uri}");
    assert!(!uri.contains('#'), "no literal hash may survive: {uri}");
    assert_eq!(Some(win(r"C:\a#b\c.cs")), from_file_uri(&uri));
}

#[test]
fn a_question_mark_is_encoded_because_it_would_start_a_query() {
    let uri = to_file_uri(&win(r"C:\a?b\c.cs"), UriStyle::Plain).unwrap();
    assert!(
        uri.contains("%3F"),
        "expected the question mark encoded, got {uri}"
    );
    assert_eq!(Some(win(r"C:\a?b\c.cs")), from_file_uri(&uri));
}

#[test]
fn a_percent_in_a_real_filename_is_itself_encoded() {
    // Otherwise `100%.cs` becomes an invalid escape, and a filename containing
    // `%3A` would decode into a colon that was never there.
    let uri = to_file_uri(&win(r"C:\100%.cs"), UriStyle::Plain).unwrap();
    assert!(uri.contains("100%25"), "got {uri}");
    assert_eq!(Some(win(r"C:\100%.cs")), from_file_uri(&uri));
}

#[test]
fn non_ascii_is_encoded_as_utf8_bytes_and_decodes_back() {
    let uri = to_file_uri(&win(r"C:\проект\файл.rs"), UriStyle::Plain).unwrap();
    assert!(uri.is_ascii(), "a URI must be ASCII on the wire, got {uri}");
    assert_eq!(Some(win(r"C:\проект\файл.rs")), from_file_uri(&uri));
}

#[test]
fn a_posix_absolute_path_round_trips() {
    assert_eq!(
        Some("file:///home/dev/repo/src/main.rs".to_string()),
        to_file_uri(
            &PathBuf::from("/home/dev/repo/src/main.rs"),
            UriStyle::Plain
        )
    );
    assert_eq!(
        Some(PathBuf::from("/home/dev/a.rs")),
        from_file_uri("file:///home/dev/a.rs")
    );
}

#[test]
fn a_unc_path_round_trips_with_the_host_as_the_authority() {
    let uri = to_file_uri(&win(r"\\server\share\a.cs"), UriStyle::Plain).unwrap();
    assert_eq!("file://server/share/a.cs", uri);
    assert_eq!(Some(win(r"\\server\share\a.cs")), from_file_uri(&uri));
}

#[test]
fn the_localhost_authority_is_treated_as_no_authority() {
    // `file://localhost/c:/x` is a legal spelling of a local path and is not a
    // UNC share on a machine called "localhost".
    assert_eq!(
        Some(win(r"C:\x\y.cs")),
        from_file_uri("file://localhost/C:/x/y.cs")
    );
}

#[test]
fn an_encoded_backslash_is_read_as_a_separator() {
    // Some servers escape the separator itself. Leaving `%5C` in a path
    // component would produce a filename with a literal backslash in it, which
    // cannot exist on Windows — so nothing would open.
    assert_eq!(
        Some(win(r"C:\x\y.cs")),
        from_file_uri("file:///C%3A%5Cx%5Cy.cs")
    );
}

#[test]
fn a_literal_backslash_in_the_uri_is_read_as_a_separator() {
    assert_eq!(Some(win(r"C:\x\y.cs")), from_file_uri(r"file:///C:\x\y.cs"));
}

#[test]
fn a_non_file_scheme_yields_nothing_at_all() {
    // Roslyn emits these for decompiled metadata and source generators. There
    // is no path behind them, and inventing one would open an unrelated file
    // that happens to sit at the fabricated location.
    for uri in [
        "untitled:Untitled-1",
        "source-generated:///Foo/Bar.g.cs",
        "csharp:/metadata/System/String.cs",
        "https://example.com/a.cs",
        "",
        "file",
        "not a uri at all",
    ] {
        assert_eq!(None, from_file_uri(uri), "{uri} must not become a path");
    }
}

#[test]
fn the_scheme_is_matched_case_insensitively() {
    assert_eq!(Some(win(r"C:\a.cs")), from_file_uri("FILE:///C:/a.cs"));
}

#[test]
fn a_malformed_escape_is_refused_rather_than_half_decoded() {
    // `%zz` is not a byte. Dropping it, or passing it through literally, both
    // produce a path that names a different file than the server meant.
    for uri in [
        "file:///C:/a%zz.cs",
        "file:///C:/a%.cs",
        "file:///C:/a%4.cs",
    ] {
        assert_eq!(None, from_file_uri(uri), "{uri} must be refused");
    }
}

#[test]
fn an_escape_sequence_that_is_not_utf8_is_refused() {
    // A lone 0xFF byte is not valid UTF-8 and so is not a path this app can
    // hold. Lossy conversion would substitute U+FFFD and name a file that does
    // not exist.
    assert_eq!(None, from_file_uri("file:///C:/a%FF.cs"));
}

#[test]
fn a_relative_path_cannot_become_a_uri() {
    // A `file:` URI is absolute by definition. A relative path here means a
    // caller lost track of which root it was relative to — the defect class
    // this repository has hit repeatedly — so it is refused rather than joined
    // onto a guess.
    assert_eq!(
        None,
        to_file_uri(&PathBuf::from(r"src\main.rs"), UriStyle::Plain)
    );
    assert_eq!(
        None,
        to_file_uri(&PathBuf::from("src/main.rs"), UriStyle::Plain)
    );
    assert_eq!(None, to_file_uri(&PathBuf::from(""), UriStyle::Plain));
}

#[test]
fn a_bare_drive_relative_path_is_refused() {
    // `C:src\main.rs` is relative to that drive's current directory, which is
    // not locatable from here. `architecture/graph.rs::is_rooted` refuses the
    // same spelling for the same reason.
    assert_eq!(
        None,
        to_file_uri(&PathBuf::from(r"C:src\main.rs"), UriStyle::Plain)
    );
}

#[test]
fn a_uri_with_no_path_yields_nothing() {
    for uri in ["file://", "file:///", "file://server"] {
        assert_eq!(None, from_file_uri(uri), "{uri} names no file");
    }
}

#[test]
fn forward_slashes_in_a_windows_path_are_accepted_on_the_way_out() {
    // libgit2 hands this app forward-slashed Windows paths, so the converter
    // must not assume backslashes.
    assert_eq!(
        Some("file:///C:/x/y.cs".to_string()),
        to_file_uri(&PathBuf::from("C:/x/y.cs"), UriStyle::Plain)
    );
}

#[test]
fn a_trailing_separator_on_a_directory_is_dropped() {
    // The workspace root arrives with one from `git2`'s `workdir()`, and a
    // `rootUri` with a trailing slash makes some servers compute a different
    // relative path for every file in the project.
    assert_eq!(
        Some("file:///C:/repo".to_string()),
        to_file_uri(&PathBuf::from(r"C:\repo\"), UriStyle::Plain)
    );
}

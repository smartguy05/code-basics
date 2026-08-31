//! Tests for [`super`]: the shapes a `.env` line comes in, and the two ways one
//! can fail to become a usable value.

use super::*;

fn entry<'a>(file: &'a EnvFile, key: &str) -> &'a EnvEntry {
    file.get(key)
        .unwrap_or_else(|| panic!("expected an entry for {key}, got {:?}", file.entries))
}

fn literal(file: &EnvFile, key: &str) -> String {
    match &entry(file, key).value {
        EnvValue::Literal { text } => text.clone(),
        other => panic!("expected {key} to be literal, got {other:?}"),
    }
}

#[test]
fn reads_a_bare_key_and_value() {
    let file = parse("DB_HOST=localhost\n");

    assert_eq!(literal(&file, "DB_HOST"), "localhost");
    assert_eq!(entry(&file, "DB_HOST").line, 1);
    assert!(file.problems.is_empty(), "{:?}", file.problems);
}

#[test]
fn an_export_prefix_is_stripped() {
    let file = parse("export DB_HOST=localhost\n");

    assert_eq!(literal(&file, "DB_HOST"), "localhost");
    assert!(
        file.get("export DB_HOST").is_none(),
        "the prefix must not survive in the key"
    );
    assert!(file.problems.is_empty(), "{:?}", file.problems);
}

#[test]
fn reads_a_single_and_a_double_quoted_value() {
    let file = parse("A='one two'\nB=\"three four\"\n");

    assert_eq!(literal(&file, "A"), "one two");
    assert_eq!(literal(&file, "B"), "three four");
    assert!(file.problems.is_empty(), "{:?}", file.problems);
}

#[test]
fn a_hash_inside_a_quoted_value_is_not_a_comment() {
    let file = parse("PW=\"p#ss word\"\nQ='a # b'\n");

    assert_eq!(literal(&file, "PW"), "p#ss word");
    assert_eq!(literal(&file, "Q"), "a # b");
    assert!(file.problems.is_empty(), "{:?}", file.problems);
}

#[test]
fn a_trailing_comment_is_stripped_from_an_unquoted_value() {
    let file = parse("HOST=localhost # the dev box\n");

    assert_eq!(literal(&file, "HOST"), "localhost");

    // A `#` that is not preceded by whitespace is part of the value: a
    // password is far likelier than a comment somebody forgot to space.
    let tight = parse("PW=abc#def\n");
    assert_eq!(literal(&tight, "PW"), "abc#def");
}

#[test]
fn an_interpolated_value_is_left_unresolved_rather_than_expanded() {
    let file = parse("A=${DB_HOST}\nB=$DB_HOST\nC=%DB_HOST%\nD=plain\n");

    for key in ["A", "B", "C"] {
        match &entry(&file, key).value {
            EnvValue::Unresolved { raw, reason } => {
                assert!(
                    raw.contains("DB_HOST"),
                    "the reference must survive verbatim, got {raw:?}"
                );
                assert!(!reason.is_empty(), "an unresolved value must say why");
            }
            other => panic!("expected {key} to be unresolved, got {other:?}"),
        }
    }

    assert_eq!(
        literal(&file, "D"),
        "plain",
        "a value with no reference is unaffected"
    );

    // The distinct-outcomes rule: an unresolved value is neither dropped nor
    // returned as if it were a real one.
    assert!(
        file.get("A").is_some(),
        "an unresolved entry is still listed"
    );
    assert_ne!(
        entry(&file, "A").value,
        EnvValue::Literal {
            text: "${DB_HOST}".into()
        },
        "an unresolved value must not read as a literal one"
    );
    assert!(!entry(&file, "A").value.is_usable());
    assert!(entry(&file, "D").value.is_usable());
}

#[test]
fn a_malformed_line_is_reported_with_its_line_number() {
    let file = parse("GOOD=1\nthis line has no equals\nALSO_GOOD=2\n");

    assert_eq!(literal(&file, "GOOD"), "1");
    assert_eq!(literal(&file, "ALSO_GOOD"), "2");
    assert_eq!(file.problems.len(), 1, "{:?}", file.problems);

    let problem = &file.problems[0];
    assert_eq!(problem.line, 2, "1-based, counting blanks and comments");
    assert_eq!(problem.kind, EnvProblemKind::NoAssignment);
    assert!(!problem.reason.is_empty());
    assert!(
        !problem.reason.contains("this line has no equals"),
        "a problem must not echo the line: it may hold a secret"
    );
}

#[test]
fn an_unterminated_quote_is_its_own_problem_not_a_missing_assignment() {
    let file = parse("A=\"open\n");

    assert!(file.get("A").is_none(), "nothing usable was parsed");
    assert_eq!(file.problems.len(), 1, "{:?}", file.problems);
    assert_eq!(file.problems[0].kind, EnvProblemKind::UnterminatedQuote);
    assert_eq!(file.problems[0].line, 1);
}

#[test]
fn crlf_and_a_bom_are_tolerated() {
    let file = parse("\u{feff}A=1\r\nB=2\r\n");

    assert_eq!(literal(&file, "A"), "1", "the BOM must not enter the key");
    assert_eq!(literal(&file, "B"), "2", "the CR must not enter the value");
    assert!(file.problems.is_empty(), "{:?}", file.problems);
}

#[test]
fn an_empty_value_is_a_value_not_a_missing_key() {
    let file = parse("EMPTY=\nQUOTED_EMPTY=\"\"\n");

    assert!(
        file.get("EMPTY").is_some(),
        "a key with an empty value is present"
    );
    assert_eq!(literal(&file, "EMPTY"), "");
    assert_eq!(literal(&file, "QUOTED_EMPTY"), "");
    assert!(file.get("ABSENT").is_none(), "an absent key is absent");
    assert!(file.problems.is_empty(), "{:?}", file.problems);
}

#[test]
fn blank_lines_and_comments_are_not_problems() {
    let file = parse("\n# a comment\n   \n   # indented comment\nA=1\n");

    assert_eq!(literal(&file, "A"), "1");
    assert_eq!(entry(&file, "A").line, 5);
    assert!(file.problems.is_empty(), "{:?}", file.problems);
}

#[test]
fn an_invalid_key_is_reported_rather_than_accepted() {
    let file = parse("not a key=1\n=1\n");

    assert!(file.entries.is_empty(), "{:?}", file.entries);
    assert_eq!(file.problems.len(), 2, "{:?}", file.problems);
    assert_eq!(file.problems[0].kind, EnvProblemKind::InvalidKey);
    assert_eq!(file.problems[1].kind, EnvProblemKind::EmptyKey);
}

#[test]
fn a_later_assignment_wins_and_the_earlier_one_is_still_listed() {
    let file = parse("A=first\nA=second\n");

    assert_eq!(literal(&file, "A"), "second");
    assert_eq!(file.entries.len(), 2, "both lines are reported");
}

#[test]
fn a_double_quoted_value_honours_escapes_and_a_single_quoted_one_does_not() {
    let file = parse("A=\"a\\nb\"\nB='a\\nb'\n");

    assert_eq!(literal(&file, "A"), "a\nb");
    assert_eq!(literal(&file, "B"), "a\\nb");
}

#[test]
fn an_unresolved_reason_never_quotes_text_from_the_value() {
    // A password containing a `$` is enough to make the eager reference
    // detector fire, and the matched text is then a fragment of the secret.
    let file = parse("CONN=Host=db;Password=pa$sword123\n");

    let EnvValue::Unresolved { reason, .. } = &entry(&file, "CONN").value else {
        panic!(
            "expected CONN to be unresolved, got {:?}",
            entry(&file, "CONN").value
        );
    };

    for fragment in ["sword123", "$sword", "pa$", "Password", "db"] {
        assert!(
            !reason.contains(fragment),
            "a reason must not carry text read out of a value; {reason:?} contains {fragment:?}"
        );
    }

    // The same holds for a value that never came from a `.env` line.
    let EnvValue::Unresolved { reason, .. } = classify_value("Server=%SECRET_HOST%".into()) else {
        panic!("expected an unresolved value");
    };
    assert!(
        !reason.contains("SECRET_HOST") && !reason.contains("Server"),
        "not even a variable name may cross: it is still text from the value ({reason:?})"
    );
}

#[test]
fn the_reason_names_the_syntax_class_it_found() {
    let cases = [
        ("A=${DB_HOST}\n", "A", "${NAME}"),
        ("B=$DB_HOST\n", "B", "$NAME"),
        ("C=%DB_HOST%\n", "C", "%NAME%"),
    ];

    let mut reasons = Vec::new();
    for (text, key, syntax) in cases {
        let file = parse(text);
        let EnvValue::Unresolved { reason, .. } = &entry(&file, key).value else {
            panic!("expected {key} to be unresolved");
        };
        assert!(
            reason.contains(syntax),
            "the reason must name the syntax class {syntax:?}, got {reason:?}"
        );
        assert!(
            !reason.contains("DB_HOST"),
            "the syntax class is named, the match is not quoted: {reason:?}"
        );
        reasons.push(reason.clone());
    }

    // Distinct outcomes stay distinct: three syntaxes, three descriptions.
    reasons.sort();
    reasons.dedup();
    assert_eq!(
        reasons.len(),
        3,
        "each syntax class gets its own description"
    );
}

#[test]
fn no_problem_reason_echoes_the_line_it_could_not_read() {
    // A malformed line is exactly where a secret is likeliest to be mistyped, so
    // every one of the five kinds is checked, not just the one with an existing
    // test. Each line carries the same sentinel.
    let file = parse(concat!(
        "Password:hunter2secret\n", // 1: NoAssignment
        "=hunter2secret\n",         // 2: EmptyKey
        "9KEY=hunter2secret\n",     // 3: InvalidKey
        "A=\"hunter2secret\n",      // 4: UnterminatedQuote
        "B=\"x\" hunter2secret\n",  // 5: TrailingCharacters
    ));

    let kinds: Vec<_> = file.problems.iter().map(|p| p.kind).collect();
    assert_eq!(
        kinds,
        vec![
            EnvProblemKind::NoAssignment,
            EnvProblemKind::EmptyKey,
            EnvProblemKind::InvalidKey,
            EnvProblemKind::UnterminatedQuote,
            EnvProblemKind::TrailingCharacters,
        ],
        "five distinct mistakes stay five distinct answers: {:?}",
        file.problems
    );

    for (index, problem) in file.problems.iter().enumerate() {
        assert_eq!(problem.line, index as u32 + 1);
        assert!(
            !problem.reason.contains("hunter2secret"),
            "a problem may name the line number, never its content: {:?}",
            problem.reason
        );
        assert!(
            problem.reason.contains(&format!("{}", problem.line)),
            "a problem must name its line so the user can find it: {:?}",
            problem.reason
        );
    }
}

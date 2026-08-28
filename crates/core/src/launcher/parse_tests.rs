use super::*;

#[test]
fn a_bare_command_splits_into_program_and_args() {
    let (program, args) = split_command("docker compose up -d").unwrap();
    assert_eq!(program, "docker");
    assert_eq!(args, vec!["compose", "up", "-d"]);
}

#[test]
fn runs_of_whitespace_do_not_produce_empty_arguments() {
    let (program, args) = split_command("  node   -e   1  ").unwrap();
    assert_eq!(program, "node");
    assert_eq!(args, vec!["-e", "1"]);
}

#[test]
fn double_quotes_group_and_are_removed() {
    let (program, args) =
        split_command(r#""C:\Program Files\redis\redis-server.exe" --port 6380"#).unwrap();
    assert_eq!(program, r"C:\Program Files\redis\redis-server.exe");
    assert_eq!(args, vec!["--port", "6380"]);
}

#[test]
fn a_backslash_before_a_quote_escapes_it_but_a_path_separator_survives() {
    // The whole reason this is not a general backslash escape: `C:\repo\src` must
    // arrive unchanged, so only `\"` is special.
    let (program, args) = split_command(r#"node -e "console.log(\"hi\")" C:\repo\src"#).unwrap();
    assert_eq!(program, "node");
    assert_eq!(args, vec![r#"-e"#, r#"console.log("hi")"#, r"C:\repo\src"]);
}

#[test]
fn an_empty_command_is_an_error_not_an_empty_argv() {
    assert!(split_command("   ").unwrap_err().contains("command"));
    assert!(program_and_args("", false).is_err());
    assert!(program_and_args("\t", true).is_err());
}

#[test]
fn an_unbalanced_quote_is_refused_rather_than_guessed_at() {
    let error = split_command(r#"node -e "unterminated"#).unwrap_err();
    assert!(error.to_lowercase().contains("quote"), "{error}");
}

#[test]
fn an_unquoted_shell_metacharacter_is_refused_with_the_fix() {
    let error = split_command("echo hi | findstr hi").unwrap_err();
    // Naming both the character and the remedy: a bare argv would pass `|` to
    // `echo` as an argument, which is the silent misbehaviour to avoid.
    assert!(error.contains('|'), "{error}");
    assert!(error.to_lowercase().contains("shell"), "{error}");
}

#[test]
fn every_shell_metacharacter_is_recognised() {
    for line in [
        "a | b",
        "a > out.txt",
        "a < in.txt",
        "a && b",
        "a || b",
        "a ; b",
        "a & b",
    ] {
        assert!(split_command(line).is_err(), "{line} should need a shell");
    }
}

#[test]
fn a_metacharacter_inside_quotes_is_just_text() {
    let (program, args) = split_command(r#"grep "a|b" file.txt"#).unwrap();
    assert_eq!(program, "grep");
    assert_eq!(args, vec!["a|b", "file.txt"]);
}

#[test]
fn shell_mode_passes_the_whole_line_after_the_platform_flag() {
    let args = shell_args("  echo hi | findstr hi  ");
    assert_eq!(args.len(), 2);
    assert_eq!(args[0], shell_flag());
    assert_eq!(args[1], "echo hi | findstr hi");
}

#[test]
fn shell_mode_accepts_what_argv_mode_refuses() {
    let (program, args) = program_and_args("echo hi | findstr hi", true).unwrap();
    assert!(!program.is_empty());
    assert_eq!(args[1], "echo hi | findstr hi");
}

#[test]
fn the_platform_shell_flag_matches_the_platform_shell() {
    #[cfg(windows)]
    assert_eq!(shell_flag(), "/C");
    #[cfg(not(windows))]
    assert_eq!(shell_flag(), "-c");
}

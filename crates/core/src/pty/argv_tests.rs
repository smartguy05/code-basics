use super::*;
use std::path::PathBuf;

fn args(list: &[&str]) -> Vec<String> {
    list.iter().map(|s| s.to_string()).collect()
}

// --- is_batch_target -------------------------------------------------------

#[test]
fn a_cmd_shim_is_a_batch_target() {
    assert!(is_batch_target(&PathBuf::from(
        r"C:\Users\me\AppData\Roaming\npm\codex.cmd"
    )));
}

#[test]
fn a_bat_file_is_a_batch_target() {
    assert!(is_batch_target(&PathBuf::from(r"C:\tools\agent.bat")));
}

#[test]
fn the_extension_is_matched_case_insensitively() {
    // The filesystem does not care about case, so neither may the guard —
    // `CODEX.CMD` is the same shim and the same hazard.
    assert!(is_batch_target(&PathBuf::from(r"C:\npm\CODEX.CMD")));
    assert!(is_batch_target(&PathBuf::from(r"C:\npm\agent.Bat")));
}

#[test]
fn an_exe_is_not_a_batch_target() {
    assert!(!is_batch_target(&PathBuf::from(
        r"C:\Program Files\claude\claude.exe"
    )));
}

#[test]
fn an_extensionless_program_is_not_a_batch_target() {
    // The unix case, and the "resolve_program found nothing" case: no
    // re-parsing shell is involved, so nothing is refused.
    assert!(!is_batch_target(&PathBuf::from("/usr/local/bin/claude")));
}

#[test]
fn a_name_merely_containing_cmd_is_not_a_batch_target() {
    // Judged on the extension, never on the text of the name — `cmdline-tool`
    // is an ordinary program.
    assert!(!is_batch_target(&PathBuf::from(r"C:\bin\cmdline-tool.exe")));
    assert!(!is_batch_target(&PathBuf::from(r"C:\bin\cmd\tool")));
}

// --- batch_argv_refusal ----------------------------------------------------

#[test]
fn ordinary_prose_is_not_refused() {
    // The overwhelming majority of questions. If this ever fails, the guard has
    // become a feature switch rather than a guard.
    assert_eq!(
        batch_argv_refusal("Why does the run tab flash when a build fails?"),
        None
    );
    assert_eq!(
        batch_argv_refusal("Explain crates/core/src/pty/mod.rs, line 84 (open_inner)"),
        None
    );
}

#[test]
fn each_cmd_metacharacter_is_refused_and_named() {
    for c in ['&', '|', '<', '>', '^', '"', '%'] {
        let arg = format!("what does {c} do here");
        let reason = batch_argv_refusal(&arg)
            .unwrap_or_else(|| panic!("`{c}` should be refused for a batch target"));
        assert!(
            reason.contains(&format!("`{c}`")),
            "refusal for `{c}` must name the character, got: {reason}"
        );
    }
}

#[test]
fn a_metacharacter_with_no_space_around_it_is_still_refused() {
    // The precise hazard: `CommandBuilder` only quotes an argument containing
    // whitespace or a quote, so a space-free `&` reaches cmd.exe bare and
    // separates commands.
    assert!(batch_argv_refusal("a&b").is_some());
}

#[test]
fn a_percent_expansion_is_refused_rather_than_silently_expanded() {
    // The quietest failure of the set: cmd.exe expands `%TEMP%` even inside
    // quotes, so the agent would answer a question the user never asked with no
    // signal that anything changed.
    let reason = batch_argv_refusal("what is in %TEMP%").expect("`%` must be refused");
    assert!(
        reason.contains("environment variable"),
        "the reason must say why, got: {reason}"
    );
}

#[test]
fn a_newline_is_refused_for_a_batch_target() {
    // A multi-line question is quoted by CommandBuilder, but cmd.exe cannot
    // carry a newline inside a quoted argument at all.
    let reason = batch_argv_refusal("first line\nsecond line").expect("a newline must be refused");
    assert!(
        reason.contains("U+000A"),
        "the reason must name the character, got: {reason}"
    );
}

#[test]
fn a_control_character_is_refused_for_a_batch_target() {
    assert!(batch_argv_refusal("bell\u{7}here").is_some());
    assert!(batch_argv_refusal("tab\there").is_some());
}

#[test]
fn the_refusal_says_how_to_proceed() {
    // Matching `launcher::parse`'s style: a refusal that does not name the fix
    // reads as a broken feature.
    let reason = batch_argv_refusal("a & b").unwrap();
    assert!(
        reason.contains("rephrase"),
        "the refusal must name the fix, got: {reason}"
    );
}

// --- check_batch_argv: the asymmetry ---------------------------------------

#[test]
fn a_hazardous_argument_is_refused_for_a_batch_target() {
    let err = check_batch_argv(
        &PathBuf::from(r"C:\npm\codex.cmd"),
        &args(&["does %PATH% & friends matter"]),
    )
    .expect_err("a batch target must refuse a cmd metacharacter");
    assert!(
        err.contains("codex.cmd"),
        "the error must name the program, got: {err}"
    );
}

#[test]
fn the_same_argument_is_allowed_for_a_real_executable() {
    // The whole point. MSVC argv quoting is correct for a `.exe`: nothing
    // re-parses the command line, so the question crosses verbatim and
    // refusing it would be a regression, not a fix.
    assert_eq!(
        check_batch_argv(
            &PathBuf::from(r"C:\Program Files\claude\claude.exe"),
            &args(&["does %PATH% & friends matter"]),
        ),
        Ok(())
    );
    assert_eq!(
        check_batch_argv(
            &PathBuf::from("/usr/local/bin/claude"),
            &args(&["a \"quoted\" ^ question\nover two lines"]),
        ),
        Ok(())
    );
}

#[test]
fn a_batch_target_with_ordinary_arguments_is_allowed() {
    assert_eq!(
        check_batch_argv(
            &PathBuf::from(r"C:\npm\codex.cmd"),
            &args(&["--model", "gpt-5", "Why is the build slow?"]),
        ),
        Ok(())
    );
}

#[test]
fn a_batch_target_with_no_arguments_is_allowed() {
    // An ordinary terminal spawning a shim with no argv has nothing to refuse.
    assert_eq!(
        check_batch_argv(&PathBuf::from(r"C:\npm\codex.cmd"), &[]),
        Ok(())
    );
}

#[test]
fn the_first_offending_argument_decides() {
    let err = check_batch_argv(
        &PathBuf::from(r"C:\npm\codex.cmd"),
        &args(&["--model", "gpt-5", "a | b"]),
    )
    .expect_err("a later argument is checked too");
    assert!(err.contains("`|`"), "got: {err}");
}

// --- check_batch_argv: the program path is on the same command line --------

#[test]
fn a_batch_program_path_containing_a_metacharacter_is_refused() {
    // `portable_pty`'s `CommandBuilder` applies the same MSVC quoting to the
    // exe path as to the arguments (`cmdbuilder.rs:679`), so a directory named
    // `dev&test` splits the command line before the program is reached: cmd.exe
    // runs `...\dev` and then `test\t.cmd` as a second command. A PATH entry
    // named `foo&calc` would therefore run `calc`.
    let err = check_batch_argv(
        &PathBuf::from(r"C:\tools\dev&test\agent.cmd"),
        &args(&["Why is the build slow?"]),
    )
    .expect_err("a hazard in the program path must be refused");
    assert!(
        err.contains("`&`"),
        "the error must name the character: {err}"
    );
}

#[test]
fn a_percent_in_a_batch_program_path_is_refused() {
    // The same quiet failure as `%TEMP%` in an argument, one position earlier.
    assert!(check_batch_argv(&PathBuf::from(r"C:\%USER%\agent.cmd"), &[]).is_err());
}

#[test]
fn an_ordinary_batch_program_path_is_allowed() {
    // The guard must not become a feature switch: a normal install path, spaces
    // and all, is spawned unchanged.
    assert_eq!(
        check_batch_argv(
            &PathBuf::from(r"C:\Program Files\nodejs\node_modules\npm\codex.cmd"),
            &args(&["Why is the build slow?"]),
        ),
        Ok(())
    );
}

#[test]
fn a_hazardous_program_path_is_allowed_for_a_real_executable() {
    // Same asymmetry as the arguments: nothing re-parses the line for a `.exe`.
    assert_eq!(
        check_batch_argv(&PathBuf::from(r"C:\tools\dev&test\agent.exe"), &[]),
        Ok(())
    );
}

#[test]
fn the_refusal_distinguishes_a_bad_program_path_from_a_bad_argument() {
    // Two distinct causes must not collapse into one message: the user can
    // rephrase a question, but the fix for a program path is to move or
    // reinstall it, and a message naming the wrong one sends them at the
    // wrong thing.
    let path_err = check_batch_argv(&PathBuf::from(r"C:\tools\dev&test\agent.cmd"), &[])
        .expect_err("program path hazard");
    let arg_err = check_batch_argv(&PathBuf::from(r"C:\npm\codex.cmd"), &args(&["a & b"]))
        .expect_err("argument hazard");

    assert_ne!(path_err, arg_err, "the two causes must read differently");
    assert!(
        path_err.contains("program path"),
        "the path refusal must say it is the program path: {path_err}"
    );
    assert!(
        !arg_err.contains("program path"),
        "the argument refusal must not blame the program path: {arg_err}"
    );
    assert!(
        arg_err.contains("argument"),
        "the argument refusal must say it is an argument: {arg_err}"
    );
}

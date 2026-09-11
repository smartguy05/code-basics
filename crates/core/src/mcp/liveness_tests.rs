use super::*;

use std::path::PathBuf;

#[test]
fn the_same_path_matches() {
    assert!(same_executable(
        &PathBuf::from(r"C:\apps\code-basics\cb-app.exe"),
        r"C:\apps\code-basics\cb-app.exe"
    ));
}

#[test]
fn case_and_separators_do_not_make_it_a_different_executable() {
    assert!(same_executable(
        &PathBuf::from(r"c:\apps\code-basics\cb-app.exe"),
        r"C:\Apps\Code-Basics\CB-App.exe"
    ));
    assert!(same_executable(
        &PathBuf::from("C:/apps/cb-app.exe"),
        r"C:\apps\cb-app.exe"
    ));
}

#[test]
fn a_different_executable_at_the_same_pid_does_not_match() {
    assert!(!same_executable(
        &PathBuf::from(r"C:\Windows\System32\notepad.exe"),
        r"C:\apps\code-basics\cb-app.exe"
    ));
}

#[test]
fn an_empty_recorded_path_matches_nothing() {
    assert!(!same_executable(&PathBuf::from(r"C:\anything.exe"), ""));
    assert!(!same_executable(&PathBuf::from(r"C:\anything.exe"), "   "));
}

#[test]
fn a_pid_that_cannot_exist_is_not_alive() {
    // pid 0 is the System Idle Process and is never this application.
    assert!(!alive_by(0, r"C:\apps\code-basics\cb-app.exe"));
}

#[test]
fn this_process_is_alive_under_its_own_executable_and_not_under_another() {
    let Ok(exe) = std::env::current_exe() else {
        return;
    };
    assert!(
        alive_by(std::process::id(), &exe.display().to_string()),
        "this process must probe as alive"
    );
    assert!(
        !alive_by(std::process::id(), r"C:\Windows\System32\notepad.exe"),
        "a live pid running a different executable must not be accepted"
    );
}
